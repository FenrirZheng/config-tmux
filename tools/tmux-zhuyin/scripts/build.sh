#!/usr/bin/env bash
# build.sh — build tmux-zhuyin into ~/.tmux/tools/target/release/tmux-zhuyin.
#
# Dependency strategy:
#   1. Prefer Debian system packages (libftxui-dev, libchewing3-dev).
#   2. If libftxui-dev is missing, automatically fall back to Conan 2 (conanfile.txt).
# Build acceleration: Ninja + ccache + PCH.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
build_dir="$here/../target/cmake-build-tmux-zhuyin"
conan_dir="$here/../target/conan-tmux-zhuyin"

if ! pkg-config --exists chewing; then
  echo "tmux-zhuyin: missing libchewing3-dev (pkg-config 'chewing' not found)." >&2
  echo "Install on Debian: sudo apt install -y libchewing3-dev libchewing3-data" >&2
  exit 1
fi

cmake_args=(
  -G Ninja
  -S "$here"
  -B "$build_dir"
  -DCMAKE_BUILD_TYPE=Release
)

has_system_ftxui=0
if [[ -f /usr/lib/x86_64-linux-gnu/cmake/ftxui/ftxui-config.cmake ]] \
   || [[ -f /usr/lib/cmake/ftxui/ftxui-config.cmake ]] \
   || [[ -f /usr/local/lib/cmake/ftxui/ftxui-config.cmake ]]; then
  has_system_ftxui=1
fi

if (( ! has_system_ftxui )); then
  if command -v conan >/dev/null 2>&1; then
    echo "tmux-zhuyin: system libftxui-dev not found; using Conan fallback..."
    conan install "$here/conanfile.txt" \
      --output-folder="$conan_dir" \
      --build=missing \
      -s build_type=Release
    toolchain="$conan_dir/conan_toolchain.cmake"
    [[ -f $toolchain ]] || toolchain="$conan_dir/build/Release/generators/conan_toolchain.cmake"
    cmake_args+=("-DCMAKE_TOOLCHAIN_FILE=$toolchain")
  else
    echo "tmux-zhuyin: missing libftxui-dev and conan is not installed." >&2
    echo "Install on Debian: sudo apt install -y libftxui-dev" >&2
    exit 1
  fi
fi

cmake "${cmake_args[@]}"
cmake --build "$build_dir"
