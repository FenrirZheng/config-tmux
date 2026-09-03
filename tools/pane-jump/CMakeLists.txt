# pane-jump — popup pane picker for `prefix g`. See runbooks/switch-pane-by-name.md
# and docs/adr/0008-own-the-interaction-loop-for-pane-selection.org.
#
# Build (from ~/.tmux/tools/pane-jump) — the presets in CMakePresets.json pin the
# Ninja generator and the build directory, so there is one spelling to remember:
#   cmake --preset release && cmake --build --preset release
#
# Presets need cmake >= 3.21 and ninja on PATH; without either, the
# generator-less form still works from ~/.tmux/tools and is the spelling the
# runbook and claude.conf's not-built stub quote:
#   cmake -S pane-jump -B target/cmake-build-pane-jump -DCMAKE_BUILD_TYPE=Release
#   cmake --build target/cmake-build-pane-jump
#
# The binary lands in tools/target/release/ alongside the cargo output and
# alongside sift, so claude.conf keeps ONE path shape for every tool. The build
# tree is its OWN directory under tools/target/ — sift's cmake cache lives in
# target/cmake-build, this one in target/cmake-build-pane-jump, so the two
# projects never share a cache. tools/.gitignore already covers target/ — no new
# ignore rule. Cost, inherited from sift: `cargo clean` deletes this binary too.
#
# Sanitizer build (ASan + UBSan + LSan) — a SEPARATE build tree, and it emits
# `pane-jump-asan`, never `pane-jump`, so an instrumented binary can never end up
# as the one `prefix g` runs:
#   cmake --preset asan && cmake --build --preset asan
#   PANE_JUMP=~/.tmux/tools/target/release/pane-jump-asan \
#     bash ~/.tmux/tools/pane-jump/verify-pane-jump.sh
cmake_minimum_required(VERSION 3.16)
project(pane-jump CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
  set(CMAKE_BUILD_TYPE Release CACHE STRING "" FORCE)
endif()

option(PANE_JUMP_SANITIZE "Build with AddressSanitizer + UndefinedBehaviorSanitizer" OFF)

if(PANE_JUMP_SANITIZE)
  add_executable(pane-jump src/main.cpp)
  set_target_properties(pane-jump PROPERTIES OUTPUT_NAME pane-jump-asan)
  target_compile_options(pane-jump PRIVATE
    -fsanitize=address,undefined -fno-omit-frame-pointer -fno-sanitize-recover=all -g)
  target_link_options(pane-jump PRIVATE -fsanitize=address,undefined)
else()
  add_executable(pane-jump src/main.cpp)
endif()

# The bar the source is held to, measured clean at each step — not aspirational.
# Identical to sift's set, because the terminal layer is copied from sift and a
# looser set here would let a copied line rot on one side only.
# -Wconversion and -Wsign-conversion are the ones that matter: this file converts
# between byte offsets, character counts and cell columns constantly, and every
# one of those casts is meant to be deliberate and visible.
target_compile_options(pane-jump PRIVATE
  -Wall -Wextra -Wpedantic
  -Wshadow -Wconversion -Wsign-conversion -Wold-style-cast -Wnon-virtual-dtor)

# No external dependencies by design: raw termios + ANSI + libc wcwidth/towlower.
# Adding ncurses would put a dev package in the fresh-machine bootstrap for a UI
# this small.
set(_out "${CMAKE_CURRENT_SOURCE_DIR}/../target/release")
set_target_properties(pane-jump PROPERTIES
  RUNTIME_OUTPUT_DIRECTORY          "${_out}"
  RUNTIME_OUTPUT_DIRECTORY_RELEASE  "${_out}"
  RUNTIME_OUTPUT_DIRECTORY_DEBUG    "${_out}")
