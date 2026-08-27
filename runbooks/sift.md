# Runbook: sift — `prefix /`, popup regex search

Operational guide for installing, verifying, and troubleshooting the
[`sift`](../tools/sift/src/main.cpp) tool. Design rationale lives in
[ADR-0005](../docs/adr/0005-own-the-interaction-loop-for-regex-search.org);
crate/binding conventions in [ARCHITECTURE.org](../tools/ARCHITECTURE.org).
Its plain-text sibling on `prefix Space` is [`seek`](seek.md).

## What it does

`prefix /` opens a popup that searches this pane's scrollback by extended
regex, **filtering as you type**. The list shows one row per occurrence with the
match highlighted; `Enter` sends the pane to the one you picked.

| key | does |
|---|---|
| any character | extends the pattern; the list refilters |
| `↑` / `↓`, `C-p` / `C-n` | move the selection |
| `PgUp` / `PgDn`, `Home` / `End` | page, or jump to the first / last match |
| `Backspace`, `C-w`, `C-u` | delete a character / a word / the whole pattern |
| `Enter` | jump the pane to the selected occurrence |
| `Esc`, `C-g`, `C-c` | cancel — the pane is not touched |

After the jump the pane is **in copy-mode with the cursor on the match start**,
and the pattern is registered with tmux itself, so `n` / `N` step to the next and
previous match and seek's `w` / `W` / `l` / `L` grab keys chain from the landing
point exactly as they do after `prefix Space`.

The selection starts on the match **nearest the bottom** — the same
most-recent-first bias as the `search-backward` binding this replaced.

When the pane is on the alternate screen (`vim`, `less`, `htop`) there is no
scrollback to search, and the header says `⚠ visible screen only`.

## Install

`sift` is C++ and does **not** build with `cargo`. From a fresh clone:

```bash
cd ~/.tmux/tools
cmake -S sift -B target/cmake-build -DCMAKE_BUILD_TYPE=Release
cmake --build target/cmake-build -j
tmux source-file ~/.tmux.conf     # required — the binding's guard is load-time
```

Needs only a C++20 compiler (g++ ≥ 10). No ncurses, no external libraries —
raw termios, ANSI, POSIX `<regex.h>` and libc `wcwidth`.

Two things worth knowing about where the build output goes:

- The binary lands in `tools/target/release/` next to the cargo output, so
  `claude.conf` keeps one path shape for every tool. **`cargo clean` deletes it**
  along with the Rust binaries; re-run the `cmake --build` line above.
- The cmake build tree is under `tools/target/` too, which
  [`tools/.gitignore`](../tools/.gitignore) already covers — there is no extra
  ignore rule to add.

## Verify

```bash
bash records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-jump.sh   # 13 assertions
bash records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-live.sh   #  5 assertions
```

Both spin up a throwaway tmux server and clean it up. `verify-sift-jump.sh`
asserts the jump arithmetic against hand-issued tmux commands;
`verify-sift-live.sh` drives the real TUI with real keystrokes and asserts on the
target pane, so it would catch a wrong wiring that the first script cannot.

Both scripts point `$TMUX` at the throwaway socket before invoking `sift`. That
is not incidental: `sift` finds its server through `$TMUX` like every other tool
here, so without it the assertions silently measure the user's real tmux server.
`verify-sift-jump.sh` opens with a control case asserting exactly that.

By hand:

```
prefix /        type  TODO|FIXME   → the list fills as you type
Enter           → the pane lands on that occurrence, in copy-mode
n / N           → step to the next / previous match
w               → seek grabs the token under the cursor
```

## Troubleshoot

**`prefix /` prints "sift: not built"** — the guard is evaluated when the config
is *loaded*, so building is not enough. Run `tmux source-file ~/.tmux.conf`.

**`prefix /` still opens the old status-line prompt** — the config was not
re-sourced after the update. Confirm with:

```bash
tmux list-keys -T prefix | grep -E '^bind-key +-T prefix +/'
```

It must show `display-popup`. If it shows `command-prompt`, re-source.

**"sift: the pane moved — landed on the nearest match"** — the pane printed
enough new output while the popup was open that the captured line indices no
longer point where they did, or tmux's search wrapped. The cursor is on a real
match of the same pattern, just not the one that was picked. Re-run the search.

**Nothing matches but you expect hits** — `sift` uses POSIX *extended* regex,
which is what tmux's own search uses. Perl-isms are not available: `\d`, `\w`,
`\s` and lazy quantifiers are not extended-regex syntax. Use `[0-9]`, `[[:alnum:]_]`,
`[[:space:]]`.

**The header says `⚠ visible screen only`** — the pane is on the alternate
screen; there is no scrollback to search. Leave the full-screen program first.

**A huge match count feels slow** — the collector stops at 20000 occurrences and
the header shows `20000+ matches (capped)`. One filter pass over a full
100000-line scrollback measured **53 ms** on this machine (2026-08-27), so this
should not normally be visible.

## Roll back

To restore the pre-2026-08-27 status-line prompt, replace the `if-shell` block in
[`claude.conf`](../claude.conf) with:

```tmux
bind -T prefix / if-shell -F '#{alternate_on}' \
  'copy-mode ; command-prompt -T search -p "(regex ⚠ visible screen only)" "send-keys -X search-backward \"%%%\""' \
  'copy-mode ; command-prompt -T search -p "(regex)" "send-keys -X search-backward \"%%%\""'
```

then `tmux source-file ~/.tmux.conf`. Nothing else depends on `sift`; deleting
`tools/sift/` and the binary is safe.
