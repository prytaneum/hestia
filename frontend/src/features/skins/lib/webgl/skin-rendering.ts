/** Model/texture loading and material setup for the GLTF player models. */
import * as THREE from 'three';
import type { GLTF } from 'three/examples/jsm/loaders/GLTFLoader.js';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';
import { clone as cloneSkeleton } from 'three/examples/jsm/utils/SkeletonUtils.js';

import type { SkinVariant } from '@/api';
import { remoteImage } from '@/lib/remote-image';
import classicModel from '../../assets/classic-player.gltf?url';
import slimModel from '../../assets/slim-player.gltf?url';

export function modelUrl(variant: SkinVariant): string {
  return variant === 'slim' ? slimModel : classicModel;
}

const models = new Map<string, GLTF>();
const modelLoads = new Map<string, Promise<GLTF>>();
const textures = new Map<string, THREE.Texture>();
const textureLoads = new Map<string, Promise<THREE.Texture>>();

export function loadModel(url: string): Promise<GLTF> {
  const cached = models.get(url);
  if (cached) return Promise.resolve(cached);

  const pending = modelLoads.get(url);
  if (pending) return pending;

  const loader = new GLTFLoader();
  const load = new Promise<GLTF>((resolve, reject) => {
    loader.load(
      url,
      (gltf) => {
        models.set(url, gltf);
        resolve(gltf);
      },
      undefined,
      reject,
    );
  }).finally(() => modelLoads.delete(url));

  modelLoads.set(url, load);
  return load;
}

export function loadTexture(url: string): Promise<THREE.Texture> {
  const cached = textures.get(url);
  if (cached) return Promise.resolve(cached);

  const pending = textureLoads.get(url);
  if (pending) return pending;

  const loader = new THREE.TextureLoader();
  loader.setCrossOrigin('anonymous');
  const load = new Promise<THREE.Texture>((resolve, reject) => {
    loader.load(
      remoteImage(url),
      (texture) => {
        texture.colorSpace = THREE.SRGBColorSpace;
        texture.flipY = false;
        texture.magFilter = THREE.NearestFilter;
        texture.minFilter = THREE.NearestFilter;
        textures.set(url, texture);
        resolve(texture);
      },
      undefined,
      reject,
    );
  }).finally(() => textureLoads.delete(url));

  textureLoads.set(url, load);
  return load;
}

function eachMaterial(
  model: THREE.Object3D,
  visit: (material: THREE.MeshStandardMaterial, mesh: THREE.Mesh) => void,
): void {
  model.traverse((child) => {
    const mesh = child as THREE.Mesh;
    if (!mesh.isMesh || !mesh.material) return;
    const materials = Array.isArray(mesh.material)
      ? mesh.material
      : [mesh.material];
    for (const material of materials) {
      if (material instanceof THREE.MeshStandardMaterial) visit(material, mesh);
    }
  });
}

function baseProperties(material: THREE.MeshStandardMaterial): void {
  material.metalness = 0;
  material.roughness = 1;
  material.color.set(0xffffff);
  material.depthTest = true;
  material.depthWrite = true;
  material.alphaTest = 0.1;
  material.flatShading = true;
  material.toneMapped = false;
}

export function applyTexture(
  model: THREE.Object3D,
  texture: THREE.Texture,
): void {
  eachMaterial(model, (material, mesh) => {
    if (material.name === 'cape') return;
    const layer = mesh.name.endsWith('_Layer');
    material.map = texture;
    material.side = THREE.FrontSide;
    material.transparent = layer;
    // The outer layer sits flush on the inner one; the offset keeps it from
    // z-fighting at every face.
    material.polygonOffset = layer;
    material.polygonOffsetFactor = layer ? SKIN_LAYER_DEPTH_BIAS : 0;
    material.polygonOffsetUnits = layer ? SKIN_LAYER_DEPTH_BIAS : 0;
    baseProperties(material);
    material.needsUpdate = true;
  });
}

const SKIN_LAYER_DEPTH_BIAS = -1;

export function applyCapeTexture(
  model: THREE.Object3D,
  texture: THREE.Texture | null,
  transparent?: THREE.Texture,
): void {
  eachMaterial(model, (material) => {
    if (material.name !== 'cape') return;
    material.map = texture ?? transparent ?? null;
    material.side = THREE.DoubleSide;
    material.transparent = !texture || !!transparent;
    baseProperties(material);
    material.visible = !!texture;
    material.needsUpdate = true;
  });
}

export function createTransparentTexture(): THREE.Texture {
  const canvas = document.createElement('canvas');
  canvas.width = 1;
  canvas.height = 1;
  canvas.getContext('2d')?.clearRect(0, 0, 1, 1);

  const texture = new THREE.CanvasTexture(canvas);
  texture.needsUpdate = true;
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.flipY = false;
  texture.magFilter = THREE.NearestFilter;
  texture.minFilter = THREE.NearestFilter;
  return texture;
}

/**
 * A private clone of the cached model: materials are per-instance state.
 * Cloned through `SkeletonUtils` so the animation clips bind to the copy's own
 * bones rather than the cached original's.
 */
export function cloneModel(gltf: GLTF): THREE.Object3D {
  const model = cloneSkeleton(gltf.scene);
  model.traverse((child) => {
    const mesh = child as THREE.Mesh;
    if (!mesh.isMesh || !mesh.material) return;
    mesh.material = Array.isArray(mesh.material)
      ? mesh.material.map((material) => material.clone())
      : mesh.material.clone();
  });
  return model;
}

export function disposeMaterials(model: THREE.Object3D | null): void {
  if (!model) return;
  const seen = new Set<THREE.Material>();
  eachMaterial(model, (material) => seen.add(material));
  for (const material of seen) material.dispose();
}

/** The tight box around the visible meshes — what the fit framing measures. */
export function measure(model: THREE.Object3D): {
  center: THREE.Vector3;
  size: THREE.Vector3;
} {
  model.updateWorldMatrix(true, true);

  const box = new THREE.Box3();
  const meshBox = new THREE.Box3();
  let found = false;

  model.traverse((child) => {
    const mesh = child as THREE.Mesh;
    if (!mesh.isMesh || !mesh.geometry || !mesh.visible) return;
    const materials = Array.isArray(mesh.material)
      ? mesh.material
      : [mesh.material];
    if (materials.length && materials.every((m) => m.visible === false)) return;
    if (!mesh.geometry.boundingBox) mesh.geometry.computeBoundingBox();
    if (!mesh.geometry.boundingBox) return;
    meshBox.copy(mesh.geometry.boundingBox).applyMatrix4(mesh.matrixWorld);
    box.union(meshBox);
    found = true;
  });

  if (!found || box.isEmpty()) {
    return {
      center: new THREE.Vector3(0, 1, 0),
      size: new THREE.Vector3(1, 2, 1),
    };
  }

  const center = new THREE.Vector3();
  const size = new THREE.Vector3();
  box.getCenter(center);
  box.getSize(size);
  size.set(
    Math.max(size.x, 0.001),
    Math.max(size.y, 0.001),
    Math.max(size.z, 0.001),
  );
  return { center, size };
}

/** Model + textures, ready to add to a scene. */
export async function setupSkinModel(
  variant: SkinVariant,
  texture: string,
  cape?: string,
): Promise<THREE.Object3D> {
  const [gltf, skin] = await Promise.all([
    loadModel(modelUrl(variant)),
    loadTexture(texture),
  ]);

  const model = cloneModel(gltf);
  applyTexture(model, skin);
  applyCapeTexture(
    model,
    cape ? await loadTexture(cape) : null,
    createTransparentTexture(),
  );
  return model;
}
