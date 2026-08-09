# Runbook: seek — `prefix Space`, search then grab word / line

Operational guide for installing, verifying, and troubleshooting the
[`seek`](../tools/seek/src/main.rs) tool on a fresh or existing machine. Design
rationale lives in [the tmux-seek map](../records/2026-08-09-1116-tmux-seek/tmux-seek.org)
— read its **Current contract** section, which supersedes the tickets under it;
crate/binding conventions in [ARCHITECTURE.md](../tools/ARCHITECTURE.md).

## What it does

`prefix Space` enters copy-mode and opens tmux's own incremental search prompt.
Type any text, press Enter, and the cursor lands on the match. Then four
copy-mode keys grab from there:

| key | grabs | goes to |
|---|---|---|
| `w` | the token under the cursor | Wayland clipboard + tmux buffer |
| `W` | same | + the marked Claude pane (`@ref` for paths, paste otherwise) |
| `l` | the whole logical line under the cursor | Wayland clipboard + tmux buffer |
| `L` | same | + the marked Claude pane |

The pane **stays in copy-mode** after every grab, so the keys repeat across one
search; leave with `q` or Escape. With no live search the four keys still work
as plain grabbers on whatever the cursor is on.

This replaces tmux-thumbs, which could only jump to regexp-matched targets —
paths, URLs, `file.ext:123`. `seek` jumps to *any* text you can type.

## Prerequisites

| dependency | why | check |
|---|---|---|
| tmux ≥ 3.4 | `display-message -l` (the literal flag every seek message relies on) must exist; developed and measured on 3.5a | `tmux -V` |
| this repo's conf | the bindings live in [claude.conf](../claude.conf), sourced from the last line of [tmux.conf](../tmux.conf) | `tmux list-keys -T copy-mode \| rg seek` |
| Rust toolchain | the tool is a crate in the [tools/](../tools/) cargo workspace | `cargo --version` |
| `wl-copy` (wl-clipboard) | the system-clipboard half; `tmux set-buffer` alone does **not** reach the Wayland clipboard | `command -v wl-copy` |
| [`to-claude`](../tools/to-claude/src/main.rs) built | only for `W` / `L` | `test -x ~/.tmux/tools/target/release/to-claude` |

## Install (fresh machine)

Steps 1–2 are part of the home repo's normal bootstrap (step 5 of "Fresh-clone
bootstrap" in `~/CLAUDE.md`); they are repeated here so this runbook stands alone.

1. **Populate the repo.** `~/.tmux` is a submodule of the home dotfiles repo:
   `git -C ~ submodule update --init .tmux`.
2. **Build the workspace:**

   ```bash
   cd ~/.tmux/tools && cargo build --release
   ```

   The bindings reference `~/.tmux/tools/target/release/seek` by absolute path —
   no install step, the build output IS the deployment.
3. **Install the bindings.** The block is written out ready to paste at
   [`assets/seek-bindings.conf`](../records/2026-08-09-1116-tmux-seek/assets/seek-bindings.conf)
   — guard, `prefix Space` prompt, and the four output keys. It goes into
   [`claude.conf`](../claude.conf), which `tmux.conf` sources last, so the
   guard's existence check runs after every plugin. Then reload:

   ```bash
   tmux source-file ~/.tmux/tmux.conf
   ```

   Required after a first build: the load-time guard checks for the binary when
   the config is sourced, so building alone leaves the guard stubs bound.
4. **Install `wl-copy`** if missing: `sudo apt install wl-clipboard`.

## Verify

```bash
# bindings are live — expect prefix Space plus the four copy-mode keys
tmux list-keys -T prefix   | rg 'Space'
tmux list-keys -T copy-mode | rg seek

# the pure half
cd ~/.tmux/tools && cargo test -p seek
```

### Automated — run this first

```bash
bash ~/.tmux/records/2026-08-09-1116-tmux-seek/assets/scripts/verify-seek-headless.sh
```

11 assertions against a throwaway tmux server, using `tmuxlib`'s `CC_TMUX_SOCKET`
seam, with `wl-copy` shadowed by a stub — your session and your clipboard are
never touched. It covers the whole of **grabber mode**: token extraction from a
wrapped path (including from its continuation row), a CJK token through the
width-aware column mapping, line grain trimming, the `--` guard on a
leading-dash token, fail-closed on whitespace, repeat grabs without leaving
copy-mode, and silence when the pane is no longer in copy-mode. Expect
`passed=11 failed=0`.

Then the live-search half:

```bash
bash ~/.tmux/records/2026-08-09-1116-tmux-seek/assets/scripts/verify-seek-live.sh
```

14 assertions covering everything that needs a real search, using **tmux-in-tmux**:
the test client runs inside a pane of an outer throwaway server, so `send-keys`
on the outer server feeds it real keystrokes — prefix, prompt text, Enter.
(`send-keys -t <pane>` alone delivers to the pane's *process* and bypasses tmux's
key table, which is why this looked impossible for a long time.) It covers rows
1-4, 8 and 9, plus the cursor-moved-off-match guard and the `%%%` quote handling.
Expect `passed=14 failed=0`.

Row 1 is the one to watch: it searches 32 lines into the scrollback and asserts
the grab lands on the *match*, not on whatever sits at that row of the live
screen. That is the regression test for the `scroll_position` defect.

Finally the real-clipboard round trip:

```bash
bash ~/.tmux/records/2026-08-09-1116-tmux-seek/assets/scripts/verify-seek-clipboard.sh
```

1 assertion: a real grab with the **real** `wl-copy` (the other two scripts stub
it) reaches the Wayland clipboard and `wl-paste` returns the text. Your clipboard
is snapshotted first and restored on exit; the script aborts rather than run if
the clipboard holds anything but plain text. Expect `passed=1 failed=0`.
`seek` shells out to `wl-copy` directly rather than using tmux's
`copy-pipe`/OSC 52 route, so this works regardless of pane visibility and with
no client attached (measured in ticket t1, and the reason ADR-0002 chose
`wl-copy`).

The `wl-copy` **degrade** path needs no human and no `sudo`: `seek` resolves
`wl-copy` through `PATH`, so the live script shadows it with a stub that exits 1
and asserts both that the tmux buffer is still filled and that the status line
names the failure and the surviving sink.

Everything else in the old nine-row matrix is now covered by the two scripts —
including row 9, repeat grabs after a *live* search, which was the last
unverified inference in the design: `search_present` is measured to stay 1 across
a grab, and the pane is measured to stay in copy-mode.

## Troubleshooting

| status-line message / symptom | cause | fix |
|---|---|---|
| `seek: not built — cd ~/.tmux/tools && cargo build --release && tmux source-file ~/.tmux/tmux.conf` | the load-time guard bound stubs because the binary was missing when the config was sourced | run exactly that; the reload is not optional, the guard is load-time |
| `seek: nothing under the cursor` | the cursor is on whitespace, **or** the query was whitespace-only, **or** punctuation trimming emptied the token | move the cursor onto the text and press again; the pane stays in copy-mode |
| `⚠ wl-copy failed → BUFFER <text>` | `wl-copy` missing, or no Wayland session (bare TTY, X11-only) | `sudo apt install wl-clipboard`; the tmux buffer is filled regardless, so `prefix ]` still works |
| `⚠ wl-copy AND set-buffer failed — nothing was copied` | both sinks failed; the text landed nowhere | check `wl-copy` as above, and that the tmux server is healthy. The message exists so the degrade line never claims a buffer that was not written |
| `→ CLAUDE …` then `⚠ wl-copy failed` | same cause, on the `W` / `L` path — the Claude pane got the text, the clipboard did not | as above. The order is deliberate: the clipboard warning is emitted *after* to-claude's receipt so it is not overwritten |
| `seek: to-claude not built — cd ~/.tmux/tools && cargo build --release` | `W` / `L` pressed with the sibling binary absent; the load-time guard covers only `seek` | rebuild the workspace |
| `W` / `L` say nothing at all | no marked Claude pane and no Claude pane found — `to-claude` reports this itself and its exit code is deliberately swallowed | `prefix m` on a Claude pane to mark it as the inbox |
| `prefix Space` asks for **three** prompts in a row | someone rewrote the binding to use a single `-p "#{?alternate_on,A,B}"` label — `-p` takes a *comma-separated list of prompts*, so the conditional format is read as three | restore the two-branch `if-shell -F '#{alternate_on}'` form; the commas are why it exists |
| the prompt label shows a literal `#{?alternate_on,…}` | same cause — `-p` is not format-expanded (the man page documents expansion only for *template*, under `-F`) | as above |
| the four keys do nothing in copy-mode | bindings not loaded | `tmux source-file ~/.tmux/tmux.conf` |
| a grabbed word containing `#{...}` shows up expanded on the status line | a message path lost its `-l`, or `tmuxlib::message()` was used instead of `message_literal()` | fix the call site — `-l` is the entire injection defence, there is no sanitizing filter behind it |

## Alt-screen panes see one screen only

An alt-screen TUI (Claude Code fullscreen, vim, less) keeps its scroll content
inside the application, so the pane's tmux history is one screen deep and a
search cannot reach anything scrolled away. `seek` warns twice rather than
passing that off as complete: in the prompt label while you type, and again in
the `w` / `l` message. The warning rides only on live-search grabs — in plain
grabber mode there is no search coverage to be wrong about.

## Rollback: restoring tmux-thumbs

The cutover removed thumbs in one commit, so the way back is one revert — plus
three steps that are easy to miss.

```bash
# 1. find the cutover commit — the one that removed the @plugin line
git -C ~/.tmux log --oneline -S '@plugin' -- tmux.conf | head -3

# 2. revert it, and bless the revert in the parent repo
git -C ~/.tmux revert <cutover-commit>
git -C ~ add .tmux && git -C ~ commit

# 3. unbind seek's keys — source-file does NOT remove them (see traps below)
tmux unbind -T copy-mode w
tmux unbind -T copy-mode W
tmux unbind -T copy-mode l
tmux unbind -T copy-mode L

# 4. reload the reverted config
tmux source-file ~/.tmux/tmux.conf

# 5. re-clone tmux-thumbs (needs network)
~/.tmux/plugins/tpm/bin/install_plugins

# 6. build its binary (needs network; bypasses the interactive installer)
cd ~/.tmux/plugins/tmux-thumbs && cargo build --release --target-dir=target
```

Step 6 deliberately does **not** use `tmux-thumbs-install.sh`, and step 5 does not use
`prefix I`. Both of those work, but neither is a command you can paste: the installer
blocks on two `read -rs -n 1` prompts and a `select` menu, and `prefix I` is a keypress.
Step 6 runs exactly what the installer's *Compile* branch runs.

Three traps:

- **`source-file` does not unbind.** Verified: `tmux list-keys -T copy-mode`
  binds none of `w`, `W`, `l`, `L` by default, so reverting the config leaves all
  four seek keys live in the running server, pointing at a binary the revert may
  have orphaned. The explicit `unbind`s — or `tmux kill-server` — are mandatory.
- **Reverting the submodule is not enough.** The `$HOME` repo's gitlink still
  points at the cutover commit until you bless the revert.
- **Rollback needs network twice.** Step 5 clones from GitHub and step 6 fetches
  crates from crates.io. There is no offline path back — this is the real cost of
  deleting a gitignored plugin tree. Expect minutes, not seconds; `prefix Space`
  is dead in between, which is not a second failure. (The interactive
  `tmux-thumbs-install.sh` route additionally offers a precompiled *Download*
  from GitHub releases, if a Rust toolchain is unavailable.)

## Update / rebuild

Edit the crate → `cargo build --release` → done; the bindings' absolute path
picks up the new binary immediately, no reload needed (the reload is only
required the *first* time, to get past the load-time guard). Run
`cargo test -p seek` for the pure functions. Commit inside `~/.tmux` (explicit
paths only), then bless the gitlink: `git -C ~ add .tmux && git -C ~ commit`.
