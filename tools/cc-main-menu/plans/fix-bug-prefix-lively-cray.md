# Fix: prefix m → swap-window does nothing

## Context

Choosing `swap-window` in the menu and pressing Enter has no visible effect. Read-only
evidence from the live server:

```
$ tmux list-commands -F '#{command_list_name}|#{command_list_usage}' | grep ^swap-window
swap-window|[-d] [-s src-window] [-t dst-window]
$ cc-main-menu resolve swap-window
swap-window
```

`derived_target_flags()` (`src/tmux_client.cpp`) only recognises `-t target-*` / `-c target-client`.
For the source/destination family it adds nothing, so Enter runs bare `swap-window`.
`launch.sh` runs that from outside the client, after the popup has closed. tmux then resolves
source and destination to the same current window, and swapping a window with itself
succeeds silently. Two related defects come from the same gap:

1. **Bare Enter is a no-op** for every command whose usage is `[-s src-X] [-t dst-X]` with
   the same kind X: `swap-window`, `swap-pane`, `link-window`, `move-window` (window), and
   `join-pane`, `move-pane` (pane). Without a destination they have nothing useful to do.
2. **Even with `-t` typed in the argument bar, the source can be wrong.** Without `-s`, tmux
   uses the *marked* pane's window or pane as the source when a mark exists, and `prefix M`
   marks the Claude inbox pane. So `swap-window -t 2` can move the inbox's window instead of
   the one the menu was opened from.

## Change

All in `tools/cc-main-menu/` (single tool; no shared code touched).

1. **`src/tmux_client.cpp` — `derived_target_flags()`**: when the usage has `-s src-window` or
   `-s src-pane` and the user did not type `-s`, add `-s <ctx.window>` / `-s <ctx.pane>`.
   It goes right after the command name, like `-t` (the existing `has_flag()` handles the
   skip). This also fixes `break-pane` (`-s src-pane`), which then breaks the origin pane
   rather than the guessed one.
2. **`src/tmux_client.{hpp,cpp}` — new `bool needs_destination(const Action&)`**: true when the
   usage contains `-s src-window` and `-t dst-window`, or `-s src-pane` and `-t dst-pane`
   (same kind on both sides). This is derived from the usage string, like the existing rule,
   not from a command list. `break-pane` (src-pane → dst-window) stays runnable bare.
3. **`src/ui.cpp` — Enter on such a row** does not run. It does what Tab does: opens the
   argument bar prefilled with `edit_line_for(action) + "-t "`, cursor at the end, and sets a
   non-error status: `swap-window needs a destination: type it after -t (e.g. :2, @5), Enter
   runs`. Enter in the bar then runs the line through `resolve_line()` as today, so `-s` is
   filled in automatically.
4. **`src/main.cpp` — `rows` seam**: for these rows, print `edit:` plus the prefilled line in
   the argv column, so the behaviour is testable headless.
5. **`scripts/verify.sh`** — new assertions:
   - `resolve 'swap-window -t :2'` → `swap-window -s @W -t :2`; the same with a user `-s`
     → no derived `-s`.
   - `resolve break-pane` → `break-pane -s %P`.
   - `rows swap-window` argv column starts with `edit:`; `rows break-pane` does not.
   - End to end: two windows A, B with B active, and a pane in window A marked (the inbox
     case). Open the menu, type `swap-window`, Enter → the bar shows `swap-window -t`. Type
     the index of A, Enter → B and A swap: B's window id now has A's old index. This shows
     the origin (B) was the source, not the marked window.
6. **Docs, same change**:
   - `docs/parser-design.org`: execution table gets the `-s src-*` row and the
     needs-destination rule.
   - `CLAUDE.md` § load-bearing decisions: one bullet.
   - `../atlas/cc-main-menu.org`: the Crux excerpt of `derived_target_flags` changes, so
     re-quote it, re-hash the sources, and run `atlas-stale --crux` and `atlas-links`.
   - `../../.claude/evidence.md`: add the measurement (bare `swap-window` from the command
     line is a silent no-op; `-s` defaults to the marked window) to the 2026-11-11 entry's
     table.

## Verification

1. Before the fix, reproduce on throwaway servers (not the live one): `verify.sh`-style
   fixture with two windows, run `swap-window` through the menu, and confirm the window order
   is unchanged with rc 0. This is the bug, confirmed by measurement and not only from
   reading the code.
2. `scripts/build.sh`: zero warnings.
3. `bash scripts/verify.sh` (timeout 10 s): all pass, including the new group.
4. `scripts/build.sh --asan` plus the ASan verify run with `CCMM_SAN_LOG`: no reports.
5. Atlas: `atlas-stale.py --crux` exits 0 and `atlas-links.py` reports 0 findings.
6. `tmux source-file` is not needed (the binary is re-read on every popup). Confirm on the
   live server read-only: `cc-main-menu rows swap-window` shows `edit:`.
7. Commit in `~/.tmux` only, by explicit path, subject
   `tools: give cc-main-menu's swap/move/join commands a source and destination`. Commit only
   on the user's go-ahead.
