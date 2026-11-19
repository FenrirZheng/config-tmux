# Agent.md

This file provides guidance to AI coding agents when working with code in this repository.

## What this is

`tmux-zhuyin` is the pure-terminal Traditional Chinese (Zhuyin/Bopomofo) IME popup on
`C-\` (`root` table, no prefix needed): a C++20 + FTXUI 5.0 popup backed by libchewing
0.9.1 (`pkg-config: chewing`), requiring no X11/Wayland, IBus/Fcitx5, or D-Bus. Committed
UTF-8 text is written to `--out` and pasted unsubmitted into the origin pane via
`paste-buffer -p -d`. It lives in `~/.tmux/tools/` alongside the Rust crates,
`sift`/`pane-jump` (CMake), and `cc-main-menu` (Meson + FTXUI), and builds with
**CMake (>= 3.22) + Ninja + ccache + PCH** (Debian-first dependencies with Conan 2
fallback). Workspace conventions are documented in `../ARCHITECTURE.org`.

## Build and test

```bash
scripts/build.sh                                                        # Release (Debian-first / Conan fallback) → ../target/release/tmux-zhuyin
cmake --preset release && cmake --build --preset release                # Ninja + ccache + PCH → ../target/release/tmux-zhuyin
cmake --preset debug && cmake --build --preset debug                    # Debug → ../target/debug/tmux-zhuyin
cmake --preset asan && cmake --build --preset asan                      # ASan+UBSan → ../target/asan/tmux-zhuyin
ctest --test-dir ../target/cmake-build-tmux-zhuyin --output-on-failure  # GoogleTest unit suite (tmux-zhuyin-tests)
bash scripts/verify.sh                                                  # CTest + headless --eval + isolated tmux e2e
bash ../build-all.sh                                                    # every tool in workspace (recovery after cargo clean)
```

- Build trees: `../target/cmake-build-tmux-zhuyin[-debug|-asan]/` and
  `../target/conan-tmux-zhuyin/`. `cargo clean` in `../` deletes `target/` including
  CMake build trees and binaries — recover via `scripts/build.sh` or
  `bash ../build-all.sh`. `scripts/build.sh` takes no CLI flags (always Release); use
  `CMakePresets.json` (`release`, `debug`, `asan`) for Debug or ASan/UBSan builds.
- Dependencies: Debian-first (`libftxui-dev` 5.0 for `ftxui::screen`, `ftxui::dom`,
  `ftxui::component`; `libchewing3-dev` 0.9.1 via `pkg-config: chewing`;
  `libchewing3-data` runtime dictionary under `/usr/share/libchewing/`; optional
  `libgtest-dev`). `scripts/build.sh` requires `chewing` via `pkg-config` and checks
  system `ftxui-config.cmake` paths before falling back to Conan 2 (`conanfile.txt`
  pins `ftxui/5.0.0` via `CMakeDeps` and `CMakeToolchain`).
- Compiler & acceleration: `CMakeLists.txt` enables C++20 (`CMAKE_CXX_EXTENSIONS OFF`),
  `CMAKE_EXPORT_COMPILE_COMMANDS ON` (`compile_commands.json` in the build tree),
  auto-`ccache` (`CMAKE_CXX_COMPILER_LAUNCHER`), and PCH (`TMUX_ZHUYIN_ENABLE_PCH=ON`,
  `src/pch.hpp` plus five `<ftxui/...>` headers for `tmux-zhuyin`). All targets compile
  with `-Wall -Wextra -Wpedantic` — keep the build at 0 warnings.
- Tests & isolation: `TMUX_ZHUYIN_BUILD_TESTS=ON` builds `tmux-zhuyin-tests`
  (`IsolatedImeTest`) when `GTest` is found, and skips cleanly if `libgtest-dev` is
  absent. `verify.sh` must never touch the live tmux server or user `~/.chewing/` state:
  headless `--eval` sets `HOME="$tmp_home"`, and tmux checks run on an isolated
  `-L "tmux-zhuyin-verify-$$" -f /dev/null` server.

## Architecture

| file | role |
|---|---|
| `src/pch.hpp` | precompiled STL headers shared by `tmux-zhuyin-core` and `tmux-zhuyin-tests` (`tmux-zhuyin` adds `<ftxui/...>`) |
| `src/chewing_engine.*` | `namespace zhuyin` RAII `ChewingEngine` (`chewing_new`/`chewing_new2`), `KeyboardLayout`, `parse_keyboard_layout`, `keyboard_layout_label`, `EngineSnapshot` |
| `src/ime_session.*` | pure `ImeSession` state machine (`KeyKind`, `KeyEvent`, `SessionAction::{Continue,Submit,Cancel}`), two-stage `Enter`, `pop_last_utf8_char` |
| `src/ui.*` | FTXUI `UiOptions` and `run_popup_ui` renderer/key translator; returns committed UTF-8, never calls tmux |
| `src/main.cpp` | CLI (`--out`, `--pane`, `--kb`, `--eval`, `-h`/`--help`; exits `0`/`1`/`2`), `parse_eval_keys`, TUI or headless runner |
| `tests/test_ime.cpp` | `IsolatedImeTest` (7 GoogleTest cases against `tmux-zhuyin-core` with isolated `uhash.sqlite3`) |
| `scripts/launch.sh` | runs `display-popup -E -T ' 注音輸入 '`, then delivers non-empty `--out` via `load-buffer` + `paste-buffer -p -d` |
| `tmux-zhuyin.tmux` | entry script run by `~/.tmux/claude.conf`; registers keybinding with `-N` note or not-built stub |

Load-bearing decisions:

- **Core library has zero FTXUI dependency.** `CMakeLists.txt` splits `tmux-zhuyin-core`
  (`chewing_engine`, `ime_session`) into a static library linked by both `tmux-zhuyin`
  and `tmux-zhuyin-tests`, so unit tests and `--eval` exercise the full IME state machine
  without a terminal.
- **Two-hop delivery; popup closes before paste.** `tmux-zhuyin` writes raw UTF-8 to
  `--out FILE` (or stdout) and exits `0` (`1` on write/runtime error, `2` on unknown
  `--kb` or invalid CLI args). `display-popup -E` blocks until exit, then
  `scripts/launch.sh` (`TMUX_ZHUYIN_BIN`, `TMPDIR`) loads non-empty output into private
  buffer `zhuyin-$$` and runs `paste-buffer -p -d` into `--pane` — preserving the user's
  default buffer, using bracketed paste, and never pressing Enter.
- **`display-popup` does not expand `#{…}` formats.** `tmux-zhuyin.tmux` binds
  `run-shell -b`, which expands `#{client_name}` and `#{pane_id}` and forwards them with
  width, height, and layout as plain argv to `scripts/launch.sh`.
- **Two-stage `Enter` vs `SubmitNow` (`<C-s>`).** `Enter` commits active composition
  first and only submits when composition is empty; `SubmitNow` calls `flush_preedit()`
  (closing candidates, clearing incomplete bopomofo via `chewing_clean_bopomofo_buf`, and
  committing preedit via `chewing_commit_preedit_buf` — note that under `libchewing`
  0.9.1 active incomplete bopomofo causes `flush_preedit()` to drop preedit unless
  `Esc` clears bopomofo first). `Esc` closes open candidates or bopomofo before
  cancelling; `C-g` / `C-c` cancel immediately (leaving `--out` empty).
- **FTXUI `IXON`/`ISIG` trap on `C-s` and `C-\`.** `src/ui.cpp` wires `kCtrlS` (`\x13`)
  and `kCtrlBackslash` (`\x1c`) and shows `C-s 立即送出` in the footer, and both work in
  `--eval`. However, `ScreenInteractive::Fullscreen()` leaves tty `IXON`/`ISIG` enabled,
  so in a live `display-popup` `C-s` triggers `VSTOP` (XOFF freeze until `C-q`) and
  `C-\` sends `SIGQUIT` (exit `131`, besides colliding with `-T root C-\`); use `Enter`
  to submit and `Tab` (candidates closed) to toggle Zhuyin/EN until `ui.cpp` clears
  `IXON`/`ISIG` via `termios`.
- **`libchewing3-data` & `$HOME` isolation.** `libchewing` needs runtime dictionaries from
  `libchewing3-data` and stores user frequency data under `$HOME` when `user_data_path`
  is omitted. Unit tests pass a temp `uhash.sqlite3`; headless CLI scripts override `HOME`.

## UI

Catppuccin Mocha palette matching `~/.tmux/claude.conf` and `../cc-main-menu/`: text
`#cdd6f4`, subtext `#a6adc8`, overlay `#6c7086`, blue border/preedit `#89b4fa`, mauve mode
badge/EOL cursor `#cba6f7`, yellow bopomofo/fullshape `#f9e2af`, green aux `#a6e3a1`, surface
`#313244`, crust `#11111b`. Background stays `Color::Default` so `claude.conf`'s
`popup-style bg=#181825` shows through; the cursor node attaches `focus` so FTXUI hides
the terminal hardware cursor (mouse events are ignored).

Keys: ASCII keys compose Zhuyin/EN · `` ` `` punctuation menu · `↓` / `Space` candidates
(`1`–`9` pick, `←`/`→` `PgUp`/`PgDn` / `Space` page) · `Enter` confirm preedit then
submit · `Tab` (when closed) Zhuyin/EN · `F6` / `C-f` half/fullshape · `Backspace` edit
composition or pop UTF-8 char · `Esc` close candidates/bopomofo or cancel · `C-g` / `C-c`
cancel (`C-s` / `C-\` work in `--eval` and are wired in `src/ui.cpp`, but are intercepted
by tty `IXON`/`ISIG` in live `display-popup`).

## tmux integration

- `~/.tmux/claude.conf` runs `unbind -T prefix Z`, sets `@tmux-zhuyin-table root` and
  `@tmux-zhuyin-key 'C-\'`, and invokes `run-shell "~/.tmux/tools/tmux-zhuyin/tmux-zhuyin.tmux"`.
  `~/.tmux/tmux.conf` sources `~/.tmux/claude.conf` last (after TPM) so bindings and
  Catppuccin `popup-style` win.
- Options read before `run-shell`: `@tmux-zhuyin-table` (`root`), `@tmux-zhuyin-key`
  (`C-\`), `@tmux-zhuyin-width` (`68`), `@tmux-zhuyin-height` (`11`), and
  `@tmux-zhuyin-kb` (`default` — `default|hsu|et|et26|dvorak|ginyieh|dachen_cp26`).
- `tmux-zhuyin.tmux` binds with `-N 'Zhuyin IME: type Traditional Chinese into this pane (tmux-zhuyin)'`
  so `cc-main-menu` (`prefix m`) discovers it. `~/.tmux/cheat.txt` documents `C-\ (no prefix)`
  under `OTHER`, paired with `bind ? display-popup -w 85 -h 69 ...` in `~/.tmux/tmux.conf`.
- Load-time not-built guard: if `../target/release/tmux-zhuyin` is missing or
  non-executable when `tmux-zhuyin.tmux` runs, the key binds to `display-message -l` with
  the build command; after building (or `bash ../build-all.sh`), run
  `tmux source-file ~/.tmux.conf`.
