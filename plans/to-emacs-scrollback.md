# to-emacs: a pane's full scrollback in an Emacs buffer

Status: implemented, 2026-08-06. Repo: `~/.tmux` (submodule of the home dotfiles repo).

## Problem

Reading or searching a long scrollback today means tmux copy-mode: line-oriented
navigation, no isearch, no occur, no way to save what you are looking at. The capture
rail ([to-claude](to-claude-capture-rail.md), `prefix e`) grabs pane tails, but its
destination is a Claude composer and its depth caps at 10000 lines. Nothing in the
dotfiles pipes text into Emacs — every existing `emacsclient` use is file/frame
oriented. When a build log or a test run has scrolled 40k lines past, the tool you
want is Emacs, and there is no one-keystroke way to get the scrollback there.

## Design

### User-visible behavior

`prefix E` from any pane:

1. The pane's **entire scrollback** (`capture-pane -p -J -S -`; `history-limit` is
   100000 here) is written to `/tmp/claude-capture/scrollback-<ts>-<pane>.txt`,
   trailing blank padding trimmed.
2. The file opens in the running Emacs daemon, point on the **last line** (`+N` —
   the interesting part of a scrollback is its tail):
   - a usable frame exists → `emacsclient -n` reuses the most-recently-used frame
     (a tmux-hosted TTY frame redraws by itself);
   - no frame anywhere → a new tmux window named `emacs` runs `emacsclient -t`.
3. The status line reports `→ EMACS scrollback-….txt (<n> lines, existing frame|new window)`.

`prefix E` overrides tmux's builtin spread-layout, still reachable via
`: select-layout -E` — same precedent as `prefix C` over `customize-mode`.

### Frame decision tree

The daemon always has an invisible frame F1 (see `~/.claude/commands/emacs-open.md`);
acting "on" it paints nothing anywhere. The probe asks for a frame with a tty or a
graphical terminal — F1 has neither:

```elisp
(if (delq nil (mapcar (lambda (f) (or (frame-parameter f 'tty) (memq (framep f) '(x w32 ns pgtk)))) (frame-list))) t nil)
```

Run as `emacsclient --eval <probe>` — deliberately **without `-a ""`**: a dead daemon
must answer "no" (status-line error naming `systemctl --user start emacs`), not get
autostarted behind a key binding — that would be a multi-second init.el hang, and the
spawned daemon would live outside `emacs.service` supervision. `t` → reuse; `nil` →
new window; spawn error / non-zero exit → daemon down.

Never `emacsclient -c` from a script: under Wayland it dies with
`could not get terminal name` (per emacs-open.md).

### Components

- **[`tools/to-emacs`](../tools/to-emacs/src/main.rs)** — the one binary. Pure
  functions (trailing-blank trim, line count, file naming, probe parse, new-window
  command) are unit-tested to-claude-style.
- **[claude.conf](../claude.conf)** — the `bind E` block.
- **[cheat.txt](../cheat.txt)** — one line in COPY & GRAB.

## Accepted limitations

- **Alt-screen TUIs**: an alt-screen pane has no tmux history (`history_size` is 0) —
  the scroll content lives inside the application, so the capture is one screen deep.
  to-emacs appends `⚠ alt-screen TUI: visible screen only` to its report instead of
  passing that off as a full scrollback. Remedies: for Claude Code panes, disable the
  fullscreen renderer (`"tui": "default"` in settings.json, `/tui default` in-session,
  or `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1`) so the conversation accumulates in real
  tmux scrollback — set globally on this machine 2026-08-07; for other TUIs,
  `prefix T` tape ([pane-tape-recorder](pane-tape-recorder.md)).
- **Capturing the Emacs pane itself**: you get the TUI framebuffer as a file. Harmless.
- **ANSI is stripped** (no `-e`): the buffer is plain text, which is what
  isearch/occur want.
- Captures land in `/tmp/claude-capture` and do not survive a reboot — by design,
  same as the capture rail.

## Verification

1. `cd ~/.tmux/tools && cargo build --release && cargo test -p to-emacs`
2. `tmux source-file ~/.tmux/tmux.conf`
3. Frame exists: pane with `seq 1 5000` → `prefix E` → existing frame shows the file
   at the last line; status line reports the count and "existing frame".
4. No frames: close every client frame (daemon survives) → `prefix E` → new tmux
   window `emacs` opens the file via `-t`.
5. Daemon down: `systemctl --user stop emacs` → `prefix E` → status-line error naming
   the fix; restart after.
6. Empty pane: `clear` + `tmux clear-history` → `prefix E` → "scrollback is empty",
   exit 0, no file written.
