# Runbook: sift — `prefix /`, popup regex search

Operational guide for installing, verifying, and troubleshooting the
[`sift`](../tools/sift/src/main.rs) tool. Design rationale for the interaction
loop lives in
[ADR-0005](../docs/adr/0005-own-the-interaction-loop-for-regex-search.org); the
Rust-vs-C++ language/build decision is
[ADR-0006](../docs/adr/0006-port-sift-from-cpp-to-rust.org); crate/binding
conventions in [ARCHITECTURE.org](../tools/ARCHITECTURE.org). Its plain-text
sibling on `prefix Space` is [`seek`](seek.md).

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

`Home`/`End` genuinely jump to the first/last match, as the table says — fixed
in the 2026-08-31 Rust port. The C++ binary recognised only the xterm
letter-form escape sequences (`ESC[H`/`ESC[F`); tmux itself sends the numeric
forms (`ESC[1~`/`ESC[4~`), which the C++ decoder did not handle and so leaked
the trailing `~` into the search pattern instead of moving the selection. The
Rust decoder accepts both forms (plus the rxvt numeric variants), so `Home`/
`End` now do what this table always said.

Resizing the popup (e.g. dragging the terminal window) now **redraws the list
at the new size** and keeps whatever you had typed. The C++ binary treated the
`SIGWINCH` the resize delivers as if it were an `Esc` keypress and cancelled
the search outright — undocumented, and also fixed in the port.

`sift` takes an optional pane id (`sift %5`); with none it searches the client's
active pane, which is how the key binding uses it. That is not a convenience —
a popup **cannot** be handed its pane through `#{pane_id}`, because
`display-popup` does not expand formats in its command (nor in `-e`), and
`$TMUX_PANE` inside a popup names the popup rather than the pane beneath it.

After the jump the pane is **in copy-mode with the cursor on the match start**,
and the pattern is registered with tmux itself, so `n` / `N` step to the next and
previous match and seek's `w` / `W` / `l` / `L` grab keys chain from the landing
point exactly as they do after `prefix Space`.

The selection starts on the match **nearest the bottom** — the same
most-recent-first bias as the `search-backward` binding this replaced.

When the pane is on the alternate screen (`vim`, `less`, `htop`) there is no
scrollback to search, and the header says `⚠ visible screen only`.

## Install

`sift` is a normal member of the `tools/` cargo workspace — build it the same
way as every sibling tool. From a fresh clone:

```bash
cd ~/.tmux/tools
cargo build --release
tmux source-file ~/.tmux.conf     # required — the binding's guard is load-time
```

Ported from C++/cmake to Rust on 2026-08-31 ([ADR-0006](../docs/adr/0006-port-sift-from-cpp-to-rust.org)).
No external dependencies beyond the workspace's existing `libc` crate — raw
termios, ANSI, POSIX `regcomp`/`regexec` and glibc `wcwidth`, all reached
through FFI, exactly as the C++ used them directly.

The binary lands in `tools/target/release/` next to every other tool, so
`claude.conf` keeps one path shape for every binary. That used to be shared
with a separate cmake build tree — `cargo clean` deleting `sift` along with
the Rust binaries was a standing footgun this runbook had to warn about — but
with the port there is only one build system: `cargo clean` followed by
`cargo build --release` simply rebuilds `sift` like any other crate here.

## Verify

```bash
bash records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-jump.sh   # 13 assertions
bash records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-live.sh   #  6 assertions
```

Both were run against the Rust binary during the 2026-08-31 port and passed
13/0 and 6/0 respectively — the same counts as the C++ baseline. `sift rows`
output was additionally checked byte-identical across 26 regex patterns, and
26 differential rendering comparisons between the two binaries matched.

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

**`prefix /` flashes a popup that shuts instantly** — `sift` could not resolve
which pane to search, so it exited. Almost always a binding that passes an
argument: `display-popup` does **not** format-expand its `shell-command`, so
`display-popup -E "sift '#{pane_id}'"` hands over those literal characters.
The shipped binding passes *no* argument — `sift` asks tmux for the active pane
itself. Check with:

```bash
tmux list-keys -T prefix | grep -E '^bind-key +-T prefix +/'
```

It should end in `.../release/sift` with nothing after it. Run
`~/.tmux/tools/target/release/sift` from a normal pane to see the error directly.

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

**Nothing matches but you expect hits** — `sift` uses POSIX *extended* regex via
glibc's `regcomp`/`regexec`, the same engine tmux's own search uses. Fewer
Perl-isms are missing than you might expect: measured on glibc 2.41, `\w`,
`\W`, `\s`, `\S`, `\b`, `\<` and `\>` all work as GNU extensions, and
backreferences work too. Only `\d` and lazy quantifiers (`*?`, `+?`) are
genuinely unavailable in this extended-regex dialect — `\d` is read as a
literal `d`. Use `[0-9]` in place of `\d`; there is no substitute for a lazy
quantifier in POSIX ERE.

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
