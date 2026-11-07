/**
 * The interactive skin preview: a rigged GLTF player under a render loop, with
 * drag-to-rotate, the idle/random animation set, and the click interaction
 * (`interact` clip + impulse shake, damage flash when clicked repeatedly).
 */
import * as THREE from 'three';

import type { SkinVariant } from '@/api';
import { spotlightMaterial, syncDamageFlash } from './shaders';
import {
  applyCapeTexture,
  applyTexture,
  cloneModel,
  createTransparentTexture,
  disposeMaterials,
  loadModel,
  loadTexture,
  measure,
  modelUrl,
} from './skin-rendering';

const BASE_ANIMATION = 'idle';
const RANDOM_ANIMATIONS = ['idle_sub_1', 'idle_sub_2', 'idle_sub_3'];
const INTERACT_ANIMATION = 'interact';
const RANDOM_INTERVAL_MS = 8000;
const TRANSITION = 0.2;
const INTERACT_DURATION = 0.5;

const IMPULSE_MAX_ENERGY = 5;
const IMPULSE_PER_CLICK = 1;
const IMPULSE_DECAY_PER_SECOND = 2;
const IMPULSE_BASE_SPEED = 18;
const IMPULSE_SPEED_BOOST = 7;
const IMPULSE_OFFSET_X = 0.035;
const IMPULSE_ROTATION_Z = 0.055;
const IMPULSE_SCALE_X = 0.018;
const IMPULSE_SCALE_Y = 0.025;
const FLASH_DURATION = 0.2;
const FLASH_REPEAT_DELAY = 0.5;
const FLASH_MAX_INTENSITY = 0.7;

const FIT_FOV = 35;
const FIT_ZOOM = 1;
const FIT_PADDING = { top: 0.1, right: 0.1, bottom: 0.18, left: 0.1 };

const DRAG_TO_RADIANS = 0.01;
/** Three-quarter view: dead-on reads flat, and hides the arm silhouette. */
const INITIAL_ROTATION = Math.PI + 0.38;

/**
 * `loadedCape` before any cape has been applied, which `undefined` cannot say —
 * it already means "no cape". Without the distinction a fresh model with no
 * cape short-circuits the diff in `setCape` and keeps the GLTF's own cape
 * material: opaque white, and visible.
 */
const CAPE_UNSET = Symbol('cape-unset');

export interface SkinPreviewOptions {
  canvas: HTMLCanvasElement;
  container: HTMLElement;
  variant: SkinVariant;
  texture: string;
  cape?: string;
}

export class SkinPreview {
  private readonly renderer: THREE.WebGLRenderer;
  private readonly scene = new THREE.Scene();
  private readonly camera: THREE.PerspectiveCamera;
  private readonly rotated = new THREE.Group();
  private readonly centered = new THREE.Group();
  private readonly spotlight: THREE.Mesh;
  private readonly transparent = createTransparentTexture();
  private readonly timer = new THREE.Timer();
  private readonly resize: ResizeObserver;
  private readonly container: HTMLElement;

  private model: THREE.Object3D | null = null;
  private mixer: THREE.AnimationMixer | null = null;
  private actions = new Map<string, THREE.AnimationAction>();
  private current = '';
  private randomTimer: number | null = null;
  private lastRandom = '';

  private size = new THREE.Vector3(1, 2, 1);

  private rotation = INITIAL_ROTATION;
  private dragX: number | null = null;
  private dragged = false;

  private energy = 0;
  private phase = 0;
  private flash = 0;
  private flashCooldown = 0;
  private flashIntensity = 0;

  private frame: number | null = null;
  private disposed = false;
  private modelVersion = 0;
  private capeVersion = 0;
  private loadedCape: string | undefined | typeof CAPE_UNSET = CAPE_UNSET;

  constructor(options: SkinPreviewOptions) {
    this.container = options.container;
    this.renderer = new THREE.WebGLRenderer({
      canvas: options.canvas,
      antialias: true,
      alpha: true,
    });
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.renderer.toneMapping = THREE.NoToneMapping;
    this.renderer.toneMappingExposure = 10;
    this.renderer.setClearColor(0x000000, 0);
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 1.5));

    this.camera = new THREE.PerspectiveCamera(FIT_FOV, 1, 0.1, 1000);
    this.rotated.add(this.centered);
    this.scene.add(this.rotated);
    this.scene.add(new THREE.AmbientLight(0xffffff, 2));

    const light = new THREE.DirectionalLight(0xffffff, 1.2);
    light.position.set(-3, 4, -2);
    this.scene.add(light);

    this.spotlight = new THREE.Mesh(
      new THREE.CircleGeometry(1, 128),
      spotlightMaterial(),
    );
    this.spotlight.rotation.x = -Math.PI / 2;
    this.scene.add(this.spotlight);

    this.resize = new ResizeObserver(() => this.refit());
    this.resize.observe(this.container);
    this.refit();

    void this.load(options.variant, options.texture, options.cape);
    this.frame = requestAnimationFrame(this.tick);
  }

  async load(
    variant: SkinVariant,
    texture: string,
    cape?: string,
  ): Promise<void> {
    const version = ++this.modelVersion;
    const [gltf, skin] = await Promise.all([
      loadModel(modelUrl(variant)),
      loadTexture(texture),
    ]);
    if (this.disposed || version !== this.modelVersion) return;

    const model = cloneModel(gltf);
    applyTexture(model, skin);

    // Measured detached: `measure` works in world space, so under the centering
    // group it would fold the previous model's offset into the new one.
    const box = measure(model);

    this.clearModel();
    this.model = model;
    this.size = box.size;
    this.centered.position.set(-box.center.x, -box.center.y, -box.center.z);
    this.centered.add(model);

    this.initAnimations(model, gltf.animations);
    syncDamageFlash(model, this.flashIntensity);
    this.loadedCape = CAPE_UNSET;
    await this.setCape(cape);
    this.refit();
  }

  async setCape(cape?: string): Promise<void> {
    if (cape === this.loadedCape) return;
    const version = ++this.capeVersion;
    const texture = cape ? await loadTexture(cape) : null;
    if (this.disposed || version !== this.capeVersion || !this.model) return;
    applyCapeTexture(this.model, texture, this.transparent);
    this.loadedCape = cape;
  }

  /** A press starts a drag; a release without movement is a click. */
  pointerDown(event: PointerEvent, target: HTMLElement): void {
    target.setPointerCapture(event.pointerId);
    this.dragX = event.clientX;
    this.dragged = false;
  }

  pointerMove(event: PointerEvent): void {
    if (this.dragX === null) return;
    this.rotation += (event.clientX - this.dragX) * DRAG_TO_RADIANS;
    this.dragX = event.clientX;
    this.dragged = true;
  }

  pointerUp(event: PointerEvent, target: HTMLElement): void {
    if (target.hasPointerCapture(event.pointerId)) {
      target.releasePointerCapture(event.pointerId);
    }
    this.dragX = null;
  }

  click(): void {
    if (this.dragged) {
      this.dragged = false;
      return;
    }
    this.energy = Math.min(IMPULSE_MAX_ENERGY, this.energy + IMPULSE_PER_CLICK);
    if (this.energy >= IMPULSE_MAX_ENERGY && this.flashCooldown <= 0) {
      this.flash = FLASH_DURATION;
      this.flashCooldown = FLASH_DURATION + FLASH_REPEAT_DELAY;
    }
    this.play(INTERACT_ANIMATION);
  }

  dispose(): void {
    this.disposed = true;
    this.resize.disconnect();
    if (this.frame !== null) cancelAnimationFrame(this.frame);
    this.clearRandomTimer();
    this.clearModel();
    this.transparent.dispose();
    this.spotlight.geometry.dispose();
    (this.spotlight.material as THREE.Material).dispose();
    this.renderer.dispose();
    this.timer.dispose();
  }

  private clearModel(): void {
    if (!this.model) return;
    this.mixer?.stopAllAction();
    this.mixer?.uncacheRoot(this.model);
    this.mixer = null;
    this.actions.clear();
    this.current = '';
    this.clearRandomTimer();
    this.centered.remove(this.model);
    disposeMaterials(this.model);
    this.model = null;
  }

  private initAnimations(
    model: THREE.Object3D,
    clips: THREE.AnimationClip[],
  ): void {
    if (clips.length === 0) return;

    this.mixer = new THREE.AnimationMixer(model);
    this.actions.clear();

    for (const clip of clips) {
      // The clip's tail is a long settle nobody sees.
      if (clip.name === INTERACT_ANIMATION) clip.duration = INTERACT_DURATION;
      const action = this.mixer.clipAction(clip);
      action.setLoop(THREE.LoopOnce, 1);
      action.clampWhenFinished = true;
      this.actions.set(clip.name, action);
    }

    this.mixer.addEventListener('finished', this.onAnimationFinished);

    if (this.actions.has(BASE_ANIMATION)) {
      this.play(BASE_ANIMATION, true);
      this.scheduleRandom();
    }
  }

  private onAnimationFinished = (): void => {
    if (this.current === BASE_ANIMATION) return;
    this.play(BASE_ANIMATION);
    this.scheduleRandom();
  };

  private play(name: string, immediate = false): void {
    const action = this.actions.get(name);
    if (!this.mixer || !action) return;
    if (this.current === name && action.isRunning() && name !== BASE_ANIMATION)
      return;

    for (const [other, instance] of this.actions) {
      if (other !== name && instance.isRunning()) instance.fadeOut(TRANSITION);
    }

    action.reset();
    if (name === BASE_ANIMATION) {
      action.setLoop(THREE.LoopRepeat, Number.POSITIVE_INFINITY);
    } else {
      action.setLoop(THREE.LoopOnce, 1);
      action.clampWhenFinished = true;
    }

    if (immediate) action.setEffectiveWeight(1);
    else action.fadeIn(TRANSITION);
    action.play();
    if (immediate) this.mixer.update(0);

    this.current = name;
  }

  private scheduleRandom(): void {
    this.clearRandomTimer();
    this.randomTimer = window.setTimeout(() => {
      const pool = RANDOM_ANIMATIONS.filter(
        (name) => this.actions.has(name) && name !== this.lastRandom,
      );
      const choices = pool.length > 0 ? pool : RANDOM_ANIMATIONS;
      const next = choices[Math.floor(Math.random() * choices.length)];
      if (this.current === BASE_ANIMATION && this.actions.has(next)) {
        this.lastRandom = next;
        this.play(next);
      } else {
        this.scheduleRandom();
      }
    }, RANDOM_INTERVAL_MS);
  }

  private clearRandomTimer(): void {
    if (this.randomTimer === null) return;
    window.clearTimeout(this.randomTimer);
    this.randomTimer = null;
  }

  private stepImpulse(delta: number): void {
    this.energy = Math.max(0, this.energy - IMPULSE_DECAY_PER_SECOND * delta);

    if (this.energy > 0) {
      const intensity = this.energy / IMPULSE_MAX_ENERGY;
      this.phase +=
        delta * (IMPULSE_BASE_SPEED + this.energy * IMPULSE_SPEED_BOOST);
      const shake = Math.sin(this.phase) * intensity;
      const squash = Math.abs(Math.sin(this.phase * 1.7)) * intensity;
      this.rotated.position.x = shake * IMPULSE_OFFSET_X;
      this.rotated.rotation.z = shake * IMPULSE_ROTATION_Z;
      this.rotated.scale.set(
        1 + squash * IMPULSE_SCALE_X,
        1 - squash * IMPULSE_SCALE_Y,
        1,
      );
    } else if (this.rotated.position.x !== 0 || this.rotated.rotation.z !== 0) {
      this.rotated.position.x = 0;
      this.rotated.rotation.z = 0;
      this.rotated.scale.set(1, 1, 1);
    }

    this.flashCooldown = Math.max(0, this.flashCooldown - delta);
    this.flash = Math.max(0, this.flash - delta);
    const next = FLASH_MAX_INTENSITY * (this.flash / FLASH_DURATION);
    if (next !== this.flashIntensity) {
      this.flashIntensity = next;
      syncDamageFlash(this.model, next);
    }
  }

  /** Frame the model to the container, padded for the drag hint below it. */
  private refit(): void {
    const width = Math.max(this.container.clientWidth, 1);
    const height = Math.max(this.container.clientHeight, 1);
    const aspect = width / height;

    this.renderer.setSize(width, height, false);

    const usableWidth = Math.max(
      width * (1 - FIT_PADDING.left - FIT_PADDING.right),
      1,
    );
    const usableHeight = Math.max(
      height * (1 - FIT_PADDING.top - FIT_PADDING.bottom),
      1,
    );

    const halfWidth = Math.sqrt(
      (this.size.x / 2) ** 2 + (this.size.z / 2) ** 2,
    );
    const halfHeight = this.size.y / 2;

    const verticalFov = THREE.MathUtils.degToRad(FIT_FOV);
    const horizontalFov = 2 * Math.atan(Math.tan(verticalFov / 2) * aspect);

    const paddedHalfWidth = halfWidth * (width / usableWidth);
    const paddedHalfHeight = halfHeight * (height / usableHeight);

    const distance =
      Math.max(
        paddedHalfHeight / Math.tan(verticalFov / 2),
        paddedHalfWidth / Math.tan(horizontalFov / 2),
      ) / FIT_ZOOM;

    const targetY =
      -(FIT_PADDING.bottom - FIT_PADDING.top) *
      (distance * Math.tan(verticalFov / 2));

    this.camera.fov = FIT_FOV;
    this.camera.aspect = aspect;
    this.camera.position.set(0, targetY, -distance);
    this.camera.lookAt(0, targetY, 0);
    this.camera.updateProjectionMatrix();

    const radius = Math.max(this.size.x, this.size.z, 1) * 0.8;
    this.spotlight.position.set(0, -this.size.y / 2 - 0.02, 0);
    this.spotlight.scale.set(radius, radius, radius);
  }

  private tick = (timestamp: number): void => {
    if (this.disposed) return;
    this.frame = requestAnimationFrame(this.tick);

    this.timer.update(timestamp);
    const delta = this.timer.getDelta();
    this.mixer?.update(delta);
    this.stepImpulse(delta);
    this.rotated.rotation.y = this.rotation;
    this.renderer.render(this.scene, this.camera);
  };
}
