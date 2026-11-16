#!/usr/bin/env bash
# Sourced as the first line of every scripts/<verb>.sh: strict mode, the repo
# root as the working directory, and the one voice status output speaks in.
#
# The cd anchors on *this* file rather than the caller's, so a verb behaves the
# same whether it was run as scripts/build.sh, ./build.sh or an absolute path —
# every path in a script here is repo-relative and stays that way.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

if [ -t 1 ]; then _C='\033[1;36m'; _R='\033[0m'; else _C=''; _R=''; fi

log() { printf '%b==>%b %s\n' "$_C" "$_R" "$*"; }

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

# Refuse early, naming what to install, rather than dying mid-pipeline on a
# `jq: command not found`.
#
# The tool is *run*, not merely located: Windows ships an App Execution Alias
# for `python` that sits on PATH, satisfies `command -v`, and then prints an
# advert for the Microsoft Store instead of running anything. Several version
# flags are tried because there is no one spelling every tool answers.
require() {
  local tool="$1" hint="${2:-}" flag
  if command -v "$tool" > /dev/null 2>&1; then
    for flag in --version -v -V; do
      "$tool" "$flag" > /dev/null 2>&1 && return 0
    done
  fi
  die "$tool is required${hint:+ — $hint}"
}
