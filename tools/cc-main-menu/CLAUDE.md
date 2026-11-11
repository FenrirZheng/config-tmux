# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`cc-main-menu` is the tmux command palette on `prefix m`: a C++20 + FTXUI popup that lists
every tmux command (`tmux list-commands -F`) and every key bound with `-N "note"`
(`tmux list-keys -N -T <table>`), shows each command's bound keys as badges, fuzzy-filters
as you type, and runs the chosen row against the pane that opened the menu. It lives in
the `~/.tmux/tools/` workspace next to the Rust crates and the other C++ tools (`sift`,
`pane-jump`), but builds with **Meson + Ninja**, not cargo or cmake (user directive,
2026-11-11). The atlas node is `../atlas/cc-main-menu.org`; read it first.

## Build and test

```bash
scripts/build.sh              # release + LTO → ../target/release/cc-main-menu
scripts/build.sh --asan       # ASan+UBSan    → ../target/release/cc-main-menu-asan
scripts/build.sh --clean      # wipe the build tree first (combine with --asan)
bash scripts/verify.sh        # 47 assertions on throwaway tmux servers, ~4 s
CC_MAIN_MENU=../target/release/cc-main-menu-asan CCMM_SAN_LOG=<empty dir> bash scripts/verify.sh
bash ../build-all.sh          # every tool in the workspace (recovery after cargo clean)
```

- Build trees: `../target/meson-cc-main-menu[-asan]/`. `cargo clean` deletes them and the
  binaries; `build.sh` copies the binary into `target/release/` atomically, so build
  through it before testing the popup. Incremental compile only:
  `meson compile -C ../target/meson-cc-main-menu`.
- FTXUI v5.0.0 is pinned by `subprojects/ftxui.wrap` and cloned into `subprojects/ftxui/`
  (ignored) on first configure — needs network. It enters through `import('cmake')`;
  its dependency names are the CMake targets `component`, `dom`, `screen`, marked
  `as_system()` so warnings judge only our code.
- Warnings: `-Wall -Wextra -Wpedantic -Wshadow -Wconversion -Wsign-conversion
  -Wold-style-cast -Wnon-virtual-dtor` (the sift / pane-jump set). The build is at 0
  warnings; keep it there.
- Linker: `meson.options` `linker=auto` picks mold, then lld, then the default.
- clangd reads `./compile_commands.json`, an ignored symlink into the release build
  tree (`ln -sf ../target/meson-cc-main-menu/compile_commands.json .`).
- `verify.sh` must never touch the live server: it uses `-L ccmm_verify` (server under
  test) attached from a pane of `-L ccmm_outer`, because prefix keys only reach a key
  table through a client's terminal. Headless calls set `$TMUX` to the throwaway socket.
- The user's Bash rule gives test commands a 10 s timeout; `verify.sh` fits (~3 s). Wait
  for state by polling (`wait_eq`, `wait_screen`), not fixed sleeps.

## Architecture

| file | role |
|---|---|
| `src/tmux_client.*` | the only code that runs tmux (`run_tmux`: posix_spawnp, argv, no shell); pure parsers for list-commands / list-keys / list-keys -N; the tmux-syntax `tokenize`; `build_catalog` (noted keys first, then commands A–Z, badges joined); `resolve_*` → argv |
| `src/fuzzy_filter.*` | multi-term scoring (exact > prefix > word-boundary substring > substring > subsequence), weighted per field |
| `src/ui.*` | FTXUI component tree and key handling; returns argv, never runs it |
| `src/main.cpp` | argv parsing; TUI mode, `rows [QUERY]` and `resolve LINE` headless seams |
| `scripts/launch.sh` | popup → read NUL-delimited argv → `tmux "${argv[@]}"` → error to `display-message -l` |
| `cc-main-menu.tmux` | entry script `claude.conf` runs; installs the binding or the not-built stub |

Load-bearing decisions (each measured on tmux 3.5a; details in
`docs/parser-design.org`):

- **The popup closes before the command runs.** The binary writes the argv to
  `--out FILE` and exits 0; `display-popup` blocks until the popup closes, then
  `launch.sh` runs it. Running inside the popup breaks `copy-mode`, `choose-tree`,
  `command-prompt` and nested popups. Never make the binary execute the choice.
- **`display-popup` does not expand `#{…}`**, so the key binds `run-shell`, which does,
  and passes `--client --session --window --pane` as plain argv to `launch.sh`.
- **Command rows** get `-t` / `-c` derived from the usage string, inserted right after
  the command name (tmux stops option parsing at the first positional). `new-window` gets
  `-t $session:` because `-t @window` fails with "index in use".
- **Noted-key rows** are replayed with `send-keys -K -c CLIENT [prefix] KEY`, so the
  binding's own formats expand as if pressed. A `;` key is sent as `\;`.
- **Ctrl-C**: FTXUI 5 leaves ISIG on, so Ctrl-C would SIGINT the whole process group.
  `ui.cpp` clears ISIG on the first render — not via `screen.Post()`, which FTXUI drops
  before `Loop()` starts — so Ctrl-C is a key that exits 0 (`verify.sh` group 6 checks the
  exit status through a wrapper).
- **`{ … }` in the argument bar** (e.g. Tab on stock `prefix $`) is folded into one
  argument holding the inner commands as a string: as separate argv words tmux rejects
  them ("too many arguments").
- **Untrusted text**: notes and bound commands are user text. They are rendered only in
  the popup (control bytes replaced by `printable()`) and reach tmux only as argv or
  `display-message -l`; never put them in a format context (`../ARCHITECTURE.org`).

## UI

Catppuccin Mocha, matching `~/.tmux`: text `#cdd6f4`, border/accent/badges `#89b4fa`,
selection `#cba6f7` (mauve) on a `#313244` band, subtext `#a6adc8`, errors `#f38ba8`.
Background is `Color::Default` so `claude.conf`'s `popup-style bg=#181825` shows through.

Keys: type to filter · `↑↓` / `C-n` `C-p` / wheel / `PgUp` `PgDn` move · `Enter` run ·
`Tab` opens the argument bar prefilled with the command (or a key's bound command) —
`Enter` there runs the edited line, `Esc` returns · `Esc` / `C-g` / `C-c` quit, run nothing.

## tmux integration

- `claude.conf` sets `@cc-main-menu-key m` and runs `cc-main-menu.tmux`. Options:
  `@cc-main-menu-{key,width,height}` (defaults `m`, `80%`, `75%`) and
  `@cc-main-menu-autobuild` (default `off`; `on` builds in the background when the binary
  is missing). Never expose the library name in option names.
- **`prefix m` replaced stock `select-pane -m`**, the Claude-inbox mark. The user accepted
  that (2026-11-11); marking moved to `prefix M`, rebound to `select-pane -m`, which
  toggles. Don't move the default off `m`.
- The custom keys in `claude.conf` carry `-N` notes so they appear as menu rows. A new
  binding there should get one too.
- Not-built guard is load-time, like sift's: after building, `tmux source-file ~/.tmux.conf`.
