#!/usr/bin/env bash
# build-all.sh — build every tool claude.conf binds, into tools/target/release/.
#
# One command for a fresh machine, and for recovery after `cargo clean` (which
# deletes target/ — the cmake and meson binaries included, not just cargo's).
# Four build systems' worth of output share target/release/ so claude.conf keeps
# one path shape:
#   cargo  the Rust workspace (every crate in Cargo.toml)
#   cmake  sift, pane-jump          (C++; generator-less form, no ninja needed)
#   meson  cc-main-menu             (C++ + FTXUI; needs meson, ninja, network on
#                                    the first configure for the FTXUI wrap)
#
#   bash build-all.sh           build all, stop at the first failure
#   bash build-all.sh --keep-going   build all, report every failure at the end
#
# Afterwards: tmux source-file ~/.tmux.conf — the not-built guards are load-time.
set -uo pipefail

tools=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd -- "$tools" || exit 1

keep_going=0
[[ ${1:-} == --keep-going ]] && keep_going=1

failed=()
step() { # step NAME CMD...
  printf '\n── %s ──\n' "$1"
  if "${@:2}"; then
    return 0
  fi
  failed+=("$1")
  (( keep_going )) || { printf '\nbuild-all: %s FAILED\n' "$1" >&2; exit 1; }
}

cmake_tool() { # cmake_tool DIR BUILDDIR
  cmake -S "$1" -B "$2" -DCMAKE_BUILD_TYPE=Release && cmake --build "$2"
}

step cargo        cargo build --release
step sift         cmake_tool sift target/cmake-build
step pane-jump    cmake_tool pane-jump target/cmake-build-pane-jump
step cc-main-menu bash cc-main-menu/scripts/build.sh

if (( ${#failed[@]} )); then
  printf '\nbuild-all: FAILED: %s\n' "${failed[*]}" >&2
  exit 1
fi
printf '\nbuild-all: ok — now: tmux source-file ~/.tmux.conf\n'
