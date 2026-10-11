#!/usr/bin/env bash
# Build release artifacts locally, mirroring the release workflow.
#
#   scripts/package.sh            # sidecars + Tauri installers + portable archive
#   scripts/package.sh bundle     # Tauri installers only (deb/rpm/appimage or nsis)
#   scripts/package.sh portable   # portable archive only (.tar.gz on Linux, .zip on Windows)
#
# Tauri bundles the desktop app + the hestiad/hestia-tray/hestia sidecars into each
# installer. The portable archive is the same four binaries, but compiled with
# the `portable` feature, so they are a separate build rather than a copy of the
# installers' — see `portable()`.
. "$(dirname "$0")/lib/common.sh"

action="${1:-all}"

triple="$(rustc -vV | sed -n 's/^host: //p')"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) os=windows ;;
  Darwin) os=macos ;;
  *) os=linux ;;
esac

bundle() {
  scripts/sidecars.sh
  case "$os" in
    windows) targets="nsis" ;;
    macos) targets="app,dmg" ;;
    *) targets="deb,rpm,appimage" ;;
  esac
  # Signatures are minisign, applied to the finished artifacts by
  # scripts/sign.sh — the bundler never signs anything.
  (cd crates/desktop && cargo tauri build --bundles "$targets")
  if [ "$os" = linux ]; then
    unbundle_host_libraries
  fi
}

# The linuxdeploy tauri pins (git 659c9db, built 2024-07-26) predates
# libwayland-client.so.0 joining the AppImage excludelist, and that list is
# compiled into the binary — so it deploys the library, AppRun puts that copy
# ahead of the host's, and the host Mesa then fails EGL init against it
# ("Could not create default EGL display: EGL_BAD_PARAMETER"): the web process
# aborts and the window comes up blank. Tauri exposes no way to exclude a
# library (tauri-apps/tauri#15665), so drop it from the AppDir the bundler
# leaves behind and rebuild the image around it.
unbundle_host_libraries() {
  local dir=target/release/bundle/appimage
  local name appdir offset

  name="$(basename "$(find "$dir" -maxdepth 1 -name '*.AppImage' -print -quit)")"
  [ -n "$name" ] || die "no AppImage in $dir"
  appdir="$(find "$dir" -maxdepth 1 -name '*.AppDir' -print -quit)"
  [ -n "$appdir" ] || die "no AppDir in $dir; cannot rebuild the AppImage"

  if [ ! -e "$appdir/usr/lib/libwayland-client.so.0" ]; then
    log "no bundled libwayland-client; leaving $name as bundled"
    return 0
  fi
  command -v mksquashfs > /dev/null || die "mksquashfs (squashfs-tools) is required"

  log "unbundling host libraries from $name"
  rm -f "$appdir/usr/lib/libwayland-client.so.0"
  # An AppImage is its runtime ELF with the squashfs appended; the runtime
  # reports where that split is, so the original's runtime is reused verbatim.
  offset="$("$dir/$name" --appimage-offset)"
  head -c "$offset" "$dir/$name" > "$dir/runtime.bin"
  mksquashfs "$appdir" "$dir/payload.squashfs" \
    -root-owned -noappend -no-progress -comp gzip -b 128K > /dev/null
  cat "$dir/runtime.bin" "$dir/payload.squashfs" > "$dir/$name"
  chmod +x "$dir/$name"
  rm -f "$dir/runtime.bin" "$dir/payload.squashfs"
}

portable() {
  local ext="" name stage out
  [ "$os" = windows ] && ext=".exe"
  name="hestia-$version-$triple"
  stage="target/package/$name"
  # Its own target dir: a portable binary resolves a different data home, so it
  # must never end up in target/release/ where sidecars.sh stages the ones the
  # installers bundle.
  out="target/portable"

  # generate_context! embeds frontend/dist — the shell serves it, so a stale
  # one must not ship.
  (cd frontend && bun run build)

  # Two invocations, like the installers (sidecars.sh, then cargo tauri build): one
  # invocation unifies the shell's features into the tray, which then imports
  # a comctl32 v6 entry point no manifest asks for and cannot load.
  log "building portable binaries"
  cargo build --release --target-dir "$out" \
    -p cli -p daemon -p tray \
    --features cli/portable,daemon/portable,tray/portable
  # tauri-build asserts every externalBin exists on disk, but only the bundler
  # ships them and the archive copies its own — so the list is emptied rather
  # than compiling an installer sidecar set nothing here reads.
  TAURI_CONFIG='{"bundle":{"externalBin":[]}}' \
    cargo build --release --target-dir "$out" \
    -p desktop --features desktop/portable,desktop/custom-protocol

  rm -rf "$stage"
  mkdir -p "$stage/bin" "$stage/data"
  # The split the Windows installer lays down, under the names common::app's
  # DESKTOP_BIN and TRAY_BIN put first.
  if [ "$os" = windows ]; then
    cp "$out/release/hestia-desktop.exe" "$stage/Hestia.exe"
    cp "$out/release/hestia-tray.exe" "$stage/Hestia Tray.exe"
  else
    cp "$out/release/hestia-desktop" "$out/release/hestia-tray" "$stage/"
  fi
  for bin in hestia hestiad; do
    cp "$out/release/$bin$ext" "$stage/bin/"
  done
  cp LICENSE README.md "$stage/"
  if [ "$os" = windows ]; then
    powershell -NoProfile -Command \
      "Compress-Archive -Path 'target/package/$name/*' -DestinationPath 'target/package/$name.zip' -Force"
    log "wrote target/package/$name.zip"
  else
    tar -C target/package -czf "target/package/$name.tar.gz" "$name"
    log "wrote target/package/$name.tar.gz"
  fi
}

case "$action" in
  all) bundle && portable ;;
  bundle) bundle ;;
  portable) portable ;;
  *) die "usage: $0 [all|bundle|portable]" ;;
esac
