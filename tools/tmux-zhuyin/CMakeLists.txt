cmake_minimum_required(VERSION 3.22)
project(tmux-zhuyin VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# Build acceleration: auto-enable ccache when available.
find_program(CCACHE_PROGRAM ccache)
if(CCACHE_PROGRAM AND NOT CMAKE_CXX_COMPILER_LAUNCHER)
  message(STATUS "Enabling ccache: ${CCACHE_PROGRAM}")
  set(CMAKE_CXX_COMPILER_LAUNCHER "${CCACHE_PROGRAM}")
endif()

# Default output path matches ~/.tmux/tools/target/release/ convention.
if(NOT DEFINED TMUX_ZHUYIN_OUTPUT_DIR)
  set(TMUX_ZHUYIN_OUTPUT_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../target/release")
endif()

option(TMUX_ZHUYIN_BUILD_TESTS "Build GoogleTest unit tests" ON)
option(TMUX_ZHUYIN_ENABLE_PCH "Enable precompiled headers for faster builds" ON)

# Dependencies: Debian system packages first, Conan CMakeDeps toolchain compatible.
find_package(ftxui CONFIG REQUIRED)
find_package(PkgConfig REQUIRED)
pkg_check_modules(CHEWING REQUIRED IMPORTED_TARGET chewing)

# Core library (headless-testable, no FTXUI dependency).
add_library(tmux-zhuyin-core STATIC
  src/chewing_engine.cpp
  src/ime_session.cpp
)
target_include_directories(tmux-zhuyin-core PUBLIC
  "${CMAKE_CURRENT_SOURCE_DIR}/src"
)
target_link_libraries(tmux-zhuyin-core PUBLIC
  PkgConfig::CHEWING
)
target_compile_options(tmux-zhuyin-core PRIVATE
  -Wall -Wextra -Wpedantic
)

# Main FTXUI popup binary.
add_executable(tmux-zhuyin
  src/main.cpp
  src/ui.cpp
)
target_link_libraries(tmux-zhuyin PRIVATE
  tmux-zhuyin-core
  ftxui::screen
  ftxui::dom
  ftxui::component
)
target_compile_options(tmux-zhuyin PRIVATE
  -Wall -Wextra -Wpedantic
)
set_target_properties(tmux-zhuyin PROPERTIES
  RUNTIME_OUTPUT_DIRECTORY "${TMUX_ZHUYIN_OUTPUT_DIR}"
)

# Precompiled headers (PCH) for build speed.
if(TMUX_ZHUYIN_ENABLE_PCH)
  target_precompile_headers(tmux-zhuyin-core PRIVATE
    src/pch.hpp
  )
  target_precompile_headers(tmux-zhuyin PRIVATE
    src/pch.hpp
    <ftxui/component/component.hpp>
    <ftxui/component/event.hpp>
    <ftxui/component/screen_interactive.hpp>
    <ftxui/dom/elements.hpp>
    <ftxui/screen/color.hpp>
  )
endif()

# Unit tests (GoogleTest via Debian libgtest-dev or Conan).
if(TMUX_ZHUYIN_BUILD_TESTS)
  find_package(GTest QUIET)
  if(GTest_FOUND)
    enable_testing()
    add_executable(tmux-zhuyin-tests
      tests/test_ime.cpp
    )
    target_link_libraries(tmux-zhuyin-tests PRIVATE
      tmux-zhuyin-core
      GTest::gtest_main
    )
    target_compile_options(tmux-zhuyin-tests PRIVATE
      -Wall -Wextra -Wpedantic
    )
    if(TMUX_ZHUYIN_ENABLE_PCH)
      target_precompile_headers(tmux-zhuyin-tests REUSE_FROM tmux-zhuyin-core)
    endif()
    include(GoogleTest)
    gtest_discover_tests(tmux-zhuyin-tests)
  else()
    message(STATUS "GTest not found; skipping unit tests (install libgtest-dev to enable)")
  endif()
endif()
