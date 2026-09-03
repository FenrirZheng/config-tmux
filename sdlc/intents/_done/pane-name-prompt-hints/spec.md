# Spec: pane name prompt hints

Author: claude (agent)
Created: 2026-09-03
Status: accepted
Accepted: 2026-09-03

Reads [`intent.md`](intent.md) (`Status: accepted`, amended 2026-09-03). Every
measurement below was taken 2026-09-03 on tmux 3.5a unless marked `[未驗證]`.

Amended 2026-09-03 after acceptance, by user directive in the session that wrote
it: the helper is a C++ binary under `tools/` owning its own TUI, not a bash
script around fzf. The user chose "Own TUI, no fzf" at the prompt. R4 and R7 and
the whole Design section changed; R1 to R3, R5, R6, R8 to R10 stand as accepted.

Amended 2026-09-03 after review: R4's channel is the popup's own header, not
`display-message -l` — the approved plan already specified it that way (step 2:
"`Enter` with `shown` empty sets `notice = …` and redraws (R4, popup stays
open)"), so the built code follows the plan and this edit brings R4 into
agreement with both. Nothing else in R4 changes.

## Requirements

Numbered so `plan.md` and the review can cite them.

- R1 **List first.** `prefix g` opens a popup that lists every pane on the server,
  all sessions, one row per pane: `session:window.pane`, the running command, the
  pane title. The pane id is carried on the row but not displayed. Scope equals the
  current binding's `-a`.
- R2 **Type to filter.** Keystrokes filter the rows on the title column only,
  case-insensitive substring, the same match rule as today's `#{mi:*x*,…}`. The
  command and location columns are shown but not matched, so typing `bash` does not
  match every shell.
- R3 **One confirming key.** `Enter` switches the attached client to the highlighted
  pane: session, window and pane in one move. The cursor opens on the first row, so
  a fragment that narrows the list to one pane needs no `Down`. This is the intent's
  success criterion: `prefix g`, read, type, `Enter`, target active.
- R4 **Fail closed on a miss.** With zero matching rows, `Enter` moves nothing and
  renders `pane-jump: no pane title matches` in the popup's own header, where the
  match count otherwise sits; the popup stays open, so the query can be corrected
  without reopening it. No full tree, no silent success. Exit status is 0, per the
  toolset invariant (`tools/ARCHITECTURE.org:142`: a non-zero exit from a key
  binding surfaces as a tmux error popup). `switch-client` is never called with an
  empty target.
- R5 **Cancel is silent.** `Esc`, `C-c` or `C-g` closes the popup, moves nothing,
  prints nothing.
- R6 **Titles are read, never written.** The tool contains no `select-pane -T` and
  no `rename-window`. Claude-pane readers (`talk ping`, `cc-attend`, `cc-fleet`) are
  unaffected.
- R7 **Missing binary fails visibly.** The binding is guarded by the same
  load-time `if-shell '[ -x …]'` as `sift` (`claude.conf:196`); when the binary is
  not built, `prefix g` prints a `not built` message naming the build command. No
  runtime dependency beyond libc and tmux.
- R8 **Same key, old binding replaced.** The `command-prompt` + `choose-tree` line in
  `tmux.conf` is removed and the key is bound in `claude.conf`, where every
  `tools/` binary key lives. Because the key is rebound rather than left unbound,
  `tmux source-file ~/.tmux.conf` installs it on a running server; the
  stale-binding hazard at `tmux.conf:18-23` applies only to a removed key.
- R9 **Docs move with the key.** `cheat.txt:7` loses `(Down Down, then Enter)` and
  stays one line, at most 81 cells wide, so the `prefix ?` popup height at
  `tmux.conf:47` does not change. `README.md:7` names the popup instead of the
  filtered tree. `runbooks/switch-pane-by-name.md` marks the tool as shipped, keeps
  Option A as the no-binary fallback, and gains Install, Verify and Troubleshoot
  content for the tool. `tools/ARCHITECTURE.org`, `tools/atlas/`, and the
  `.tmux/CLAUDE.md` crate count name the new tool.
- R10 **Verified on a throwaway server.** The build proves R1 to R5 on a nested
  `tmux -L` server with panes titled `alpha-build`, `beta-server`, `gamma-notes`, per
  the runbook's testing note and the `verify-sift-jump.sh` harness shape; the user's
  live server is only read from. The bound key on the live server stays `[未驗證]`
  until the user presses it; the spec does not claim otherwise.

## Design

### Shape: a popup running a C++ TUI, then one `switch-client`

Plain tmux cannot do R1 and R3 together. `command-prompt` is a one-line prompt with
no list; `choose-tree -f` lists but ignores an empty filter (R4 fails) and opens on
the session row (R3 fails); nothing in tmux filters a list as you type. This is the
same gap ADR-0005 records for regex search, and the answer is the same shape:
`display-popup -E` around a program that owns the keystroke loop and hands the final
move back to tmux.

The program is `pane-jump`, C++20, one translation unit, no dependencies beyond
libc, built by cmake into `tools/target/release/` exactly as `sift` is. It reuses
`sift`'s terminal layer by copy: `RawMode`, `TermSize`, `read_key`, the UTF-8
cell-width decoder, `tmux_out` / `tmux_run` over `execvp` with an argv vector. No
fzf, no ncurses.

### Entry points

Three, so the two halves are testable without a TUI, the seam `sift rows` and
`cc-fleet rows` already use:

| invocation | does | exit |
|---|---|---|
| `pane-jump` | rows → TUI → jump; the bound form | 0 always |
| `pane-jump rows [<fragment>]` | prints the rows, tab-separated: `pane_id`, `session:window.pane`, `command`, `title`; with a fragment, only the rows R2 would show | 0 |
| `pane-jump jump <pane_id>` | the guarded `switch-client`; an empty id prints `pane-jump: no pane selected` to stderr and calls nothing | 0 |

### Rows

One tmux call, argv vector, never a shell string:

```
tmux list-panes -a -F '#{pane_id}<TAB>#{session_name}:#{window_index}.#{pane_index}<TAB>#{pane_current_command}<TAB>#{pane_title}'
```

Measured on the live server: tab-separated, one line per pane, titles with the `✳`
prefix pass through intact. Row order is `list-panes` order: session, window, pane
index. The current pane is listed like any other.

### Match

Case-insensitive substring on the title field: both sides are decoded to code
points, folded with `towlower` under the user's locale, and compared as sequences.
ASCII behaves as today's `#{mi:}`; non-ASCII folding is best effort and
`[未驗證]` against tmux's own `mi` for non-ASCII input. The query is a plain
string, not a regex, so `.` and `*` are literal.

### TUI

Header row: `pane name: <query>` and `<shown>/<total>` at the right. Then one row
per match, `location` and `command` dimmed, title plain, the selected row inverted.
Rows longer than the popup are cut at a cell boundary. Keys, the `sift` set minus
ordinal mode: text appends to the query, `Backspace` removes one character, `C-w`
one word, `C-u` the query; `Up` / `Down` / `C-p` / `C-n`, `PgUp` / `PgDn`,
`Home` / `End` move the selection; `Enter` jumps; `Esc` / `C-c` / `C-g` cancel.
Selection resets to the first row whenever the query changes (R3).

### Jump

`Enter` on a row restores the terminal, then runs `tmux switch-client -t <pane_id>`.
The runbook measured on 2026-09-03 that `switch-client -t %0` changes session,
window and pane in one call, and that a `switch-client` issued from inside
`display-popup -E` moves the client that opened the popup (`cc-fleet` does it from
the same position, `tools/cc-fleet/src/main.rs:68`). `Enter` on an empty list prints
the R4 message and returns to the loop; it does not close the popup.

### Format-context safety

Titles are untrusted text. They enter only the popup rendering and the `rows`
stdout; every `display-message` carries `-l`; tmux is always spawned through
`execvp` with an argv vector. This is the `seek` / `sift` route from
`tools/ARCHITECTURE.org:115`, and the proof obligation is the same: grep the source
for `display-message` without `-l` and for any `system`/`popen`, both zero.

### Binding

In `claude.conf`, after the `sift` block, with the same load-time guard:

```tmux
if-shell '[ -x ~/.tmux/tools/target/release/pane-jump ]' {
  bind -T prefix g display-popup -E -w 80% -h 60% -T ' pane name ' \
    "~/.tmux/tools/target/release/pane-jump"
} {
  bind -T prefix g display-message -l 'pane-jump: not built — cd ~/.tmux/tools && cmake -S pane-jump -B target/cmake-build-pane-jump -DCMAKE_BUILD_TYPE=Release && cmake --build target/cmake-build-pane-jump && tmux source-file ~/.tmux.conf'
}
```

`tmux.conf` lines 101-110 are removed; a two-line comment points at `claude.conf`.
The build tree is its own directory under `tools/target/`, already ignored, so the
two cmake projects do not share a cache. Cost inherited from `sift`: `cargo clean`
deletes this binary too.

### Not included, with reason

- **Auto-jump on a unique match.** The intent's success criterion names "one
  confirming key"; an auto-jump would fire on the first letter that happens to be
  unique. Left out and left open.
- **Ordinal mode** (`ADR-0007`, left-Alt-digit). A second feature; the row count
  here is small enough that `Down` suffices. Possible follow-up.
- **Preview of the target pane.** `cc-fleet` shows one; not part of "hint, type,
  jump".
- **Current-pane exclusion or marking.** A no-op jump is harmless.
- **Matching on the command column.** Excluded by R2; display only.
- **A shared C++ terminal library.** Copying ~250 lines from `sift` is chosen over
  extracting a header both tools include, because extraction edits `sift` and its
  ADR/atlas in the same change. Named as a follow-up.

### Files that change

| file | change |
|---|---|
| `tools/pane-jump/CMakeLists.txt`, `CMakePresets.json`, `src/main.cpp` | new |
| `tools/pane-jump/verify-pane-jump.sh` | new headless harness, `verify-sift-jump.sh` shape |
| `claude.conf` | new guarded `prefix g` block after the `sift` block; header build comment |
| `tmux.conf` | lines 101-110 removed, pointer comment left |
| `cheat.txt` | line 7 text, same line count and width budget |
| `README.md` | line 7 entry |
| `runbooks/switch-pane-by-name.md` | shipped recipe, Install, Verify, Troubleshoot, Roll back |
| `tools/ARCHITECTURE.org` | Crates table row, Key bindings row, the "except sift" sentence |
| `tools/atlas/pane-jump.org`, `tools/atlas/index.org` | new node, index entry, Coverage counts |
| `CLAUDE.md` (this repo) | "eleven Rust crates plus sift" sentence |
| `docs/adr/0008-own-the-interaction-loop-for-pane-selection.org` | short ADR citing 0005 as precedent |

Untouched: `tools/sift/`, `tools/tmuxlib/`, `plugins/`, `.gitignore`,
`tools/.gitignore` (`target/` already covers the new build tree). Outside this
repo: the parent dotfiles `~/CLAUDE.md` build snippet lists one cmake tool and
needs a second line; not part of this change, flagged below.

## Flagged concerns

Single-operator repo: the policy owner for each is the user, fenrir.

1. **Copied terminal layer.** ~250 lines of `sift` (`RawMode`, `read_key`, UTF-8)
   are duplicated. A shared header is the clean form and touches `sift`; deferred.
2. **Second cmake project.** The fresh-machine bootstrap in the parent
   `~/CLAUDE.md` and in this repo's `CLAUDE.md` gains a build command. The
   alternative, one top-level `tools/CMakeLists.txt` building both, changes
   `sift`'s documented commands; not chosen.
3. **Exact substring, not fuzzy.** Chosen to match the current rule. Fuzzy is a
   scoring function away and can be decided at review.
4. **ADR number.** `docs/adr/` holds 0001 to 0005 and 0007; 0006 is absent. The
   new record takes 0008 rather than filling the gap, so a reader never wonders
   whether 0006 was reused. Say so if 0006 was reserved.
5. **Runbook scope.** The runbook is 300 lines built around `choose-tree`; R9
   rewrites its recipe and tables rather than appending, so its "what tmux cannot
   do" measurements stay and its shipped recipe changes.

## Open questions carried from intent.md

- **Presentation** — answered: a popup TUI, filtering as you type, the `sift`
  shape. Not `display-message` before a prompt, not a pre-positioned `choose-tree`.
- **Ambiguity on several matches** — answered: the list is always on screen and
  narrows as you type; `Enter` takes the highlighted row, `Down` picks another.
  Whether a unique remaining match should also auto-accept stays open; the spec
  ships without it.
- **Match rule** — answered: substring, case-insensitive, title column only (R2).
- **A miss** — answered: empty list while typing; `Enter` on it prints a message
  and stays open, never the full tree (R4).
