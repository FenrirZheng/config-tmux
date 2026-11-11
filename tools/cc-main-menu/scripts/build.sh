#!/usr/bin/env bash
# Build cc-main-menu.
#
#   scripts/build.sh            release + LTO → tools/target/release/cc-main-menu
#   scripts/build.sh --asan     ASan + UBSan  → tools/target/release/cc-main-menu-asan
#   scripts/build.sh --clean    wipe the chosen build tree first (combine with --asan)
#
# Build trees: tools/target/meson-cc-main-menu[-asan]/. The binaries land in the
# same target/release/ the cargo crates, sift and pane-jump write into, so
# claude.conf keeps one path shape for every tool. The sanitizer twin has a
# different NAME on purpose, so it can never become the binary prefix m runs.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
tools=$(cd -- "$here/.." && pwd)

asan=0 clean=0
for a in "$@"; do
  case $a in
    --asan) asan=1 ;;
    --clean) clean=1 ;;
    *) echo "build.sh: unknown option $a" >&2; exit 2 ;;
  esac
done

for tool in meson ninja cmake; do
  command -v "$tool" >/dev/null || { echo "build.sh: $tool not found" >&2; exit 1; }
done

if (( asan )); then
  builddir="$tools/target/meson-cc-main-menu-asan"
  out="$tools/target/release/cc-main-menu-asan"
  setup_args=(--buildtype=debug -Db_sanitize=address,undefined -Db_lundef=false)
else
  builddir="$tools/target/meson-cc-main-menu"
  out="$tools/target/release/cc-main-menu"
  setup_args=(--buildtype=release -Db_lto=true -Db_ndebug=if-release)
fi

(( clean )) && rm -rf -- "$builddir"

if [[ ! -f $builddir/build.ninja ]]; then
  meson setup "$builddir" "$here" "${setup_args[@]}"
fi
meson compile -C "$builddir"

mkdir -p -- "$(dirname -- "$out")"
# cp to a temp name + mv: a popup launched mid-build never execs a half-written file.
cp -- "$builddir/cc-main-menu" "$out.tmp"
mv -f -- "$out.tmp" "$out"
echo "built: $out"
