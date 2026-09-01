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
| `PgUp` / `PgDn` | page |
| `Home` / `End` | **do not work** — see Troubleshoot |
| `Backspace`, `C-w`, `C-u` | delete a character / a word / the whole pattern |
| left `Alt-<digit>` | enter **ordinal mode**, see below |
| `Enter` | jump the pane to the selected occurrence |
| `Esc`, `C-g`, `C-c` | cancel — the pane is not touched |

### Ordinal mode — pick a match by number

Every row carries an ordinal, its position in the current result set (1..*N*).
Left `Alt-<digit>` enters **ordinal mode**, and the keypress is itself the
first digit; the mode is the fastest way to reach an arbitrary candidate
without walking the list one row at a time. Right Alt does **not** work for
this — `~/.config/keyd/default.conf:40` gives `rightalt` to the fcitx5 IME
toggle at the kernel level, so it never reaches tmux on this machine, and the
footer names "left Alt" specifically for that reason. The footer item reads:

```
left-Alt-<n> goto match n
```

`match` rather than a bare `goto` because the verb alone named the action without
its object; `match` is the word already on screen, in the header's `N matches` and
in the ordinal column itself. Lowercase `n` is one candidate's number — capital *N*
is the total.

| key (while in ordinal mode) | does |
|---|---|
| left `Alt-<digit>` | enters the mode; that digit is the first buffered digit. Ignored on `Alt-0`, on an empty list, or on an invalid pattern — none of those names a candidate |
| bare digit | extends the buffer; a digit that would push the ordinal past *N* is simply not buffered — there is no error to see |
| left `Alt-<digit>` (already in the mode) | extends the buffer exactly like a bare digit |
| `↑` / `↓`, `C-p` / `C-n`, `PgUp` / `PgDn` | move the selection **and rewrite the buffer**, so the prompt always names where the cursor actually is |
| `Backspace` | pops one digit; popping the last digit leaves the mode. The *next* `Backspace` is the one that starts deleting the pattern |
| `C-w` / `C-u` | clear the buffer first, before touching the pattern |
| any other non-digit printable | leaves the mode and lands in the pattern |
| `Esc` | leaves the mode (outside the mode it still cancels sift) |
| `Enter` | jumps, unchanged |

While the mode is live the prompt reads `goto>` instead of `regex>` and shows
the buffered digits; the right-hand status keeps showing the match count, so
*N* stays on screen. **The `goto>` prompt is the only indicator that the mode
is live** — there is no separate in-popup marker beyond it.

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

`sift` is C++ and does **not** build with `cargo`. From a fresh clone:

```bash
cd ~/.tmux/tools/sift
cmake --preset release && cmake --build --preset release
tmux source-file ~/.tmux.conf     # required — the binding's guard is load-time
```

[`CMakePresets.json`](../tools/sift/CMakePresets.json) pins the generator
(Ninja) and the build directory, so there is one spelling to remember and the
`-S`/`-B`/`-DCMAKE_BUILD_TYPE` triple cannot drift between this runbook and
whatever you typed last time.

Needs a C++20 compiler (g++ ≥ 10), `ninja`, and cmake ≥ 3.21 for presets. No
ncurses, no external libraries — raw termios, ANSI, POSIX `<regex.h>` and libc
`wcwidth`. **Ninja is a convenience, not a requirement**: without it, or on an
older cmake, the generator-less form still builds the same binary —

```bash
cd ~/.tmux/tools
cmake -S sift -B target/cmake-build -DCMAKE_BUILD_TYPE=Release
cmake --build target/cmake-build -j
```

Do not expect Ninja to speed up the compile. `sift` is a single translation
unit, so there is nothing for a build scheduler to parallelise; measured on this
machine, a full build is 2446 ms under Ninja against 2434 ms under make. What
Ninja actually buys is the **no-op** rebuild — 16 ms against make's 56 ms —
which is the case that recurs while editing. The 2.4 s is g++ optimising one
910-line TU, and only 454 ms of that is the standard headers, so precompiled
headers do not help either (measured: 3014 ms clean, because it also has to
build the PCH). If that 2.4 s ever becomes the bottleneck, `ccache` is the lever
that would move it, not the generator.

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
bash records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-live.sh   # 19 assertions
```

Both spin up a throwaway tmux server and clean it up. `verify-sift-jump.sh`
asserts the jump arithmetic against hand-issued tmux commands;
`verify-sift-live.sh` drives the real TUI with real keystrokes and asserts on the
target pane, so it would catch a wrong wiring that the first script cannot.

### Under the sanitizers

Both scripts honour `$SIFT`, so the same 32 assertions can be re-run against an
ASan + UBSan + LSan build. The sanitizer target is deliberately named
`sift-asan`: it shares the output directory with the release binary, and an
instrumented binary silently taking over `prefix /` would be a slow, confusing
regression rather than a loud one.

```bash
cd ~/.tmux/tools/sift
cmake --preset asan && cmake --build --preset asan
SIFT=~/.tmux/tools/target/release/sift-asan \
  bash ~/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-jump.sh
```

Expect the same `passed 13, failed 0` with no sanitizer output at all. The
instrumented build is roughly 4x slower, so the live script's scrollback timing
assertion has that much less headroom (measured 243 ms against its 300 ms
budget, versus 60 ms uninstrumented) — a failure there under `sift-asan` alone
is the sanitizer, not a regression.

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

**Nothing matches but you expect hits** — `sift` uses POSIX *extended* regex,
which is what tmux's own search uses. Perl-isms are not available: `\d`, `\w`,
`\s` and lazy quantifiers are not extended-regex syntax. Use `[0-9]`, `[[:alnum:]_]`,
`[[:space:]]`.

**`Home` / `End` do nothing, or leak a `~` into the pattern** — this is a known,
pre-existing defect, **not caused by ordinal mode and not fixed here**. tmux
emits `ESC [ 1 ~` for `Home` and `ESC [ 4 ~` for `End`, but sift's CSI decoder
only handles `ESC [ H` and `ESC [ F`; the key is dropped and its trailing `~`
lands in the pattern as ordinary text. Use `PgUp` / `PgDn` or `↑` / `↓` /
`C-p` / `C-n` instead.

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
