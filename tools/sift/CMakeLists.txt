# sift — popup regex search for `prefix /`. See runbooks/sift.md and
# docs/adr/0005-own-the-interaction-loop-for-regex-search.org.
#
# Build (from ~/.tmux/tools/sift) — the presets in CMakePresets.json pin the
# Ninja generator and the build directory, so there is one spelling to remember:
#   cmake --preset release && cmake --build --preset release
#
# Ninja does NOT make this compile faster: sift is one translation unit, so
# there is nothing to schedule in parallel (measured 2446 ms vs make's 2434 ms).
# What it buys is a no-op rebuild in 16 ms instead of 56 ms, which is the case
# that actually recurs while editing. Presets need cmake >= 3.21 and ninja on
# PATH; without either, the generator-less form still works from ~/.tmux/tools:
#   cmake -S sift -B target/cmake-build -DCMAKE_BUILD_TYPE=Release
#   cmake --build target/cmake-build -j
#
# The binary lands in tools/target/release/ alongside the cargo output so
# claude.conf keeps ONE path shape for every tool. The build tree lives under
# tools/target/ too, which tools/.gitignore already covers — no new ignore
# rule. Cost, documented in the runbook: `cargo clean` deletes sift as well.
#
# Sanitizer build (ASan + UBSan + LSan) — a SEPARATE build tree, and it emits
# `sift-asan`, never `sift`, so an instrumented binary can never end up as the
# one `prefix /` runs:
#   cmake --preset asan && cmake --build --preset asan
#   SIFT=~/.tmux/tools/target/release/sift-asan \
#     bash ~/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-jump.sh
cmake_minimum_required(VERSION 3.16)
project(sift CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
  set(CMAKE_BUILD_TYPE Release CACHE STRING "" FORCE)
endif()

option(SIFT_SANITIZE "Build with AddressSanitizer + UndefinedBehaviorSanitizer" OFF)

if(SIFT_SANITIZE)
  add_executable(sift src/main.cpp)
  set_target_properties(sift PROPERTIES OUTPUT_NAME sift-asan)
  target_compile_options(sift PRIVATE
    -fsanitize=address,undefined -fno-omit-frame-pointer -fno-sanitize-recover=all -g)
  target_link_options(sift PRIVATE -fsanitize=address,undefined)
else()
  add_executable(sift src/main.cpp)
endif()

# The bar the source is held to, measured clean at each step — not aspirational.
# -Wconversion and -Wsign-conversion are the ones that matter here: this file
# converts between byte offsets, character counts and cell columns constantly,
# and every one of those casts is meant to be deliberate and visible.
target_compile_options(sift PRIVATE
  -Wall -Wextra -Wpedantic
  -Wshadow -Wconversion -Wsign-conversion -Wold-style-cast -Wnon-virtual-dtor)

# No external dependencies by design: raw termios + ANSI + POSIX <regex.h> +
# libc wcwidth. Adding ncurses would put a dev package in the fresh-machine
# bootstrap for a UI this small.
set(_out "${CMAKE_CURRENT_SOURCE_DIR}/../target/release")
set_target_properties(sift PROPERTIES
  RUNTIME_OUTPUT_DIRECTORY          "${_out}"
  RUNTIME_OUTPUT_DIRECTORY_RELEASE  "${_out}"
  RUNTIME_OUTPUT_DIRECTORY_DEBUG    "${_out}")
