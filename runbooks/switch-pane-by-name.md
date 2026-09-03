# Runbook: switch to a pane by typing its name

How to reach a pane by typing part of its **title** instead of its position.
Shipped as `prefix g` since 2026-09-03: the `pane-jump` popup, Option C below,
bound in [`claude.conf`](../claude.conf) where every `tools/` binary key lives.
This runbook records what else exists, what tmux can and cannot do, and how the
key was built. Measured against tmux 3.5a (`tmux -V`) on 2026-09-03.

Related: [`seek`](seek.md), [`sift`](sift.md) search *inside* a pane; this is
about choosing *which* pane.

## The short answer

**tmux cannot address a pane by name.** A pane target accepts an id, an index,
or a positional token, and nothing else — so `select-pane -t 'my-title'` always
fails. Name-typing has to be built as a *filter over a list*, not as a target.

Three things do that here. The first two are not general:

| what | key | scope | matches on |
|---|---|---|---|
| [cc-fleet](../tools/cc-fleet/src/main.rs) | `prefix S` | **Claude panes only** | `@claude_task`, else `#{pane_title}` |
| [ace-window](../plugins/tmux-ace-window) | `prefix o` / `O` | current window only | nothing — positional letters |
| [pane-jump](../tools/pane-jump/src/main.cpp), a C++ popup bound in [`claude.conf`](../claude.conf) | `prefix g` | **every pane, every session** | `#{pane_title}`, case-insensitive substring |

If the pane you want is a Claude pane, **`prefix S` is already the answer** and
the rest of this runbook is not needed. fzf filters as you type over a column
that carries the task name ([`cc-fleet/src/main.rs:113`](../tools/cc-fleet/src/main.rs)
— `let label = if p.task.is_empty() { &p.title } else { &p.task };`), and the
column is searchable because fzf runs with `--with-nth=2..`, hiding only the
pane-id field ([`cc-fleet/src/main.rs:247-250`](../tools/cc-fleet/src/main.rs)).
Its roster is `list_claude_panes()`, i.e. panes that pass `is_claude()`
([`tmuxlib/src/lib.rs:291-296`](../tools/tmuxlib/src/lib.rs)) — a shell, an
editor or a log tailer is **not** in that list.

The gap this runbook fills: **a non-Claude pane cannot be reached by name at
all** by either of those. `prefix g` is what closes it.

## What already exists

Before `prefix g`, nothing in this repo selected a pane by matching title
text. Confirmed by grepping the whole tree for `pane_title` — every other hit is
a *reader* or a border format, none is a picker.

- **`prefix o` / `prefix O`** — [ace-window](../plugins/tmux-ace-window),
  loaded at [`tmux.conf:99`](../tmux.conf). Overlays a letter on each pane; you
  press the letter. Positional, current window only. The plugin contains **no
  reference to `pane_title` or to any name** (grepped: `title` appears only in
  `helpers.sh:4`, in the phrase "option-name").
- **`prefix S`** — [cc-fleet](../tools/cc-fleet/src/main.rs), bound at
  [`claude.conf:89`](../claude.conf). The type-to-filter picker described above.
- **`prefix N` / `prefix B`** — [cc-attend](../tools/cc-attend/src/main.rs),
  bound at [`claude.conf:83-84`](../claude.conf). Jumps to the pane that most
  needs attention, ranked; you do not name it.
- **`prefix w`** — tmux's own `choose-tree` ([`cheat.txt:11`](../cheat.txt)).
  Interactive, unfiltered.
- **talk-fleet roles** — `talk-fleet role set <name> [<pane>]` binds a
  human-readable name to a pane id under `/tmp/claude-talk/roles/<name>`, and
  `talk-fleet resolve @<name>` prints the pane id
  ([`talk-fleet/src/main.rs:80-84`](../tools/talk-fleet/src/main.rs) lists the
  four subcommands: `role`, `resolve`, `bcast`, `collect`). **There is no
  `goto` / `switch` subcommand** — roles exist for addressing messages, not for
  moving the client. `talk-fleet resolve` does emit a pane id, so it composes
  with a jump; nothing binds that today. [未驗證] — the role commands were read,
  not run.

### Do not write pane titles

[`tools/ARCHITECTURE.org:35`](../tools/ARCHITECTURE.org) states the constraint:

> *Pane titles are never written.* The layout-resurrect plan wanted
> `select-pane -T <slug>`, but `talk ping` reads `#{pane_title}` to decide
> busy/idle — writing it would break inter-agent messaging for every plan at
> once. The slug lives in `@claude_task` and renders on the border instead.

Both readers are real: `cmd_ping` reads `#{pane_title}`
([`~/.claude/skills/talk/talk:99`](../../.claude/skills/talk/talk)), and
`title_state()` keys off the same two shapes
([`tmuxlib/src/lib.rs:306-316`](../tools/tmuxlib/src/lib.rs)) — a leading `✳`
or the literal `Claude Code` means idle, a leading braille character
(U+2800–U+28FF, the spinner) means busy.

**So `select-pane -T` is off the table for naming Claude panes.** It would
silently reclassify the pane as `Unknown` and drop it out of `prefix S`,
`prefix N`, and `talk ping`.

**Reading titles is free**, and it is what makes the recipe below work: Claude
Code writes the title itself, so those panes are *already named*. Live server,
2026-09-03:

```
%7  | ✳ Switch tmux pane by name  | claude | cc:.tmux
%9  | ✳ OCG 2026 work campion deck | claude | cc:ocg-analysis
%14 | ✳ agents/lead.md 修改        | claude | cc:.claude
```

For a **non-Claude** pane there is no such writer, so you must set the title
yourself with `select-pane -T <name>`. That is safe precisely because the pane
is not a Claude pane — `is_claude()` already returns false for it, and no
reader is watching. Do not do it to a pane running `claude`.

## What tmux gives you natively

### A pane cannot be a name target

`man tmux`, TARGETS ("COMMANDS" section). Windows get name matching:

> 4. An exact window name, such as 'mysession:mywindow'.
> 5. The start of a window name, such as 'mysession:mywin'.
> 6. As an fnmatch(3) pattern matched against the window name.

Panes get none of it:

> target-pane (or src-pane or dst-pane) may be a pane ID or takes a similar
> form to target-window but with the optional addition of a period followed by a
> pane index or pane ID, for example: 'mysession:mywindow.1'.  If the pane index
> is omitted, the currently active pane in the specified window is used.

The token list that follows is entirely positional (`{last}`, `{next}`,
`{top}`, `{left-of}`, …) plus `{mouse}` and `{marked}`. **No title rule
exists.** Verified — with three panes titled `alpha-build`, `beta-server`,
`gamma-notes`:

```
$ tmux select-pane -t 'beta-server'
can't find pane: beta-server        # exit 1
```

### The commands that do work on names

| command | matches | selects |
|---|---|---|
| `find-window [-CNT] pattern` | window **name**, window **title**, visible content | a **window**, not a pane |
| `choose-tree -f <filter>` | any format you write, incl. `#{pane_title}` | session, window **or pane** |
| `display-panes` | nothing — digits `0`-`9` | a pane, positionally |

`find-window` is the near miss: it is `prefix f` by default and it does match
titles, but per `man tmux` it searches "window names, titles, and visible
content" and lands you on a **window**. `#{window_title}` is not
`#{pane_title}` — with pane titles set via `select-pane -T`, `window_title`
measured **empty**, so `find-window -T` would not see them.

`choose-tree -f` is the one that reaches panes. Per `man tmux`:

> -f specifies an initial filter: the filter is a format - if it evaluates to
> zero, the item in the list is not shown, otherwise it is shown.  **If a filter
> would lead to an empty list, it is ignored.**

That last sentence is a trap; see Troubleshoot.

## The recipe

### Option C — pane-jump popup — SHIPPED

[`tools/pane-jump/src/main.cpp`](../tools/pane-jump/src/main.cpp): C++20, one
translation unit, nothing beyond libc and tmux. It lists every pane on the
server, narrows the list on every keystroke against `#{pane_title}` only, and
`Enter` runs one `switch-client` on the highlighted pane id. It exists because
no tmux command filters a list as you type, and because `choose-tree -f` fails
in the two ways measured below: the cursor opens on the session row, and a
filter that would leave the list empty is ignored — see
[ADR-0008](../docs/adr/0008-own-the-interaction-loop-for-pane-selection.org).

Bound in [`claude.conf`](../claude.conf) behind the same load-time guard as
`sift`, so an unbuilt binary prints a build command instead of failing silently:

```tmux
if-shell '[ -x ~/.tmux/tools/target/release/pane-jump ]' {
  bind -T prefix g display-popup -E -w 80% -h 60% -T ' pane name ' \
    "~/.tmux/tools/target/release/pane-jump"
} {
  bind -T prefix g display-message -l 'pane-jump: not built — cd ~/.tmux/tools && cmake -S pane-jump -B target/cmake-build-pane-jump -DCMAKE_BUILD_TYPE=Release && cmake --build target/cmake-build-pane-jump && tmux source-file ~/.tmux.conf'
}
```

The binding passes **no argument** on purpose: `display-popup` does not
format-expand its shell-command, so a `#{pane_id}` written there would be handed
over as literal characters. `pane-jump` asks tmux for the pane list itself.

Three entry points, so both halves are testable without a TUI — the same seam
`sift rows` and `cc-fleet rows` use:

| invocation | does | exit |
|---|---|---|
| `pane-jump` | rows → popup → jump; the bound form | 0 |
| `pane-jump rows [<fragment>]` | prints the rows, tab-separated: `pane_id`, `session:window.pane`, command, title; with a fragment, only the rows the popup would show | 0 |
| `pane-jump jump <pane_id>` | the guarded `switch-client`; an empty id prints `pane-jump: no pane selected` on stderr and calls nothing | 0 |

**Every invocation exits 0** — a miss, an empty id and a usage error included.
A non-zero exit from a key binding surfaces as a tmux error popup
([`tools/ARCHITECTURE.org`](../tools/ARCHITECTURE.org)), so exit status is not
an oracle for this tool: the harness below asserts on the active pane and on the
popup's own text instead.

Keys inside the popup:

| key | does |
|---|---|
| any printable character | appends to the query; the list refilters and the selection returns to the first row |
| `Backspace` / `C-w` / `C-u` | deletes one character / one word / the whole query |
| `↑` `↓`, `C-p` `C-n` | moves the selection |
| `PgUp` / `PgDn`, `Home` / `End` | pages, first row, last row |
| `Enter` | `switch-client` to the highlighted pane — session, window and pane in one call |
| `Esc`, `C-c`, `C-g` | closes the popup, moves nothing, prints nothing |

Because the selection returns to the first row on every query change, a fragment
that narrows the list to one pane needs no `Down` before `Enter`. On zero
matches `Enter` moves nothing and the header reads `pane-jump: no pane title
matches`; the popup stays open so the query can be corrected, and the full tree
is never shown — that is the `choose-tree` behaviour this replaced.

The frame with nothing typed, captured 2026-09-03 from the three-pane throwaway
server the harness builds (blank rows between the list and the footer elided):

```
┌─ pane name ──────────────────────────────────────────────────────────────────┐
│pane name:                                                                 3/3│
│> t:0.0 bash       alpha-build                                                │
│  t:0.1 bash       beta-server                                                │
│  t:0.2 bash       gamma-notes                                                │
│type to filter titles  ↑↓ select  Enter jump  Esc cancel  C-w word  C-u clear │
└──────────────────────────────────────────────────────────────────────────────┘
```

The location and command columns are shown but not matched (typing `bash` finds
nothing), and the command column is a fixed width so the title column does not
jitter as the filter changes. Rows are cut at a cell boundary, so a CJK title
does not straddle the popup edge — captured with a `中文筆記-gamma` title on the
same server, 2026-09-03. The fold is `towlower` per code point under the user's
locale: ASCII behaves exactly as the `#{mi:}` rule below, and for non-ASCII it
is best effort, `[未驗證]` against tmux's own `mi`.

Build it before the key works — see [Install](#install).

### Option A — filtered tree (no binary) — fallback

Plain tmux, no binary behind it: the shape to fall back on when `pane-jump` is
not built, or on a machine where the toolset is not installed. It is not bound
today; paste it into [`tmux.conf`](../tmux.conf) and
`tmux source-file ~/.tmux.conf`:

```tmux
bind -T prefix g command-prompt -p "pane name:" \
  "choose-tree -Z -f '#{mi:*%%*,#{pane_title}}'"
```

`#{mi:}` not `#{m:}` — `mi` is the case-insensitive form. `%%` is
`command-prompt`'s substitution of what you typed. `prefix g` was **free**
until 2026-09-03, when it was first bound (checked with
`tmux list-keys -T prefix`; still free are `G`, `F`, `j`, `J`).

Type `prefix g`, a fragment of the title, `Enter`. The tree opens showing only
matching panes plus their parent session and window rows.

**Then press `Down` twice, or the pane row's bracketed shortcut key, before
`Enter`.** The cursor starts on the *session* row whenever the filter excludes
the pane you are currently in, and a blind `Enter` on that row runs
`switch-client -t <session>`, which does not move you. This is the single most
likely way to conclude the recipe "does not work".

### Option B — resolve and jump (no list, one keystroke)

The resolution primitive is `list-panes -f`, which takes the same filter format:

```bash
tmux list-panes -a -f '#{mi:*beta*,#{pane_title}}' -F '#{pane_id}' | head -1
```

Feed that to a jump. The repo's own jump idiom is `focus_pane()`
([`tmuxlib/src/lib.rs`](../tools/tmuxlib/src/lib.rs)), which does
`switch-client -t <session>`, `select-window -t <window_id>`, `select-pane -t
<pane>` — three commands, because it targets the window explicitly. A single
`switch-client -t <pane-id>` also works; `man tmux` documents it and it
measured correct:

> As a special case, -t may refer to a pane (a target that contains ':', '.' or
> '%'), to change session, window and pane.

Binding this needs a helper script rather than an inline `run-shell`: the
filter contains `#{...}`, `%%`, single quotes and a command substitution, and
nesting those inside a tmux string is where it breaks. **Guard the empty
case** — `switch-client -t ""` exits **0** and does nothing, so an unguarded
version reports success on a typo.

The jump half of this option is now `pane-jump jump <pane_id>`, which carries
that guard: an empty or missing id prints `pane-jump: no pane selected` on
stderr and on the status line, calls no tmux command, and still exits 0 (the
key-binding invariant above). The composition is one line — the id is an
argument, not stdin:

```bash
pane-jump jump "$(tmux list-panes -a -f '#{mi:*beta*,#{pane_title}}' -F '#{pane_id}' | head -1)"
```

Nothing binds it: Option C covers the same ground and shows the list.

### Naming a non-Claude pane

```bash
tmux select-pane -t %12 -T 'db-tail'
```

Persists for the life of the pane unless the program inside sets its own title.
Never do this to a pane running `claude` — see "Do not write pane titles".

## Install

`pane-jump` is C++ and does **not** build with `cargo`:

```bash
cd ~/.tmux/tools/pane-jump
cmake --preset release && cmake --build --preset release
tmux source-file ~/.tmux.conf     # required — the binding's guard is load-time
```

[`CMakePresets.json`](../tools/pane-jump/CMakePresets.json) pins the Ninja
generator and the build directory. Without `ninja`, or on cmake < 3.21, the
generator-less form builds the identical binary:

```bash
cd ~/.tmux/tools
cmake -S pane-jump -B target/cmake-build-pane-jump -DCMAKE_BUILD_TYPE=Release
cmake --build target/cmake-build-pane-jump
```

Both spellings write to the same build directory,
`tools/target/cmake-build-pane-jump`, and they produce the same binary — the two
were run back to back on 2026-09-03 and the output hashed to the same sha256.
But **cmake refuses to change generator in an existing tree**: running the
preset over a tree configured by the generator-less form stops with

```
Either remove the CMakeCache.txt file and CMakeFiles directory
or choose a different binary directory.
```

so switching spellings needs `rm -rf tools/target/cmake-build-pane-jump` first.
Same arrangement, and same hazard, as [`sift`](sift.md).

Needs a C++20 compiler (g++ ≥ 10) and cmake; no ncurses, no external libraries.
Two consequences of the output landing in `tools/target/release/` next to the
cargo binaries, both inherited from `sift`:

- **`cargo clean` deletes it.** Re-run the `cmake --build` line, not just
  `cargo build --release`.
- The cmake build tree is under `tools/target/` too, which
  [`tools/.gitignore`](../tools/.gitignore) already covers — no ignore rule to
  add.

## Verify

### The shipped key: pane-jump

```bash
bash ~/.tmux/tools/pane-jump/verify-pane-jump.sh
```

[`verify-pane-jump.sh`](../tools/pane-jump/verify-pane-jump.sh) is 28 assertions
on two throwaway servers — `-L pj_verify` holding three panes titled
`alpha-build`, `beta-server`, `gamma-notes`, and `-L pj_outer` holding the
client attached to it, because keys for a prefix binding must arrive at the
attached client's terminal (see the testing note below). Its first assertion is
the control that `$TMUX` points at the throwaway socket, without which every
assertion would be measuring the user's real server. Run 2026-09-03:

```
passed 28, failed 0
```

exit 0, and the release build compiles with 0 warnings under `sift`'s flag set
(`-Wall -Wextra -Wpedantic -Wshadow -Wconversion -Wsign-conversion
-Wold-style-cast -Wnon-virtual-dtor`).

**That the checker can fail** was shown the same day: pointed at a do-nothing
stub, it reports `passed 12, failed 16` and exits 1.

```bash
PANE_JUMP=/path/to/a/stub/that/does/nothing bash ~/.tmux/tools/pane-jump/verify-pane-jump.sh
```

The keystrokes it drives through `prefix g` on those nested servers, and what
each was observed to do (2026-09-03):

| keys | result |
|---|---|
| `C-b g`, `gamma`, `Enter` | active pane became `%2` (`gamma-notes`) |
| `C-b g`, `alpha`, `Enter` | active pane became `%0` (`alpha-build`) |
| `C-b g`, `Enter` with nothing typed | active pane became `%0` — the first row |
| `C-b g`, `zzz`, `Enter` | popup stays open, header reads `pane-jump: no pane title matches`, nothing moved; `Escape` then closes it |
| `C-b g`, `beta`, `Escape` | nothing moved, and no message logged |
| `pane-jump jump ''` and `pane-jump jump` with no id | `no pane selected` on stderr, exit 0, nothing moved |

The silent-cancel row needs its own oracle: `show-messages` on 3.5a logs every
command the server ran, so the log grows whether or not anything was printed.
A `display-message -l` from the tool logs one ` message: ` line, so the harness
counts those and not the log's length — with a positive control asserting that
a message the tool *does* print moves the same counter.

One behaviour worth naming because it was the plan's most likely break: a plain
`switch-client -t <pane_id>` issued from inside the popup, after the terminal
has been restored, moves the client that opened the popup. No `-c` argument was
needed.

### The fallback, and what tmux does natively

Everything below was run on a throwaway server (`tmux -L pbnprobe -f /dev/null`)
with three panes titled `alpha-build`, `beta-server`, `gamma-notes`, plus a
second throwaway server holding the attached client that `choose-tree`
requires. The user's real server was only ever read from.

```bash
# the negative control: panes have no name resolution
tmux select-pane -t 'beta-server'          # -> "can't find pane", exit 1

# the filter format
tmux list-panes -a -f '#{mi:*BETA*,#{pane_title}}' -F '#{pane_id} #{pane_title}'
# -> %1 beta-server        (case-insensitive, one row)

# the jump
tmux switch-client -t %0                   # -> exit 0, active pane becomes %0
```

Results, all observed:

| test | result |
|---|---|
| `select-pane -t 'beta-server'` | `can't find pane: beta-server`, exit 1 |
| `#{m:*beta*,#{pane_title}}` per pane | `no` / `MATCH` / `no` |
| `list-panes -a -f` with that filter | one row, `%1 beta-server` |
| `#{mi:}` against `*BETA*` | matched — case-insensitive confirmed |
| `choose-tree -Z -f '#{mi:*beta*,…}'` | tree showed only `1: bash: "beta-server"` |
| `Down Down Enter` in that tree | active pane became `%1 beta-server` |
| shortcut key `2` in that tree | active pane became `%0 alpha-build` |
| **Enter with no navigation** | **active pane unchanged** — cursor was on the session row |
| full `prefix g` → `gamma` → `Enter` → `Down Down Enter` | active pane became `%2 gamma-notes` |
| `switch-client -t %0` | exit 0, pane changed |
| `switch-client -t ""` | exit 0, **nothing happened** |
| filter matching nothing | filter ignored, **all three panes shown** |
| `window_title` with pane titles set | empty string |

Not verified [未驗證]:

- **The bound key pressed on the user's live server.** The block is installed in
  [`claude.conf`](../claude.conf), and the binding step recorded that
  `tmux list-keys -T prefix g` prints the `display-popup` form there after
  `source-file` (2026-09-03; not re-run when this section was written). A
  keypress cannot be delivered to that client from a script — see the testing
  note above.
  The same binding line, and the keystrokes above, were exercised verbatim on
  the throwaway servers.
- Non-ASCII case folding against tmux's own `#{mi:}`. `pane-jump` folds with
  `towlower` per code point; that it agrees with `mi` for non-ASCII input was
  not measured. CJK titles were only checked for rendering, where they were
  observed to stay inside the popup edge.
- `talk-fleet role set` / `resolve` end to end — source read, not run.
- Option B as a bound key. Its two halves were each measured; the helper script
  that joins them was not written.

Testing note: `tmux send-keys -t <pane> C-b g` does **not** trigger a prefix
binding — `send-keys` injects into the pane and bypasses the key table. Keys
must be delivered to the terminal of the attached client, which is why the
verification used a nested server and sent keys to the *outer* pane.

## Troubleshoot

**`prefix g` prints `pane-jump: not built`** — the guard is evaluated when the
config is *loaded*, so building the binary is not enough: run
`tmux source-file ~/.tmux.conf`. If the message survives that, the binary really
is missing — see [Install](#install). Check which form is bound with:

```bash
tmux list-keys -T prefix | grep -E '^bind-key +-T prefix +g'
```

It should end in `.../release/pane-jump` with nothing after it.

**The popup header says `pane-jump: no pane title matches`** — zero rows matched
the query, and `Enter` therefore moved nothing. Only the **title** column is
matched, so typing a command name such as `bash` matches nothing by design. Read
the titles you are matching against:

```bash
tmux list-panes -a -F '#{pane_id} [#{pane_title}]'
```

Correct the query in place — the popup is still open — or `Esc` out.

**`pane-jump: no terminal (run it from a tmux popup)`** — the binary was started
without a terminal on stdin: from a script, a pipeline, or a `run-shell`. The
popup needs a tty; the two headless seams do not, so use `pane-jump rows` and
`pane-jump jump <pane_id>` there. It still exits 0 (measured 2026-09-03), and
if the server has **no attached client at all** the message cannot be delivered
either — tmux answers the `display-message` with `no current client` and the run
is silent. Look in `tmux show-messages` when a scripted run appears to do
nothing.

The next two entries are about the **Option A fallback**; `pane-jump` has
neither failure mode.

**Typing a name shows every pane instead of one** — the filter matched nothing,
and `man tmux` says a filter leading to an empty list "is ignored". A full tree
after typing a name means **zero matches**, not a broken filter. Check the
titles you are matching against:

```bash
tmux list-panes -a -F '#{pane_id} [#{pane_title}]'
```

**The tree filters correctly but `Enter` does not move me** — the cursor is on
the session or window row, not the pane row. Press `Down` until the highlighted
line is the one reading `<index>: <command>: "<title>"`, or press the bracketed
shortcut key at the left of that row. Verified: blind `Enter` left the active
pane unchanged; `Down Down Enter` moved it.

**A Claude pane disappeared from `prefix S` / `prefix N`** — something wrote its
pane title. `is_claude()` falls back to the title shape when no `@claude_state`
is stamped ([`tmuxlib/src/lib.rs:291-296`](../tools/tmuxlib/src/lib.rs)), so a
custom title reclassifies the pane as `Unknown`. Do not `select-pane -T` a
Claude pane. There is no unset; restart the pane's Claude session, or wait for
the next hook to stamp `@claude_state`.

**`talk ping` reports the wrong state** — same cause, same fix.

**The jump silently does nothing** — `switch-client -t ""` exits 0. The
resolution step found no pane and the empty string was passed through. Guard it.

**A name matches two panes** — `list-panes -f | head -1` picks arbitrarily
(source order, not relevance). Use Option C, which keeps every match on screen
and lets you pick one.

**`prefix f` (`find-window`) does not find my pane title** — by design. It
searches window names, window titles and visible content, and selects a window.
`#{window_title}` measured empty while pane titles were set.

## Roll back

Option C ships. To drop it, delete the `prefix g` `if-shell` block from
[`claude.conf`](../claude.conf), then choose one of two endings.

To fall back to the filtered tree, paste the Option A binding into
[`tmux.conf`](../tmux.conf):

```tmux
bind -T prefix g command-prompt -p "pane name:" \
  "choose-tree -Z -f '#{mi:*%%*,#{pane_title}}'"
```

and run `tmux source-file ~/.tmux.conf`. A *rebind* does replace the popup
binding on a running server.

To leave the key unbound, run `tmux unbind g` on the running server. This one is
not optional: `source-file` does **not** remove a stale binding, so deleting the
block alone leaves `prefix g` still opening the popup until the server restarts
— the same hazard recorded at [`tmux.conf:18-23`](../tmux.conf) and in
[seek.md](seek.md).

Deleting `tools/pane-jump/` and its binary is safe; nothing else in the repo
reads them.

Titles set by hand with `select-pane -T` persist for the life of the pane; kill
the pane or let the program overwrite the title.
