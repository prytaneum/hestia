#!/usr/bin/env bash
# Fetch the official plugins pinned in plugins.lock and lay them into each
# destination directory, ready for the daemon to seed on its first start.
#
# Each line of the lock is `<id> <version> <sha256>`; the package is downloaded
# from the official repository's `repo` branch and refused unless its sha256
# matches the lock. Its `.minisig` rides beside it untouched: the daemon
# verifies it against the official key when it installs the package, so a
# package that fails that check is never installed. An empty lock leaves each
# destination empty.
#
#   scripts/plugins.sh <dest>...      # e.g. crates/desktop/bundled-plugins target/debug/plugins
#
# HESTIA_PLUGINS_BASE replaces the repository URL (a file:// tree works), for
# trying a lock against a mirror or a local publish.
. "$(dirname "$0")/lib/common.sh"

[ "$#" -ge 1 ] || die "usage: $0 <dest>..."

lock="plugins.lock"
base="${HESTIA_PLUGINS_BASE:-https://raw.githubusercontent.com/prytaneum/plugins/repo}"
cache="target/plugins-cache"

sha256_of() {
  if command -v sha256sum > /dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

fetch() {
  local url="$1" out="$2"
  curl --fail --silent --show-error --location --proto '=https,file' --proto-redir '=https' --retry 3 \
    --output "$out.part" "$url" || die "cannot download $url"
  mv "$out.part" "$out"
}

[ -f "$lock" ] || die "$lock is missing"
mkdir -p "$cache"

staged=()
while read -r id version sha _; do
  case "$id" in '' | '#'*) continue ;; esac
  [[ "$id" =~ ^[a-z0-9][a-z0-9-]*$ ]] || die "$lock: bad plugin id '$id'"
  [[ "$version" =~ ^[0-9][0-9A-Za-z.+-]*$ ]] || die "$lock: bad version '$version' for $id"
  [[ "$sha" =~ ^[0-9a-f]{64}$ ]] || die "$lock: bad sha256 for $id $version"

  file="$id-$version.hplugin"
  if [ ! -f "$cache/$file" ] || [ "$(sha256_of "$cache/$file")" != "$sha" ]; then
    log "fetching $file"
    fetch "$base/packages/$file" "$cache/$file"
  fi
  [ "$(sha256_of "$cache/$file")" = "$sha" ] || {
    rm -f "$cache/$file"
    die "$file does not match the sha256 pinned in $lock"
  }
  fetch "$base/packages/$file.minisig" "$cache/$file.minisig"
  staged+=("$file")
done < "$lock"

for dest in "$@"; do
  rm -rf "$dest"
  mkdir -p "$dest"
  for file in ${staged[@]+"${staged[@]}"}; do
    cp "$cache/$file" "$cache/$file.minisig" "$dest/"
  done
  echo "  staged ${#staged[@]} plugin(s) in $dest"
done
