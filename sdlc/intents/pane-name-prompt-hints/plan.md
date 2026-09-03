# Plan: pane name prompt hints

Author: claude (agent)
Created: 2026-09-03
Status: approved
Approved: 2026-09-03

Implements [`spec.md`](spec.md) as amended 2026-09-03 (C++ tool, own TUI, no
fzf). Requirement numbers R1 to R10 are the spec's. Redrafted 2026-09-03; the
earlier draft (commit `326221c`) planned a bash script and is superseded whole.
Facts with a date were probed then on tmux 3.5a; anything else is `[未驗證]`.

## Files that change

| file | change | requirement |
|---|---|---|
| `tools/pane-jump/src/main.cpp` | new, ~550 lines, C++20, one translation unit | R1 to R6 |
| `tools/pane-jump/CMakeLists.txt` | new, copy of `tools/sift/CMakeLists.txt` with `sift` → `pane-jump` | build |
| `tools/pane-jump/CMakePresets.json` | new, copy of sift's with `binaryDir` `../target/cmake-build-pane-jump` and `../target/cmake-asan-pane-jump` | build |
| `tools/pane-jump/verify-pane-jump.sh` | new headless harness | R10 |
| `claude.conf` | guarded `prefix g` block after the `sift` block (line 213); header comment lines 10-13 name the second cmake tool | R7, R8 |
| `tmux.conf` lines 101-110 | removed; two-line pointer comment | R8 |
| `cheat.txt` line 7 | text after `prefix g`; one line, at most 81 cells (today 77) | R9 |
| `README.md` line 7 | entry text | R9 |
| `runbooks/switch-pane-by-name.md` | shipped recipe, Install, Verify, Troubleshoot, Roll back | R9, R10 |
| `tools/ARCHITECTURE.org` | line 62 sentence, Crates table row after line 92, Key bindings row after line 133 | R9 |
| `tools/atlas/pane-jump.org`, `tools/atlas/index.org` | new node; index entry under Subsystems; Coverage `8 nodes over 16` → `9 nodes over 17`, and the "one non-Rust tool" phrase | R9 |
| `CLAUDE.md` (this repo) | "eleven Rust crates plus `sift`" → "plus `sift` and `pane-jump`, both C++" | R9 |
| `docs/adr/0008-own-the-interaction-loop-for-pane-selection.org` | new, MADR sections as 0005 | R9 |

Untouched: `tools/sift/`, `tools/tmuxlib/`, every Rust crate, `plugins/`,
`.gitignore`, `tools/.gitignore`. Outside the repo, not in this change: the parent
`~/CLAUDE.md` tmux build snippet, spec concern 2.

## Order of work

Each step names its check. A step with no executable oracle names what the reviewer
reads. Branch `intent/pane-name-prompt-hints`; one commit per step, subjects in
the repo's `<area>: <verb> <thing>` style (`tools: add pane-jump …`,
`tmux: bind prefix g to pane-jump`, `docs: …`).

1. **Scaffold the cmake project.** Copy `tools/sift/CMakeLists.txt` and
   `CMakePresets.json` into `tools/pane-jump/`, rename target and option
   (`SIFT_SANITIZE` → `PANE_JUMP_SANITIZE`, output name `pane-jump` /
   `pane-jump-asan`), change both `binaryDir` values so the two projects never
   share a cache, keep `RUNTIME_OUTPUT_DIRECTORY` at `../target/release`. Rewrite
   the header comment for this tool. Write a `src/main.cpp` holding only `main`
   returning 0.

   Check: from `~/.tmux/tools`, `cmake -S pane-jump -B target/cmake-build-pane-jump
   -DCMAKE_BUILD_TYPE=Release && cmake --build target/cmake-build-pane-jump` exits
   0 and `target/release/pane-jump` exists; `cmake --preset release` from
   `tools/pane-jump` does the same when ninja is present. `target/release/sift`
   is unchanged (`stat -c %Y` before and after).

2. **Write `src/main.cpp`.** Layout, top to bottom, mirroring `sift` so a reader of
   one can read the other:

   - Header comment: usage (`pane-jump`, `pane-jump rows [fragment]`, `pane-jump
     jump <pane_id>`), the four inherited invariants (exit 0; no format context
     for titles; never write titles; tmux via `execvp` argv), the note that the
     terminal layer is copied from `sift/src/main.cpp` and where.
   - Copied verbatim from `sift`: `Fd`, `write_all`, `tmux_out`, `tmux_run`,
     `say` (`display-message -l`), the UTF-8 decoder and `utf8_cells` with their
     `static_assert` tests, `RawMode`, `TermSize` / `term_size`, `Key` / `Input` /
     `read_byte` / `read_key` with the `AltDigit` case dropped, the escape
     constants, `on_winch`.
   - New:

     ```cpp
     struct Row { std::string id, where, cmd, title; };
     std::vector<Row> list_rows();               // list-panes -a -F, split on '\t'
     std::u32string fold(std::string_view);      // decode + towlower per code point
     bool title_matches(const Row&, const std::u32string& needle);  // substring
     struct Ui { std::vector<Row> rows; std::string query; std::vector<size_t> shown;
                 size_t sel = 0, top = 0; std::string notice; };
     void refilter(Ui&);                         // shown = matching indices; sel = top = 0
     void draw(const Ui&);                       // header, rows, invert sel, cut at cells
     bool jump(const std::string& id);           // empty → say + false; else switch-client
     int run_ui();  int run_rows(const char* fragment);  int run_jump(const char* id);
     ```

   - `run_ui`: `list_rows()`; empty → `say("pane-jump: no panes")`, return 0.
     `RawMode`; engage fails → `say("pane-jump: no terminal (run it from a tmux
     popup)")`, return 0. Loop on `read_key`: `Text` / `Backspace` / `KillWord` /
     `KillLine` edit `query` and `refilter`; `Up` / `Down` / `PgUp` / `PgDn` /
     `Home` / `End` move `sel` within `shown`; `Enter` with `shown` empty sets
     `notice = "pane-jump: no pane title matches"` and redraws (R4, popup stays
     open); `Enter` with a selection does `term.restore()` then `jump(id)`, and
     returns 0 either way; `Esc` returns 0 with nothing said (R5). Each `draw`
     shows `notice` on the header's right and clears it on the next key.
   - `run_rows`: same rows, filtered when a fragment is given, one buffered
     `write_all` of `id\twhere\tcmd\ttitle\n` lines. `run_jump`: `jump(argv[2])`.
     Both return 0.
   - `main`: `setlocale(LC_ALL, "")`, dispatch on `argv[1]`.

   Row shape is read once at startup. The width column budget in `draw`:
   `where` left-aligned to the widest `where` plus one, `cmd` to 10 cells, the
   rest for `title`; a title cut at a cell boundary so a wide glyph never
   straddles the edge (`utf8_cells`).

   Check: the build from step 1 with `-Wall -Wextra -Wpedantic -Wshadow
   -Wconversion -Wsign-conversion -Wold-style-cast` (inherited from the copied
   CMakeLists) compiles with zero warnings; `ug -c 'display-message' src/main.cpp`
   equals `ug -c 'display-message", "-l"' src/main.cpp`; `ug -c -e 'system(' -e
   'popen(' -e 'select-pane' -e 'rename-window' src/main.cpp` prints 0 (R6, format
   safety). On the live server, read only: `target/release/pane-jump rows | wc -l`
   equals `tmux list-panes -a | wc -l`, and `pane-jump rows claude | cut -f4` are
   all titles containing `claude` case-insensitively.

3. **Write and run `verify-pane-jump.sh`** (R1 to R4 headless, R10). Same skeleton
   as `records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-jump.sh`:
   `set -u`, `ok`/`bad`/`check`, throwaway server `-L pj_verify`, `trap` kill,
   `TMUX="$SOCK,0,0"` so bare `tmux` inside the binary reaches it, and the same
   control case first (the harness targets the throwaway server, not the user's).
   Because `switch-client` needs an attached client, a second server `-L
   pj_outer` runs one pane holding `tmux -L pj_verify attach`.

   Fixture: session `t`, one window, three panes titled `alpha-build`,
   `beta-server`, `gamma-notes` via `select-pane -T` (a throwaway server, so the
   never-write-titles rule does not apply).

   | assertion | want |
   |---|---|
   | control: `TMUX=… tmux display -p '#{pane_id}'` equals `-L pj_verify`'s | equal |
   | `pane-jump rows \| wc -l` | 3 |
   | `pane-jump rows \| cut -f4` | `alpha-build`, `beta-server`, `gamma-notes` in that order |
   | `pane-jump rows BETA \| cut -f4` | `beta-server` only (case-insensitive, R2) |
   | `pane-jump rows bash \| wc -l` | 0 (command column not matched, R2) |
   | `pane-jump rows zzz \| wc -l` | 0 |
   | `pane-jump jump %1; tmux -L pj_verify display -p '#{pane_id}'` | `%1`, exit 0 |
   | `pane-jump jump ''` stderr | contains `no pane selected`; active pane still `%1`; exit 0 |
   | `pane-jump jump` (no arg) | same as empty |

   Check: the script exits 0 with every line `ok`. A `FAIL` stops the build and is
   reported verbatim.

4. **Prove the TUI end to end on the throwaway servers** (R1, R3, R4, R5). Keys go
   to the `pj_outer` pane, whose terminal is the `pj_verify` client; `send-keys`
   into a `pj_verify` pane would bypass the key table (runbook testing note).
   Bind the key on `pj_verify` by hand with the exact `display-popup` line from
   step 5, then:

   | keys to `pj_outer` | then read | expected |
   |---|---|---|
   | `C-b g`, `gamma`, `Enter` | `tmux -L pj_verify display -p '#{pane_id}'` | `%2` |
   | `C-b g`, `alpha`, `Enter` | same | `%0` |
   | `C-b g`, `Enter` (no typing) | same | `%0`, the first row |
   | `C-b g`, `zzz`, `Enter` | `tmux -L pj_verify display -p '#{popup_active}'`? no: capture the outer pane | popup still open, header shows `no pane title matches`; then `Escape` closes it, pane unchanged |
   | `C-b g`, `beta`, `Escape` | pane id, `show-messages` | unchanged; no new message |

   Sleep 0.3 s between key groups. Record observed values for the runbook.
   Check: all five rows observed. Add these five as a second block of the harness
   if they can be driven from the script (they can, with `send-keys -t pj_outer`
   and `capture-pane -p -t pj_outer`); otherwise record them as run by hand,
   dated, in the runbook.

5. **Bind the key** (R7, R8). In `claude.conf`, after the `sift` block's closing
   `}` at line 213, a comment block in the file's style (why a popup and a
   program: tmux has no as-you-type pane filter; supersedes the 2026-09-03
   `command-prompt` + `choose-tree` binding, which opened on the session row and
   showed the full tree on a miss; pointer to ADR-0008 and the runbook), then:

   ```tmux
   if-shell '[ -x ~/.tmux/tools/target/release/pane-jump ]' {
     bind -T prefix g display-popup -E -w 80% -h 60% -T ' pane name ' \
       "~/.tmux/tools/target/release/pane-jump"
   } {
     bind -T prefix g display-message -l 'pane-jump: not built — cd ~/.tmux/tools && cmake -S pane-jump -B target/cmake-build-pane-jump -DCMAKE_BUILD_TYPE=Release && cmake --build target/cmake-build-pane-jump && tmux source-file ~/.tmux.conf'
   }
   ```

   Update the header comment at `claude.conf:10-13` to list both cmake builds.
   In `tmux.conf`, delete lines 101-110 and leave:

   ```tmux
   # prefix + g : jump to a pane by TITLE — bound in claude.conf (pane-jump,
   # tools/). Was an inline command-prompt + choose-tree here until 2026-09-03.
   ```

   Check: `tmux source-file ~/.tmux.conf` on the live server, then `tmux
   list-keys -T prefix g` prints the `display-popup` form. This reload is the one
   live-server write in the plan and is the reload the user runs after any config
   edit. Then move the binary aside, re-source, confirm `list-keys` prints the
   `display-message` stub, move it back, re-source.

6. **Docs** (R9). No executable oracle; the reviewer reads each against the
   source.
   - `runbooks/switch-pane-by-name.md`: "The short answer" table row for `prefix
     g` → `pane-jump` (C++ popup over `#{pane_title}`); "The recipe" gains
     "Option C — pane-jump popup — SHIPPED" first, with the binding, the three
     entry points, the key list; Option A retitled "filtered tree (no binary) —
     fallback", `SHIPPED` removed; Option B says its jump half is now
     `pane-jump jump`. New "Install" section with both cmake spellings, copied
     from `runbooks/sift.md:79`. "Verify": add the harness invocation and the
     step 3 and step 4 tables with observed values, dated; keep the existing
     table for the fallback; keep "the bound key on the real server" under
     `[未驗證]`. "Troubleshoot": add `not built` stub, `no pane title matches`,
     `no terminal (run it from a tmux popup)`; the `Down Down` entry moves under
     a fallback note. "Roll back": paste the Option A binding into `tmux.conf`
     and `source-file`, or `unbind g`.
   - `cheat.txt:7`: `               prefix g        jump to pane by title: list, type, Enter`.
   - `README.md:7`: ``- [switch-pane-by-name](runbooks/switch-pane-by-name.md) — `prefix g`: pane-jump popup over `#{pane_title}` — type part of a pane's title, Enter jumps``.
   - `tools/ARCHITECTURE.org`: line 62 "except `sift`" → "except `sift` and
     `pane-jump`"; Crates table row `| =pane-jump= | popup TUI / =rows= / =jump=
     seams | ADR-0008 · runbooks/switch-pane-by-name.md |`; Key bindings row
     `| =prefix g= | popup list of every pane title; type to filter, Enter jumps
     | pane-jump |`.
   - `tools/atlas/pane-jump.org`: Summary, Crux (rows → fold → substring →
     `switch-client`; why the popup stays open on a miss), Links (`sift` for the
     copied terminal layer, `attention-navigation` for the fzf sibling
     `cc-fleet`), Sources (`pane-jump/src/main.cpp`), Notes. `index.org`: entry
     under Subsystems, Coverage counts, the "one non-Rust tool" phrase → two.
   - `CLAUDE.md`: the crate-count sentence.
   - `docs/adr/0008-own-the-interaction-loop-for-pane-selection.org`: Context
     (tmux has no as-you-type pane filter; `choose-tree -f` ignores an empty
     filter), Decision Drivers, Options (keep `choose-tree`; popup + fzf script;
     popup + own TUI), Outcome (own TUI, citing 0005 as precedent and the
     user's directive), Consequences (copied terminal layer, second cmake
     project).

   Check: `wc -l cheat.txt` prints 60; `sed -n 7p cheat.txt | wc -m` at most 82;
   every org link in the new atlas node and ADR resolves (`ug -o '\[\[file:[^]]*'`
   and `test -e` each); the reviewer confirms no doc line claims the live key was
   pressed.

7. **Review and merge.** `/code-review` against `main` per the `implement` body;
   findings addressed or answered in the merge commit. Merge to `main`; no PR
   flow, so the close is `sdlc close pane-name-prompt-hints --merge <sha>`.

## Risks

- **Most likely break: `switch-client` from inside the popup after
  `term.restore()`.** `sift` never switches clients; `cc-fleet` does, from Rust,
  without restoring a raw terminal first. If the move fails the symptom is exit 0
  and no move; step 3's `jump %1` row and step 4's first row are the oracle. Fix:
  `switch-client -c "$(display -p '#{client_name}')" -t <id>`, the
  `focus_pane(Some(client), …)` form.
- **Copied terminal code drifts from `sift`.** A fix landing in one file and not
  the other. Mitigation: the header comment names the source lines; spec concern
  1 names the shared-header follow-up.
- **Case folding of non-ASCII titles.** `towlower` under the user's locale is not
  tmux's `mi`. ASCII is exact; Chinese titles have no case, so folding is the
  identity. Marked `[未驗證]` for other scripts in the runbook.
- **A title containing a tab.** Shifts fields; no writer in this repo emits one.
  Accepted, noted in the runbook.
- **`cargo clean` deletes the binary.** Inherited from `sift`; the stub binding
  says how to rebuild.
- **Two cmake projects, one `target/`.** Separate `binaryDir`s prevent cache
  collision; the `release/` output dir is shared by design. A future
  `tools/CMakeLists.txt` could unify them; not now.
- **Popup size on a narrow client.** 80 % × 60 % is a percentage; the TUI reflows
  on `SIGWINCH`, so percentages are fine.
- **Runbook rewrite scope.** Step 6 edits six sections of a 300-line file; the
  reviewer diff is the control. Spec concern 5 accepted this.
- **What was not chosen.** The bash + fzf script (superseded by directive), a
  Rust crate on `tmuxlib` (the user asked for C++), a shared C++ header (touches
  `sift`), fuzzy matching (spec concern 3), auto-accept on a unique match,
  ordinal mode, a preview pane.

## Proof

- **Static**: the release build compiles with zero warnings under the inherited
  flag set; the three `ug` counts in step 2 (all `display-message` carry `-l`; no
  `system` / `popen` / `select-pane` / `rename-window`); `git diff --stat main`
  lists exactly the paths in Files that change, plus `plan.md` if amended.
- **Headless** (step 3): `verify-pane-jump.sh` exits 0, nine assertions `ok`,
  output pasted into the runbook Verify section with the date.
- **Whole** (step 4): the five keystroke rows on the nested servers, observed
  values recorded likewise.
- **Live** (step 5, step 6): `tmux list-keys -T prefix g` prints the popup form
  after `source-file`, and the stub form with the binary moved aside;
  `cheat.txt` stays 60 lines, line 7 within 81 cells. The live keypress is the
  user's and stays `[未驗證]` until they report it.
- **Sanitizer**: `cmake --preset asan && cmake --build --preset asan` from
  `tools/pane-jump`, then `PANE_JUMP=…/pane-jump-asan bash verify-pane-jump.sh`
  exits 0, the same discipline `runbooks/sift.md:136` records.
- **Spec trace**: R1, R3, R5 by step 4; R2, R4 by step 3 and step 4; R6 by the
  step 2 counts; R7 by the stub check in step 5; R8 by step 5; R9 by step 6; R10
  by steps 3 and 4 having run on `-L pj_verify` / `-L pj_outer` only.

## Departures during build

Recorded per the Build play, step 7: each departure names the commit it landed in.

- `3d97246` — `read_key` decodes `ESC [ 1 ~` / `ESC [ 4 ~` (Home / End) and
  consumes the trailing `~` of numeric CSI sequences, wider than sift's copy,
  because sift's version has a documented `~` leak and the spec lists Home / End
  as working keys. sift itself is untouched.
- `3d97246` — the popup has a one-line key-hint footer; the spec described the
  header and rows only. Cost: one list row.
- `3d97246` — `draw` takes `Ui&`, not `const Ui&`, because it keeps the scroll
  offset in view, as sift's does.
- `3d97246` — `jump("")` reports on stderr and through `display-message -l`; the
  spec table said stderr, the plan's signature comment said `say`.
- `3d97246` — the header comment does not spell out the two forbidden title-writing
  commands, so the step 2 `ug` count is 0 on code and comments alike.
- `b26f8cf` — the R5 silent-cancel assertion counts ` message: ` lines from
  `show-messages`, with a positive control, because on 3.5a `show-messages` logs
  every server command and a line count is not an oracle. The step 4 keystroke
  rows live in the harness as its second block, each starting from a pane that
  differs from its expected landing.
- docs commit — atlas Coverage reads `9 nodes over 19 source files`, not `17`:
  the existing drawers hold 17 unique paths of which one is a doc, and the new node
  lists three sources. The atlas Links section carries one edge to a non-node
  file (ADR-0008); the node format writes node-to-node edges, so this is a format
  extension to accept or move to prose.
- docs commit — Option A's opening sentence and the "was free" sentence in the
  runbook changed, because the `tmux.conf` binding they described no longer exists.
- `3d97246` — R4's message is rendered in the popup's own header (the `notice`
  field), not through `display-message -l` as the spec's R4 read. That is what
  step 2 of this plan specified; the spec has since been amended to match.
- `3d97246` — an unknown verb prints
  `usage: pane-jump | pane-jump rows [fragment] | pane-jump jump <pane-id>` to
  stderr and exits 0; the plan's `main` line said only "dispatch on `argv[1]`".
- `3d97246` — a server with no panes at all says `pane-jump: no panes` and
  returns 0. Planned in step 2's `run_ui` sketch, not in the spec's R-list.
- review commit — silent-failure fix: both callers of `jump()` now read its
  `bool`; a failed `switch-client` is reported (`run_ui` on the status line,
  `run_jump` on stderr and the status line), still exit 0.
- review commit — the source header's provenance list now records that
  `chain()` / `StringLike` and `parse_fields` (sift lines 202-231) sit inside
  the named 158-237 range but were *not* copied.
- review commit — the figurative comment above the escape constants is replaced
  by a literal statement of the same rule.
- review commit — the header-plus-footer row budget is one `kChromeRows`
  constant; `draw()` and the PgUp/PgDn step both derive from it.
- review commit — `verify-pane-jump.sh` gains a `from_pane_press` helper for the
  repeated "select a start pane, open the popup, send key groups" shape; still
  28 assertions, 28 passing.
- review commit — the atlas node's Summary is cut to six sentences, the surplus
  moved to its `Notes`; its `implements` edge to ADR-0008 is dropped and the ADR
  cited in prose instead, because atlas edges are node-to-node.
- review commit — `tools/atlas/claude-state-protocol.org` is re-verified against
  the edited `tools/ARCHITECTURE.org` (prose still true) and its Sources hash
  updated; `tools/atlas/sift.org` no longer calls sift the only non-Rust tool;
  `index.org` Coverage states the counting convention behind "19 source files".
- review commit — ADR-0008's "roughly 250 lines" becomes the measured 312, with
  the counting command; the runbook's Install section prints the actual sha256
  the two build spellings agree on, with the date and both commands.
- review commit — the runbook's `[未驗證]` list no longer claims the binding step
  read `list-keys` on the live server. The live server has not been re-sourced
  (its `prefix g` still holds the old `choose-tree` form, read 2026-09-03); the
  binding is verified only on throwaway servers, both branches of the guard.
- Not done, by design: the live keypress. It stays `[未驗證]` in the runbook.
