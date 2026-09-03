# Plan: pane name prompt hints

Author: claude (agent)
Created: 2026-09-03
Status: draft

Implements [`spec.md`](spec.md) (`Status: accepted`, 2026-09-03). Requirement
numbers R1 to R10 are the spec's. Facts marked with a date were probed on that date
on tmux 3.5a and fzf 0.60; anything else is `[未驗證]`.

## Files that change

| file | change | requirement |
|---|---|---|
| `bin/pane-jump` | new, `chmod +x`, bash | R1 to R7 |
| `tmux.conf` lines 101-110 | replace the `prefix g` comment block and two-line binding | R8 |
| `cheat.txt` line 7 | replace the text after `prefix g`, keep one line, at most 81 cells (today 77) | R9 |
| `README.md` line 7 | replace the entry text | R9 |
| `runbooks/switch-pane-by-name.md` | Option A demoted to fallback, new shipped recipe, Verify table replaced, Troubleshoot and Roll back updated | R9, R10 |

Untouched: `claude.conf`, `tools/`, `plugins/`, `.gitignore`, `CLAUDE.md`. No
`tools/atlas/` node covers these files, so no atlas update is due.

## Order of work

Each step names its check. A step with no executable oracle names what the reviewer
reads. Work on branch `intent/pane-name-prompt-hints`; one commit per step in the
repo's `<area>: <verb> <thing>` style.

1. **Write `bin/pane-jump`.** Contents:

   ```bash
   #!/usr/bin/env bash
   # pane-jump: list every pane's title, filter as you type, Enter jumps.
   # Bound to `prefix g` inside display-popup -E (tmux.conf). Titles are only
   # ever read here; see runbooks/switch-pane-by-name.md, "Do not write pane titles".
   set -euo pipefail

   rows() {
     tmux list-panes -a -F \
       '#{pane_id}	#{session_name}:#{window_index}.#{pane_index}	#{pane_current_command}	#{pane_title}'
   }

   jump() {
     local id=${1:-}
     if [ -z "$id" ]; then
       tmux display-message 'pane-jump: no pane selected'
       exit 2
     fi
     tmux switch-client -t "$id"
   }

   case ${1:-} in
     rows) rows; exit 0 ;;
     jump) shift; jump "$@"; exit 0 ;;
     '') ;;
     *) echo "usage: pane-jump [rows | jump <pane_id>]" >&2; exit 64 ;;
   esac

   if ! command -v fzf >/dev/null 2>&1; then
     tmux display-message 'pane-jump: fzf not installed'
     exit 1
   fi

   rc=0
   sel=$(rows | fzf --delimiter=$'\t' --with-nth=2.. --nth=4 --exact -i \
           --layout=reverse --prompt='pane name: ' --no-multi) || rc=$?
   case $rc in
     0)   jump "$(printf '%s' "$sel" | cut -f1)" ;;
     1)   tmux display-message 'pane-jump: no pane title matches'; exit 1 ;;
     130) exit 130 ;;
     *)   tmux display-message "pane-jump: fzf exited $rc"; exit "$rc" ;;
   esac
   ```

   The literal tab inside the `-F` string is a real tab character, not `\t`; tmux
   formats do not unescape. `--delimiter=$'\t'` is bash's tab. `--nth=4` counts
   original fields, so the hidden id column still shifts the title to field 4.

   Check: `bash -n bin/pane-jump` exits 0; `shellcheck bin/pane-jump` prints
   nothing if shellcheck is installed, else skipped and reported as such;
   `bin/pane-jump rows` on the live server prints one tab-separated line per pane
   and `bin/pane-jump rows | wc -l` equals `tmux list-panes -a | wc -l`.
   Live server is read only here: `rows` runs `list-panes`, nothing else.

2. **Prove the halves on a throwaway server** (R1, R3, R4, R7 guards, R10). Nothing
   here touches the live server. Two nested servers, the shape the runbook's Verify
   section already uses: `pj` holds the panes, `outer` holds the client attached to
   `pj`, because `switch-client` needs an attached client.

   ```bash
   tmux -L pj -f /dev/null new-session -d -s t -x 80 -y 24
   tmux -L pj split-window -t t:0; tmux -L pj split-window -t t:0
   tmux -L pj select-pane -t t:0.0 -T alpha-build
   tmux -L pj select-pane -t t:0.1 -T beta-server
   tmux -L pj select-pane -t t:0.2 -T gamma-notes
   tmux -L outer -f /dev/null new-session -d -s o -x 100 -y 30 'tmux -L pj attach -t t'
   PJ=$(tmux -L pj display -p '#{socket_path}')
   ```

   Then, with `TMUX="$PJ,0,0"` exported so bare `tmux` inside the script reaches
   `pj`:

   | probe | expected |
   |---|---|
   | `bin/pane-jump rows` | three lines, field 4 is `alpha-build` / `beta-server` / `gamma-notes` |
   | `bin/pane-jump rows \| fzf --filter=BETA --delimiter=$'\t' --nth=4 --exact -i` | one line, `beta-server`; exit 0 |
   | same with `--filter=bash` | no output, exit 1 (command column not matched, R2) |
   | same with `--filter=zzz` | no output, exit 1 |
   | `bin/pane-jump jump %1` | exit 0, then `tmux -L pj display -p '#{pane_id}'` prints `%1` |
   | `bin/pane-jump jump ''` | exit 2, nothing moves |
   | `PATH=/nonexistent bin/pane-jump` | exit 1 (fzf guard; `tmux` must still resolve, so prepend a dir holding only a `tmux` symlink) |

   Check: every row observed as expected; the observed values go into the runbook
   Verify table in step 5. A row that differs stops the build and is reported.

3. **Rebind `prefix g` in `tmux.conf`** (R8). Replace lines 101-110 with:

   ```tmux
   # prefix + g : jump to a pane by TITLE, across sessions. Opens a popup listing
   # every pane; typing filters on the title (case-insensitive substring);
   # Enter jumps. Esc cancels; a miss says so and moves nothing. bin/pane-jump
   # holds the logic; recipe and measurements: runbooks/switch-pane-by-name.md.
   # Replaced the command-prompt + choose-tree form on 2026-09-03 (that one
   # opened on the session row and showed the full tree on a miss).
   bind -T prefix g display-popup -E -w 80% -h 60% -T ' pane name ' "~/.tmux/bin/pane-jump"
   ```

   No `unbind`: the key is rebound, so `source-file` replaces it on a running server.

   Check: `tmux -L pj source-file ~/.tmux/tmux.conf` is not usable (it loads TPM
   and the plugin loaders), so on `pj` run the one `bind` line by hand, then
   `tmux -L pj list-keys -T prefix g` prints the `display-popup` form. On the live
   server, after `tmux source-file ~/.tmux.conf`, `tmux list-keys -T prefix g`
   prints the same form. That reload is the one live-server write in this plan and
   is the reload the user runs after any config edit.

4. **Prove the bound key end to end on the throwaway server** (R1, R3, R4, R5).
   Keys go to the `outer` pane, whose terminal is the `pj` client; `send-keys`
   into a `pj` pane would bypass the key table (runbook testing note).

   | keys to `outer` | then read | expected |
   |---|---|---|
   | `C-b g`, `gamma`, `Enter` | `tmux -L pj display -p '#{pane_id}'` | `%2` |
   | `C-b g`, `alpha`, `Enter` | same | `%0` |
   | `C-b g`, `zzz`, `Enter` | same, and `tmux -L pj show-messages \| tail -1` | pane unchanged; last message contains `no pane title matches` |
   | `C-b g`, `beta`, `Escape` | same | pane unchanged; no new message |
   | `C-b g`, `Enter` (no typing) | same | `%0`, the first row |

   Between key groups sleep 0.3 s for the popup to open. Check: all five rows
   observed. Then `tmux -L outer kill-server; tmux -L pj kill-server`.

5. **Update the runbook** (R9, R10). In `runbooks/switch-pane-by-name.md`:
   - "The short answer" table: the `prefix g` row becomes `bin/pane-jump` (fzf popup
     over `#{pane_title}`), scope and match column unchanged.
   - "The recipe": new first subsection "Option C — fzf popup — SHIPPED" with the
     binding, the script's three entry points, and the exit-code table from the
     spec. Option A keeps its text, retitled "filtered tree (no helper script,
     no fzf) — fallback", its `SHIPPED` marker removed. Option B stays as the
     one-command form the script's `jump` now implements; say so in one line.
   - "Verify": add a second table with the observed values from steps 2 and 4,
     dated. Keep the existing table; its measurements still hold for the fallback.
     Move "the bound key on the real server" out of the `[未驗證]` list only if the
     user reports pressing it; otherwise it stays listed.
   - "Troubleshoot": add `**Popup opens and closes at once, status line says fzf not
     installed**` and `**Enter shows "no pane title matches"**`; the `Down Down`
     entry moves under a "fallback (Option A)" note.
   - "Roll back": the shipped key is now `display-popup`; to fall back, paste the
     Option A binding over it and `source-file`; to drop the key, `unbind g`.

   Check: no executable oracle. The reviewer reads that every claim in the new
   Verify table matches a step 2 or step 4 row, and that no line says the live
   key was pressed.

6. **Update `cheat.txt` line 7 and `README.md` line 7** (R9).
   - `cheat.txt:7`: `               prefix g        jump to pane by title: list, type, Enter`.
   - `README.md:7`: ``- [switch-pane-by-name](runbooks/switch-pane-by-name.md) — `prefix g`: fzf popup over `#{pane_title}` — type part of a pane's title, Enter jumps``.

   Check: `wc -l cheat.txt` prints 60; `sed -n 7p cheat.txt | wc -m` is at most 82
   (81 cells plus newline); `prefix ?` popup on the live server shows the sheet in
   one screenful without paging, which the user confirms by eye.

7. **Review and merge.** Run `/code-review` against `main` per the `implement`
   body; every finding is addressed or answered in the PR or merge commit. Merge
   to `main` by fast-forward or merge commit; there is no PR flow in this repo, so
   the close is `sdlc close pane-name-prompt-hints --merge <sha>`.

## Risks

- **Most likely break: the tab in the `-F` string.** An editor that converts tabs
  to spaces silently turns four fields into one and `--nth=4` matches nothing.
  Step 1's `rows` check catches it; keep the file out of any auto-format.
- **`switch-client` from inside the popup.** The spec relies on `cc-fleet` proving
  this works from the same position. If it does not on this server, the symptom is
  exit 0 with no move. Step 4 rows 1 and 2 are the oracle; the fix is
  `switch-client -c "$(tmux display -p '#{client_name}')" -t "$id"`, the form
  `focus_pane(Some(client), …)` uses.
- **`set -e` and fzf's exit 1.** A `$(...)` assignment under `set -e` aborts the
  script on exit 1 before the `case`. The `|| rc=$?` on the assignment line is
  what keeps R4's message reachable; a refactor that drops it reintroduces a
  silent close.
- **A title containing a tab.** Would shift the title into field 5; not matched.
  No writer in this repo emits one; accepted, noted in the runbook.
- **Popup size on a narrow client.** 80 % × 60 % is a percentage; the sheet popup
  at `tmux.conf:47` avoids percentages for a fixed-column file, but fzf reflows, so
  percentages are fine here.
- **`display-message` after the popup closes.** cc-fleet's comment at
  `tools/cc-fleet/src/main.rs:57` notes the popup closes the instant the program
  returns, so the message lands on the status line, not in the popup. Expected.
- **Runbook rewrite scope.** Step 5 edits five sections of a 300-line file; the
  reviewer diff is the control. Spec concern 4 accepted this.
- **What was not chosen.** A Rust crate in `tools/` (spec: out of scope), a
  `plugins/tmux-pane-jump/` loader (spec concern 1: heavier for one script),
  fuzzy matching (spec concern 3), auto-accept on a unique match (spec: not
  included), a `--preview` pane (spec: not included).

## Proof

- **Static**: `bash -n bin/pane-jump` exit 0; `shellcheck` clean or reported
  skipped; `git diff --stat main` lists exactly the five paths in Files that
  change plus `plan.md` if it was amended.
- **Halves** (step 2): the seven-row table, every row observed, recorded in the
  runbook Verify section with the date.
- **Whole** (step 4): the five-row keystroke table on the nested server, every
  row observed, recorded likewise.
- **Live** (step 3, step 6): `tmux list-keys -T prefix g` on the live server
  prints the `display-popup` binding after `source-file`; `cheat.txt` stays 60
  lines and line 7 within 81 cells. The live keypress itself is the user's and
  stays `[未驗證]` in the runbook until they report it.
- **Spec trace**: R1, R3, R5 by step 4; R2, R4, R7 by step 2; R6 by
  `ug -c 'select-pane -T' bin/pane-jump` printing 0; R8 by step 3; R9 by step 6
  and the step 5 diff; R10 by steps 2 and 4 having run on `-L pj` / `-L outer`
  only.
