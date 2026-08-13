# Runbook: to-emacs — `prefix E`, full pane scrollback → Emacs buffer

Operational guide for installing, verifying, and troubleshooting the
[`to-emacs`](../tools/to-emacs/src/main.rs) tool on a fresh or existing machine.
Design rationale lives in [the design doc](../plans/to-emacs-scrollback.md);
crate/binding conventions in [ARCHITECTURE.org](../tools/ARCHITECTURE.org).

## What it does

`prefix E` captures the current pane's entire scrollback
(`capture-pane -p -J -S -`, history-limit 100000, trailing blank padding trimmed)
to `/tmp/claude-capture/scrollback-<ts>-<pane>.txt` and opens it in the running
Emacs daemon at the last line — reusing an existing frame (`emacsclient -n`),
else opening a new tmux window named `emacs` (`emacsclient -t`). Outcome is
always reported on the tmux status line.

## Prerequisites

| dependency | why | check |
|---|---|---|
| tmux ≥ 3.5 with this repo's conf | the binding lives in [claude.conf](../claude.conf), sourced from the last line of [tmux.conf](../tmux.conf) | `tmux -V` |
| Rust toolchain | the tool is a crate in the [tools/](../tools/) cargo workspace | `cargo --version` |
| Emacs + daemon via systemd user unit | the destination; the tool refuses to autostart a daemon itself | `systemctl --user status emacs` |
| Claude Code with `"tui": "default"` | only for capturing Claude panes — see [Alt-screen](#alt-screen-panes-capture-one-screen-only) | `rg '"tui"' ~/.claude/settings.json` |

## Install (fresh machine)

Steps 1–2 are part of the home repo's normal bootstrap (see "Fresh-clone
bootstrap" in `~/CLAUDE.md`); they are repeated here so this runbook stands alone.

1. **Populate the repo.** `~/.tmux` is a submodule of the home dotfiles repo:
   `git -C ~ submodule update --init .tmux` (or however this repo got here).
2. **Build the workspace:**

   ```bash
   cd ~/.tmux/tools && cargo build --release
   ```

   The binding references `~/.tmux/tools/target/release/to-emacs` by absolute
   path — no install step, the build output IS the deployment.
3. **Load the binding.** Inside tmux:

   ```bash
   tmux source-file ~/.tmux/tmux.conf
   ```

   (New servers pick it up automatically; reload is only for a running server.)
4. **Emacs daemon:**

   ```bash
   systemctl --user enable --now emacs
   ```

5. **Claude Code renderer** — so Claude panes have a real scrollback to capture:
   `"tui": "default"` in `~/.claude/settings.json` (tracked in the `.claude`
   submodule; set 2026-08-07). Per-session alternative: `/tui default`.
   Env-var override: `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1`.

## Verify

```bash
# binding is live
tmux list-keys | rg to-emacs

# daemon reachable and a usable frame exists (t = yes, nil = only invisible F1)
emacsclient --eval '(if (delq nil (mapcar (lambda (f) (or (frame-parameter f (quote tty)) (memq (framep f) (quote (x w32 ns pgtk))))) (frame-list))) t nil)'
```

End-to-end: in a pane, `seq 1 5000`, then `prefix E` — an Emacs buffer
`scrollback-<ts>-<pane>.txt` opens with 5000 lines, point on the last line, and
the status line shows `→ EMACS scrollback-… (5000 lines, existing frame)`.

Empty-pane path: `clear && tmux clear-history`, `prefix E` → status line says
`scrollback is empty`, exit 0, no file written.

## Troubleshooting

| status-line message / symptom | cause | fix |
|---|---|---|
| `emacs daemon not reachable — systemctl --user start emacs` | daemon down; the tool never autostarts one (a `-a ""` autostart would hang the key binding for seconds, unsupervised) | `systemctl --user start emacs` |
| `⚠ alt-screen TUI: visible screen only` | see [Alt-screen](#alt-screen-panes-capture-one-screen-only) | Claude panes: `/tui default`; other TUIs: `prefix T` tape |
| file opens in a new tmux window instead of your frame | no usable frame existed (probe returned `nil`) — this is the designed fallback, not an error | keep that frame; later captures reuse it |
| `prefix E` does nothing / spread-layout runs | binding not loaded, or binary missing | `tmux source-file ~/.tmux/tmux.conf`; rebuild (`cargo build --release`) |
| buffer opens but you see nothing anywhere | should not happen — the frame probe exists precisely to avoid acting on the daemon's invisible F1 frame (see `~/.claude/commands/emacs-open.md`) | report with the status-line message |

## Alt-screen panes capture one screen only

An alt-screen TUI (Claude Code fullscreen, vim, less) keeps its scroll content
inside the application; the pane's tmux `history_size` is 0, so "full
scrollback" is one screen deep. The tool warns rather than passing that off as
complete. The scrolled-away content of an already-running fullscreen session is
unrecoverable from tmux — switching the renderer only helps from that point on
(for Claude, `/export` inside the session is the retroactive path).

## Update / rebuild

Edit the crate → `cargo build --release` → done; the binding's absolute path
picks up the new binary immediately, no reload needed. Run
`cargo test -p to-emacs` for the pure functions. Commit inside `~/.tmux`
(explicit paths only), then bless the gitlink:
`git -C ~ add .tmux && git -C ~ commit`.

Captures land in `/tmp/claude-capture/` and do not survive a reboot — by
design, shared with the [to-claude capture rail](../plans/to-claude-capture-rail.md).
