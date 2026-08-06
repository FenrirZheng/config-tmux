---
captured: 2026-08-06 23:56
session: 5fbc597a-2484-406d-a591-c537995c2889
project_dir: /home/fenrir/.tmux
cwd: /home/fenrir/.tmux
transcript: /home/fenrir/.claude/projects/-home-fenrir--tmux/5fbc597a-2484-406d-a591-c537995c2889.jsonl
source: ExitPlanMode (PostToolUse hook)
plan_source: /home/fenrir/.claude/plans/tmux-buffer-emacs-buffer-staged-castle.md
---

# to-emacs: open a pane's full scrollback in an Emacs buffer (`prefix E`)

## Context

Reading or searching a long pane scrollback today means tmux copy-mode (line-oriented,
no isearch/occur/save). The capture rail (`prefix e` / to-claude) already grabs pane
tails, but its destination is a Claude composer and it caps at 10000 lines. Nothing in
the dotfiles pipes text into Emacs — every emacsclient use is file/frame oriented.

New tool: `prefix E` captures the **entire scrollback** (`capture-pane -S -`,
history-limit is 100000) of the current pane to a file and opens it in the running
Emacs daemon as a file-visiting buffer. User decisions (confirmed): full scrollback;
Rust crate in the existing workspace; file-then-open (not elisp text injection);
`prefix E` (overrides tmux builtin spread-layout — same precedent as `prefix C`
overriding customize-mode, [claude.conf:58](/home/fenrir/.tmux/claude.conf)).

Repo: `/home/fenrir/.tmux` (submodule of the $HOME dotfiles repo — commit inside
first, then bless the gitlink in $HOME).

## Files

| path | change |
|---|---|
| `tools/Cargo.toml` | add `"to-emacs"` to `members` |
| `tools/to-emacs/Cargo.toml` | new — package + `tmuxlib = { workspace = true }` |
| `tools/to-emacs/src/main.rs` | new — the tool |
| `claude.conf` | `bind E` section (after the layout-snapshot block) |
| `cheat.txt` | one line under COPY & GRAB |
| `tools/ARCHITECTURE.md` | crate-table row (:79-89) + key-table row (:124-137) |
| `plans/to-emacs-scrollback.md` | new design doc, capture-rail style |

Pattern source: [to-claude/src/main.rs](/home/fenrir/.tmux/tools/to-claude/src/main.rs) —
`capture_pane` (:427), `capture_file_name` (:125), `stamp` (:449, `date` shell-out, no
time crate), `fail` (:244), test style. Shared lib:
[tmuxlib](/home/fenrir/.tmux/tools/tmuxlib/src/lib.rs) — `capture_dir()` :462
(`/tmp/claude-capture`), `current_pane()` :164, `message()` :356.

## main.rs flow

```
arg1 = source pane id ('#{pane_id}'; empty string → current_pane() fallback, as in to-claude)
1. capture: tmux capture-pane -p -J -S - -t <src>     (-J joins wrapped lines; no -e, ANSI stripped)
2. trim_trailing_blank_lines()  → empty? message("to-emacs: <src> scrollback is empty"), exit 0
   (NOTE: to-claude has NO trim helper — verified; -S - pads below the cursor to pane
    height with blank lines, so to-emacs writes its own pure fn)
3. write capture_dir()/scrollback-<stamp>-<sanitized-pane-id>.txt
   ("scrollback-" prefix so the Emacs buffer name says what it is)
4. frame probe: emacsclient --eval FRAME_PROBE   — deliberately NO -a "" (a dead daemon
   must answer "no", not get autostarted behind a key binding; the fix is named instead)
     spawn err / non-zero → fail("emacs daemon not reachable — systemctl --user start emacs"), exit 1
     stdout "t"           → emacsclient -n +<lastline> <path>        (reuse MRU frame)
     "nil"                → tmux new-window -n emacs "emacsclient -t +<lastline> '<path>'"
5. t::message("→ EMACS <basename> (<n> lines, existing frame|new window)")
```

Frame-detection elisp (single argv element, combines
[emacs-open.md](/home/fenrir/.claude/commands/emacs-open.md)'s tty check with
[claude-editor-gui:60](/home/fenrir/.claude/bin/claude-editor-gui)'s graphical check;
the daemon's invisible F1 frame has neither):

```elisp
(if (delq nil (mapcar (lambda (f) (or (frame-parameter f 'tty) (memq (framep f) '(x w32 ns pgtk)))) (frame-list))) t nil)
```

Functions (pure ones unit-tested, to-claude style):
`capture_scrollback(src) -> Result<Vec<u8>,String>`,
`trim_trailing_blank_lines(&[u8]) -> &[u8]` [tested],
`line_count(&[u8]) -> usize` [tested],
`capture_file_name(stamp, pane) -> String` [tested],
`parse_probe(&str) -> bool` [tested],
`new_window_cmd(path, line) -> String` [tested — quoting present; filename chars are
`[A-Za-z0-9._-]` only so single-quoting is trivially safe],
`frame_usable()`, `open_in_frame()`, `open_in_new_window()`, `stamp()`, `fail()`.

Accepted limitations (one sentence each in the design doc, no overbuild):
- Alt-screen TUI panes: capture-pane sees the alt screen + primary history, not the TUI
  session body — same documented blindness as to-claude; `prefix T` tape is the remedy.
- Capturing the Emacs pane itself: you get the TUI framebuffer as a file; harmless.
- Never `emacsclient -c` from a script (Wayland `could not get terminal name`, per emacs-open.md).
- `+<lastline>` jump in both open paths — the interesting part of a scrollback is its tail.

## claude.conf binding

```
# ── Scrollback → Emacs ─────────────────────────────────────────────────────
# E: capture this pane's ENTIRE scrollback (history-limit 100000) to
#    /tmp/claude-capture/scrollback-<ts>-<pane>.txt and open it in the Emacs
#    daemon — reusing an existing frame, else a new "emacs" tmux window.
#    Overrides tmux's builtin spread-layout, still reachable via
#    `: select-layout -E` (same precedent as prefix C over customize-mode).
bind E run-shell "~/.tmux/tools/target/release/to-emacs '#{pane_id}'"
```

cheat.txt, COPY & GRAB block (after the `prefix ]` line, column-aligned):
`               prefix E        open this pane's FULL scrollback in Emacs`

## Verification

1. `cd /home/fenrir/.tmux/tools && cargo build --release && cargo test -p to-emacs`
2. `tmux source-file ~/.tmux/tmux.conf`
3. Frame-exists path: pane with `seq 1 5000` → `prefix E` → existing Emacs frame shows
   `scrollback-*.txt` at the last line; status line reports `→ EMACS … (≈5000 lines, existing frame)`.
4. No-frames path: close all client frames (daemon survives), probe returns `nil` →
   `prefix E` opens a new tmux window named `emacs` with `emacsclient -t`.
5. Daemon down: `systemctl --user stop emacs` → `prefix E` → status-line error naming
   `systemctl --user start emacs`; then restart the daemon.
6. Empty pane: `clear` + `tmux clear-history` → `prefix E` → "scrollback is empty", exit 0, no file.

## Commit (after verification; no push)

1. In `~/.tmux`: `git add` explicit paths only (the 7 files above) —
   `tmux: add to-emacs — open a pane's full scrollback in the Emacs daemon`.
2. In `$HOME`: `git -C ~ add .tmux && git -C ~ commit` (bless the gitlink).
