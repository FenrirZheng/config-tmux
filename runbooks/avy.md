# Runbook: avy — `prefix Space`, label-jump anywhere on the screen

Operational guide for installing, verifying, and troubleshooting the
[`avy`](../tools/avy/src/main.rs) tool. Design decisions, the measured
copy-mode cursor semantics it depends on, and the measurement scripts live in
[records/2026-08-21-2310-tmux-avy/](../records/2026-08-21-2310-tmux-avy/);
crate/binding conventions in [ARCHITECTURE.org](../tools/ARCHITECTURE.org).

## What it does

`prefix Space` opens a borderless popup exactly covering the current pane,
showing a snapshot of it (panes freeze while a popup is open, so the snapshot
cannot go stale). Then, avy-goto-char-timer style:

1. **Type characters** — matches on the visible screen highlight as you type
   (smart-case: an all-lowercase query is case-insensitive).
2. **Pause** (`@avy-timeout` ms, default 500) — single-key labels appear over
   the matches. Exactly one match skips the labels and jumps immediately.
   Enter forces labels early; Backspace edits the query.
3. **Press a label** (`@avy-keys`, default `asdfghjkl`; two-key labels when
   matches outnumber keys) — the popup closes and the pane is left **in
   copy-mode with the cursor on the match**.

Escape / `C-g` / `C-c` cancel without touching the pane.

Because the jump ends in copy-mode, [seek](seek.md)'s grab keys chain
directly: `prefix Space` → type → label → `w` grabs the token under the
cursor, `L` sends the line to the marked Claude pane, and so on. This is the
jump half of an EasyMotion-style "jump, then act".

Options (set in tmux.conf / claude.conf, global):

```tmux
set -g @avy-timeout 500        # ms of pause before labels appear
set -g @avy-keys  "asdfghjkl"  # label alphabet, order = preference
```

## Prerequisites

| dependency | why | check |
|---|---|---|
| tmux ≥ 3.3 | `display-popup -B` (borderless) and the `popup_pane_*` position formats; developed and measured on 3.5a | `tmux -V` |
| this repo's conf | the binding lives in [claude.conf](../claude.conf), sourced from the last line of [tmux.conf](../tmux.conf) | `tmux list-keys -T prefix Space` |
| Rust toolchain | the tool is a crate in the [tools/](../tools/) cargo workspace | `cargo --version` |
| `stty` (coreutils) | raw keyboard mode inside the popup — no termios dependency | `command -v stty` |

## Install (fresh machine)

```bash
cd ~/.tmux/tools && cargo build --release
tmux source-file ~/.tmux.conf
tmux list-keys -T prefix Space   # must show ".../avy launch"
```

The guard in claude.conf is LOAD-time: building the binary is not enough, the
config must be re-sourced. Until then `prefix Space` shows an honest
"avy: not built" message.

## Verify

### Automated — run this first

```bash
bash ~/.tmux/records/2026-08-21-2310-tmux-avy/assets/scripts/verify-avy-headless.sh
```

Seven assertions against a throwaway tmux server (CC_TMUX_SOCKET seam):
unique-match instant jump, label selection, mid-line column, CJK wide chars,
a target beyond a wrap boundary, Escape cancel, and a scrolled copy-mode
viewport. The popup itself needs an attached client, so it stays out of the
headless suite — same limitation class as seek's `search_present` rows.

### Manual — the popup path

1. In a pane with some text: `prefix Space` — the popup should cover the pane
   seamlessly (same content, colors intact).
2. Type 2–3 characters of something visible, pause — labels appear; press
   one — cursor lands there, pane is in copy-mode.
3. Press `w` — seek grabs the token (whole jump-and-grab chain).
4. `prefix Space`, Escape — pane untouched, not in copy-mode.

## Troubleshooting

- **`prefix Space` says "avy: not built"** — run the Install block; the stub
  means the binary was missing at config load time.
- **Popup opens but keys echo / nothing highlights** — `stty` failed inside
  the popup; check it exists and that the popup command is `avy ui` (run
  `tmux list-keys -T prefix Space`).
- **Jump lands one cell off on a wrapped line** — that is exactly what the
  measured phantom-step rule prevents; re-run the headless suite. If it fails
  on a new tmux version, re-run the `measure-*.sh` scripts in
  [records/2026-08-21-2310-tmux-avy/assets/scripts/](../records/2026-08-21-2310-tmux-avy/assets/scripts/)
  and update `steps_to` / `locate` in
  [tools/avy/src/main.rs](../tools/avy/src/main.rs).
- **A match on the top screen row won't take a label** — the top row belongs
  to a line wrapped from above the viewport and the two captures disagreed;
  avy fails closed rather than jumping wrong. Scroll a line and retry.

## Alt-screen panes see one screen only

Same caveat as seek: an alternate-screen program (htop, vim) exposes only its
visible screen. avy is by definition visible-screen-only, so behaviour is
consistent — you just can't jump into scrollback that the program doesn't
have.

## Scrollback

avy deliberately covers the **visible screen only** (that is the avy
semantics). For scrollback, `prefix [` then `C-r` incremental search, or
scroll first and then `prefix Space` — a scrolled copy-mode viewport is
supported (headless case 7).

## Rollback: restoring the seek search entry

The pre-avy binding (copy-mode + incremental search prompt) is in git history:

```bash
git -C ~/.tmux log --oneline -- claude.conf   # find the pre-avy commit
git -C ~/.tmux show <commit>:claude.conf      # copy the old Space block back
tmux source-file ~/.tmux.conf
```

`source-file` does not unbind: rebinding Space replaces it, nothing else to
clean up.

## Update / rebuild

```bash
cd ~/.tmux/tools && cargo test -p avy && cargo build --release
bash ~/.tmux/records/2026-08-21-2310-tmux-avy/assets/scripts/verify-avy-headless.sh
```

No re-source needed after a rebuild — the binding calls the binary by path.
