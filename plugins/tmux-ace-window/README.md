# tmux-ace-window

Jump to (or swap) tmux panes with single-key labels — Emacs
[`ace-window`](https://github.com/abo-abo/ace-window) for tmux.

Press the trigger key and every pane in the current window gets a one-letter
badge in its border. Press a badge letter and you are on that pane.

```
┌──── a ─────┬──── s ─────┐
│            │            │
│            ├──── d ─────┤
│            │            │
└────────────┴────────────┘
   prefix + a, then press a / s / d
```

## Why not just `prefix + q`?

tmux's built-in `display-panes` is the closest native equivalent, but it only
labels panes with the digits `0`–`9` (so it caps at 10 panes) and the labels
are not configurable. `tmux-ace-window` uses home-row letters, scales past 10
panes, and adds a swap action.

## Requirements

- tmux **3.2+** (needs `display-popup` to capture the keypress)
- `bash`

## Install

### Plain (no plugin manager)

Add to [`~/.tmux.conf`](../../.tmux.conf):

```tmux
run-shell ~/.tmux/plugins/tmux-ace-window/ace-window.tmux
```

### With TPM

```tmux
set -g @plugin 'fenrir/tmux-ace-window'
```

Then reload: `tmux source-file ~/.tmux.conf` (and `prefix + I` for TPM).

## Usage

| Key                   | Action                                          |
|-----------------------|--------------------------------------------------|
| `prefix` + `a`        | Label panes, then jump to the one you pick       |
| `prefix` + `A`        | Label panes, then swap the current pane with it  |
| `Esc` / any other key | Cancel — no pane is changed                      |

Edge cases, matching ace-window's feel:

- **1 pane** — nothing to choose; a short message is shown.
- **2 panes** — the UI is skipped and the action runs on the other pane
  directly (disable via `@ace-window-jump-on-2`).
- **3+ panes** — panes are labelled and a small prompt popup waits for a key.

## Configuration

Set these in [`~/.tmux.conf`](../../.tmux.conf) *before* the `run-shell` line.

| Option                  | Default                           | Meaning                                         |
|-------------------------|-----------------------------------|-------------------------------------------------|
| `@ace-window-key`       | `a`                               | Prefix key that triggers "jump"                 |
| `@ace-window-swap-key`  | `A`                               | Prefix key that triggers "swap"                 |
| `@ace-window-keys`      | `a s d f g h j k l q w e r t y …` | Label keys, assigned to panes in order          |
| `@ace-window-label-bg`  | `red`                             | Badge background colour                         |
| `@ace-window-label-fg`  | `white`                           | Badge foreground colour                         |
| `@ace-window-jump-on-2` | `on`                              | With exactly 2 panes, act directly (`on`/`off`) |

Example:

```tmux
set -g @ace-window-key 'o'
set -g @ace-window-label-bg 'colour33'
set -g @ace-window-keys 'a o e u i d h t n s'   # Dvorak home row
run-shell ~/.tmux/plugins/tmux-ace-window/ace-window.tmux
```

## How it works

1. [`ace-window.tmux`](ace-window.tmux) binds `prefix + a` / `prefix + A` to
   the orchestrator.
2. [`scripts/ace-window.sh`](scripts/ace-window.sh) lists the panes, writes a
   per-pane `@ace_label` option to each, and shows those labels by temporarily
   switching `pane-border-status` to `top` with a badge `pane-border-format`.
   The original border options are stashed so they can be restored.
3. It opens a `display-popup` running
   [`scripts/reader.sh`](scripts/reader.sh), which lists every label (with its
   pane index and running command) and reads exactly one keypress.
4. `reader.sh` finds the pane whose label matches, runs `select-pane` (or
   `swap-pane`), and — via an `EXIT` trap — always restores the border options
   and clears the temporary labels, even on cancel or `Ctrl-C`.

State is intentionally self-healing: each run first calls `ace_restore`
(in [`scripts/helpers.sh`](scripts/helpers.sh)), so a previous interrupted
run never leaves stray badges behind.

## Limitations

- Labels render in the pane *border*, not as a centred overlay — tmux has no
  per-pane content overlay short of redrawing the pane.
- When `pane-border-status` was `off`, turning it on for the prompt shifts
  pane content down by one row for the brief moment the prompt is open.
- More panes than `@ace-window-keys` entries: the extra panes are unlabelled
  and not selectable that round (the default list holds 26).
