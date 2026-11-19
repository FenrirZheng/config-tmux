cmake_minimum_required(VERSION 3.20)
project(win-pick VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
  set(CMAKE_BUILD_TYPE Release CACHE STRING "" FORCE)
endif()

find_program(CCACHE_PROGRAM ccache)
if(CCACHE_PROGRAM AND NOT CMAKE_CXX_COMPILER_LAUNCHER)
  set(CMAKE_CXX_COMPILER_LAUNCHER "${CCACHE_PROGRAM}")
endif()

find_package(ftxui CONFIG REQUIRED)

set(_out "${CMAKE_CURRENT_SOURCE_DIR}/../target/release")

add_library(win-pick-core STATIC
  src/model.cpp
  src/tmux_io.cpp
)
target_include_directories(win-pick-core PUBLIC
  "${CMAKE_CURRENT_SOURCE_DIR}/src"
)
target_compile_options(win-pick-core PRIVATE
  -Wall -Wextra -Wpedantic
  -Wshadow -Wconversion -Wsign-conversion
  -Wold-style-cast -Wnon-virtual-dtor
)

add_executable(win-pick
  src/main.cpp
  src/ui.cpp
)
target_link_libraries(win-pick PRIVATE
  win-pick-core
  ftxui::screen
  ftxui::dom
  ftxui::component
)
target_compile_options(win-pick PRIVATE
  -Wall -Wextra -Wpedantic
  -Wshadow -Wconversion -Wsign-conversion
  -Wold-style-cast -Wnon-virtual-dtor
)
set_target_properties(win-pick PROPERTIES
  RUNTIME_OUTPUT_DIRECTORY          "${_out}"
  RUNTIME_OUTPUT_DIRECTORY_RELEASE  "${_out}"
  RUNTIME_OUTPUT_DIRECTORY_DEBUG    "${_out}"
)

enable_testing()

add_executable(test_win_pick
  tests/test_win_pick.cpp
)
target_link_libraries(test_win_pick PRIVATE
  win-pick-core
)
target_compile_options(test_win_pick PRIVATE
  -Wall -Wextra -Wpedantic
  -Wshadow -Wconversion -Wsign-conversion
  -Wold-style-cast -Wnon-virtual-dtor
)
add_test(NAME win_pick_unit COMMAND test_win_pick)
