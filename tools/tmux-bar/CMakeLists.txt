# tmux-bar — unified status bar, Mocha theme, and Claude glyph installer for tmux.
#
# Replaces catppuccin/tmux, try-palette.sh, claude.conf's bar-format & style
# overrides, and `cc-beacon install-bar` with one zero-dependency C++20 binary.
#
# Build (from ~/.tmux/tools):
#   cmake -S tmux-bar -B target/cmake-build-tmux-bar -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_COMPILER=clang++
#   cmake --build target/cmake-build-tmux-bar
#   ctest --test-dir target/cmake-build-tmux-bar --output-on-failure
cmake_minimum_required(VERSION 3.16)
project(tmux-bar CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
  set(CMAKE_BUILD_TYPE Release CACHE STRING "" FORCE)
endif()

set(_strict_warnings
  -Wall -Wextra -Wpedantic
  -Wshadow -Wconversion -Wsign-conversion -Wold-style-cast -Wnon-virtual-dtor)

add_library(tmux-bar-lib STATIC src/bar_spec.cpp)
target_include_directories(tmux-bar-lib PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}/src")
target_compile_options(tmux-bar-lib PRIVATE ${_strict_warnings})

add_executable(tmux-bar src/main.cpp)
target_link_libraries(tmux-bar PRIVATE tmux-bar-lib)
target_compile_options(tmux-bar PRIVATE ${_strict_warnings})

set(_out "${CMAKE_CURRENT_SOURCE_DIR}/../target/release")
set_target_properties(tmux-bar PROPERTIES
  RUNTIME_OUTPUT_DIRECTORY         "${_out}"
  RUNTIME_OUTPUT_DIRECTORY_RELEASE "${_out}"
  RUNTIME_OUTPUT_DIRECTORY_DEBUG   "${_out}")

enable_testing()
add_executable(tmux-bar-test tests/test_bar_spec.cpp)
target_link_libraries(tmux-bar-test PRIVATE tmux-bar-lib)
target_compile_options(tmux-bar-test PRIVATE ${_strict_warnings})
add_test(NAME bar_spec_unit_tests COMMAND tmux-bar-test)
