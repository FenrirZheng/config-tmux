# Evidence — ~/.tmux (config-tmux repo)

Measurements and user decisions behind this repo's tools and config; cited from the surface each entry is about.

### 2026-11-11 cc-main-menu-tmux-measurements

**Verdict.** The palette's design rests on seven measured tmux / FTXUI / less behaviours and five user decisions, listed below; each is pinned by `tools/cc-main-menu/scripts/verify.sh` where a harness can see it.

**Purpose and question.** Before writing `tools/cc-main-menu` (prefix m command palette): how does tmux 3.5a behave for popups, key listing, key replay and command-line parsing, and which FTXUI 5 / less behaviours does the UI depend on?

**Environment.** tmux 3.5a, FTXUI v5.0.0 (meson wrap), g++ 14.2, less 668, Debian 13. All probes on throwaway servers (`tmux -L <name> -f /dev/null`, an outer server's pane attached as the client), never the live server.

**Target and scope.** `tools/cc-main-menu/` (src, scripts, entry script), `claude.conf` bindings, `tmux.conf` `prefix ?` popup, `cheat.txt`.

**Method and inputs / Evidence and results.**

| # | probe | result |
|---|---|---|
| 1 | `( time tmux display-popup -E -c $C "sleep 1.5" )` from a shell | returned after 1.51 s, rc 0 — the CLI call blocks until the popup closes; `launch.sh` runs the chosen command after it |
| 2 | `list-keys -N`, `-aN`, `-N -T prefix` after `bind -N 'test note' F11` | without `-T`: `C-b F11  test note` (prefix shown); with `-T prefix`: `F11  test note`; plain `list-keys` never shows the note. Stock server: only `prefix` has notes (85) |
| 3 | `send-keys -K -c $C C-b F9` (F9 bound to `set -g @hit yes`); `send-keys -K -c $C C-b '"'` | `@hit=yes`; panes 1 → 2 — a binding replays through the key table |
| 4 | `new-window -t @0` / `new-window -t $0:` | `create window failed: index 0 in use` / ok |
| 5 | `command-prompt -t $C -I zz '{' rename-session '%%' '}'` | `command command-prompt: too many arguments (need at most 1)` — braces are config syntax, folded into one string argument |
| 6 | FTXUI 5 source + a popup run through a bash wrapper with `trap INT` | `ScreenInteractive::Post()` drops tasks sent before `Loop()` (no `task_sender_`); ISIG stays on, so C-c signals the whole process group (wrapper logged `wrapper-got-INT`). Clearing ISIG on first render: no signal, rc 0 |
| 7 | `less -R ~/.tmux/cheat.txt` in a `-h 68` popup, LANG=en_US.UTF-8 | PUA glyphs print as `<U+F012C>`, the BORDER row wraps and the last sheet line falls off; with `LESSUTFCHARDEF=E000-F8FF:p,F0000-FFFFD:p` the sheet fits, `(END)` shown. Also: `cheat.txt` had 64 lines against a `-h 65` sized for 62 (rows from e9cc877, 651e3db) |
| 8 | `select-pane -m` twice on one pane; then on another | 1 → 0 (toggle); the mark moves to the other pane |
| 9 | `run-shell -b -t %0 "echo hello"` | output shown in the pane's view mode (`pane_in_mode=1`) — list-*/show-* rows run from `launch.sh` display their output |
| 10 | two windows, @1 active: `TMUX=… tmux swap-window`; then window 0 marked, `tmux swap-window -t t:0` | both rc 0, order unchanged (`0:@0 1:@1`) — bare it swaps a window with itself; `-s` defaults to the MARKED window. Reported by the user as "prefix + m -> swap-window 沒有反應" |
| 11 | `tmux -f /dev/null` server: `rename-window -t @W`; `rename-session -t p` | both rc 1, `too few arguments (need at least 1)` — what Enter on the command row ran. Usages with a word outside every `[…]`: 24 commands on 3.5a. Reported by the user as "prefix + +m -> rename window 不能正常運作" |
| 12 | live server after row 11's fix: `list-windows -F '#{window_name} #{automatic-rename}'`; the bar's `window-status-format` | window 1 already named `aaa` (auto 0) — the rename had worked — but the tab label (`claude.conf` bar-format block) showed only the pane title / dir, never `#W`. Fix: the label shows `#W` when automatic-rename is off and the name is not cc-launch's `cc:<dir>`; rendered after reload: `1 aaa`, `0 ~`. User chose "your names win" (2026-11-11) |

User decisions (2026-11-11): trigger key `prefix m` (accepted: it replaces stock `select-pane -m`, the Claude-inbox mark); `prefix M` rebound to `select-pane -m` so marking stays one key; Meson + Ninja for this tool (the cmake directive of 2026-08-27 was sift's); name `cc-main-menu` and options `@cc-main-menu-*`; in-repo tool under `tools/`, not a standalone TPM plugin; theme Catppuccin Mocha; no background build on config load unless `@cc-main-menu-autobuild on`.

**Limitations.** Not exercised: `build-all.sh` from an empty `target/`, mouse-wheel scrolling, key replay for tables other than prefix and root, terminals other than tmux-in-tmux. `display-popup` not expanding formats is the earlier measurement recorded in `claude.conf`'s sift block, not re-measured here.
