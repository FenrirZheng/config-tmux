# sift — popup regex search for `prefix /`. See runbooks/sift.md and
# docs/adr/0005-own-the-interaction-loop-for-regex-search.org.
#
# Build (from ~/.tmux/tools):
#   cmake -S sift -B target/cmake-build -DCMAKE_BUILD_TYPE=Release
#   cmake --build target/cmake-build -j
#
# The binary lands in tools/target/release/ alongside the cargo output so
# claude.conf keeps ONE path shape for every tool. The build tree lives under
# tools/target/ too, which tools/.gitignore already covers — no new ignore
# rule. Cost, documented in the runbook: `cargo clean` deletes sift as well.
cmake_minimum_required(VERSION 3.16)
project(sift CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
  set(CMAKE_BUILD_TYPE Release CACHE STRING "" FORCE)
endif()

add_executable(sift src/main.cpp)
target_compile_options(sift PRIVATE -Wall -Wextra -Wpedantic)

# No external dependencies by design: raw termios + ANSI + POSIX <regex.h> +
# libc wcwidth. Adding ncurses would put a dev package in the fresh-machine
# bootstrap for a UI this small.
set(_out "${CMAKE_CURRENT_SOURCE_DIR}/../target/release")
set_target_properties(sift PROPERTIES
  RUNTIME_OUTPUT_DIRECTORY          "${_out}"
  RUNTIME_OUTPUT_DIRECTORY_RELEASE  "${_out}"
  RUNTIME_OUTPUT_DIRECTORY_DEBUG    "${_out}")
