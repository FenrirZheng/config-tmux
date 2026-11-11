project(
  'cc-main-menu',
  'cpp',
  version: '0.1.0',
  meson_version: '>=1.1.0',
  default_options: [
    'cpp_std=c++20',
    'warning_level=2', # -Wall -Wextra
    'default_library=static',
    'buildtype=release',
    'b_ndebug=if-release',
  ],
)

cxx = meson.get_compiler('cpp')

# ── Linker: mold > lld > toolchain default ──────────────────────────────────
# Probed with has_link_argument, so a host without either falls back silently.
# -Dlinker=default skips the probe.
linker_choice = get_option('linker')
picked_linker = 'default'
if linker_choice != 'default'
  foreach cand : (linker_choice == 'auto' ? ['mold', 'lld'] : [linker_choice])
    if picked_linker == 'default' and cxx.has_link_argument('-fuse-ld=' + cand)
      add_project_link_arguments('-fuse-ld=' + cand, language: 'cpp')
      picked_linker = cand
    endif
  endforeach
endif
message('linker: ' + picked_linker)

# ── FTXUI (upstream is CMake) via the cmake module ──────────────────────────
cmake = import('cmake')
ftxui_opts = cmake.subproject_options()
ftxui_opts.add_cmake_defines({
  'FTXUI_BUILD_EXAMPLES': false,
  'FTXUI_BUILD_DOCS': false,
  'FTXUI_BUILD_TESTS': false,
  'FTXUI_ENABLE_INSTALL': false,
  'BUILD_SHARED_LIBS': false,
})
# Our -Wall -Wextra must not turn upstream's warnings into our problem.
ftxui_opts.set_override_option('warning_level', '0')
ftxui_opts.set_override_option('werror', 'false')
ftxui_proj = cmake.subproject('ftxui', options: ftxui_opts)

# as_system(): FTXUI's headers are included as -isystem, so the strict set
# below judges our code only.
ftxui_deps = [
  ftxui_proj.dependency('component').as_system(),
  ftxui_proj.dependency('dom').as_system(),
  ftxui_proj.dependency('screen').as_system(),
]

# The bar sift and pane-jump are held to (their CMakeLists.txt), on our targets
# only. -Wall -Wextra come from warning_level=2.
strict = cxx.get_supported_arguments(
  '-Wpedantic',
  '-Wshadow',
  '-Wconversion',
  '-Wsign-conversion',
  '-Wold-style-cast',
  '-Wnon-virtual-dtor',
)

sources = files(
  'src/fuzzy_filter.cpp',
  'src/main.cpp',
  'src/tmux_client.cpp',
  'src/ui.cpp',
)

executable(
  'cc-main-menu',
  sources,
  cpp_args: strict,
  dependencies: ftxui_deps + [dependency('threads')],
  install: false,
)
