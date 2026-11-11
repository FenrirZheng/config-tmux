# Fix: prefix m → rename-window fails

## Context

Choosing the `rename-window` command row and pressing Enter runs `rename-window -t @W`
with no new name. tmux rejects it and `launch.sh` shows the error. Measured on a
throwaway server (tmux 3.5a, `-f /dev/null`):

```
$ cc-main-menu rows rename         →  cmd rename-window … rename-window -t @12
$ tmux rename-window -t @W         →  too few arguments (need at least 1), rc 1
$ tmux rename-session -t p         →  same
```

The noted-key row "Rename current window" (`prefix ,` replayed with `send-keys -K`) is a
different path and already opens tmux's prompt. Only the command row is broken.

The cause is general. 17 commands have a required positional argument in their usage
(the usage with every `[…]` group removed still has a word): `rename-window`,
`rename-session`, `find-window`, `load-buffer`, `save-buffer`, `set-buffer`,
`source-file`, `send-keys`, `set-option`, `set-window-option`, `set-environment`,
`set-hook`, `bind-key`, `unbind-key`, `confirm-before`, `if-shell`, `display-menu`.
Enter on any of these always fails.

This builds on the uncommitted swap-window fix in the working tree (`needs_destination()`,
the Enter branch in `ui.cpp`, verify group 11). Both fixes use the same pattern: when a bare
Enter cannot work, open the argument bar with a prefilled line and a hint, and do not run.

## Change

All files are under `tools/cc-main-menu/`.

1. **`src/tmux_client.{hpp,cpp}`**: add `std::string required_args(const Action&)`. For a
   Command row, remove every bracketed `[…]` group from the usage (nested groups too, such
   as `[-T key-table [-N note]]`). Return what is left after trimming, for example
   `new-name` or `key command [argument ...]`. Return an empty string otherwise. The rule
   comes from the usage string, like `needs_destination()`, and uses no command list.
2. **`src/ui.cpp`, Enter in list mode**: after the `needs_destination` branch, if
   `required_args(*a)` is not empty, set `line = edit_line_for(*a)` (already `"NAME "`),
   put the cursor at the end, set `mode = 1`, and show the non-error status
   `rename-window needs new-name: type it, Enter runs`. Enter in the bar then goes through
   `resolve_line()`, so `-t @W` is still derived.
3. **`src/main.cpp`, `rows` seam**: show these rows as `edit:` plus the prefilled line,
   like the destination rows, so the behaviour can be tested headless.
4. **`scripts/verify.sh`**:
   - `rows rename-window` argv column = `edit:rename-window `.
   - The set of `edit:` command rows = the six destination commands plus the 17 above.
     Pin the exact list so that a tmux upgrade adding one shows up as a diff.
   - `rows kill-window` is not `edit:`. This guards against over-matching.
   - End to end (new group): open the menu, type `rename-window`, press Enter, and wait for
     the `needs new-name` screen. Type `zzwin`, press Enter, then check that the origin
     window's `#{window_name}` is `zzwin`.
5. **Docs, in the same change**: one row in the execution table of `docs/parser-design.org`.
   Extend the `needs_destination` bullet in `CLAUDE.md` to cover "required positional →
   bar". Update `../atlas/cc-main-menu.org`: re-quote, re-hash, run `atlas-stale --crux`
   and `atlas-links`. Add the measurement above to the 2026-11-11 entry of
   `../../.claude/evidence.md`.

## Verification

1. `scripts/build.sh` finishes with 0 warnings.
2. `bash scripts/verify.sh` (10 s timeout) passes, including the new group and the
   swap-window group.
3. `scripts/build.sh --asan` and the ASan verify run with `CCMM_SAN_LOG` show no reports.
4. Live check, read-only: `cc-main-menu rows rename-window` shows `edit:rename-window `.
5. Commit only after you say go. Commit in `~/.tmux` by explicit path, together with the
   swap-window fix (or as a second commit if you prefer):
   `tools: open cc-main-menu's argument bar for commands that need an argument`.
