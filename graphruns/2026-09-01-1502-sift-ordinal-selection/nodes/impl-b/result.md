# impl-b — returned & verified

files: [tools/sift/src/main.cpp] (+129 / -9)
buildClean true, warnings 0, jump 13/0, live 6/0.
footerText: `↑↓ select  left-Alt-digit goto  Enter jump  Esc cancel  C-w word  C-u clear`

keyContract: Key::AltDigit added to read_key()'s ESC branch (c1 in '0'..'9'), 40 ms window
untouched. Ui::goto_buf is the single mode field — non-empty IS the mode. Helpers
in_ordinal_mode / ordinal_of / push_ordinal_digit / sync_ordinal. push_ordinal_digit carries
one bounds check that implements the out-of-range refusal, the Alt-0 refusal, the empty-list
refusal and the invalid-pattern refusal. draw() picks the live prompt/typed text and
recomputes plen.

## Three decisions beyond ADR-0007's literal text (routed to map-close + G1)

1. **Invariant extended to every mover.** ADR-0007 names only the arrows as rewriting the
   buffer; its stated invariant ("goto> always names where the cursor actually is") only holds
   if every mover does, so sync_ordinal() is called from Up, Down, Home, End and PgUp/PgDn.
2. **C-w / C-u clear the buffer before touching the pattern** — chosen over ignoring them, so
   the load-bearing property "no exit from the mode happens after the pattern changes" stays
   true (both call refilter()).
3. **Alt-<digit> while already in the mode extends the buffer**, exactly as a bare digit does.

## Orchestrator verification (against on-disk artifacts, not the self-report)

- `git status --porcelain` → only ` M tools/sift/src/main.cpp`.
- `cmake --build --preset release --clean-first` → rc 0, **0 warnings** (independent re-run).
- `verify-sift-jump.sh` → passed 13, failed 0; `verify-sift-live.sh` → passed 6, failed 0.
- Source read at the key sites: AltDigit enumerator (571/620), goto_buf (724), the four
  helpers (764-796), live prompt selection (826-827), footer (871-873), Esc-in-mode (904),
  movers (918-928), AltDigit case (932-935), Backspace pop (943-945), C-w/C-u clears
  (958/968), Text fallthrough (974-981).

routeAfterBuild(...) → **proceed**.

## Pre-existing defect discovered, INDEPENDENTLY VERIFIED by the orchestrator

`read_key()`'s CSI switch decodes Home/End only as `ESC[H`/`ESC[F`; tmux `send-keys Home`/`End`
emit `ESC[1~`/`ESC[4~` (measured this run with `cat -v` on a throwaway server), which hit the
`default:` arm and return Key::None **without consuming the trailing `~`** — the `~` then lands
in the pattern as printable text. That switch is unchanged by this effort. Not introduced, not
fixed, out of scope. Routed to: verify-impl's prompt (as a scope boundary, so it does not burn
a repair round), tests' prompt (so it does not write an assertion that cannot pass), map-close
(to be recorded), and G1's payload.

---

# impl-b attempt 2 (repair) — returned & verified

Fix: a new pure helper `utf8_fit(std::string_view, int budget) -> size_t` (main.cpp:314), the
forward inverse of the existing `utf8_cells`, built from the same `utf8_decode` + `cell_width`
pair and always landing on a character boundary. `draw()` now emits
`kFooter.substr(0, utf8_fit(kFooter, w))` wrapped in kDim/kReset (main.cpp:902) — the escapes
are zero-width, so they are neither charged against the budget nor cut. The footer TEXT is
unchanged (75 cells), so ADR-0007's "left Alt" wording and every doc quoting the footer
verbatim still match the binary. Nothing else touched; `main.cpp` total now +157/-9.

## Orchestrator verification — independent, and shown capable of failing

1. **Width sweep** (`width-sweep.sh`, orchestrator-written): at w = 20, 30, 40, 60, 74, 75,
   100, 265 screen line 1 matches `^goto> ` after `M-1` and `goto>` appears on screen.
   **8 passed, 0 failed.**
2. **Control for the sweep** — an unguarded-but-featured binary built by the orchestrator in
   the scratchpad, differing from the shipped source *only* in the reverted guard line:
   **3 passed, 5 FAILED** (w = 20, 30, 40, 60, 74 lose the header; 75/100/265 pass). The
   boundary is exactly the footer's 75 cells, as predicted. The sweep is therefore shown
   capable of both failing and passing — it is a gate, not an advisory.
3. **Feature probe** (`feature-probe.sh`, orchestrator-written): the seven settled behaviours
   plus the ordinal-column render and the TARGET-pane jump, run at **w=100 and w=74**:
   **13 passed / 0 failed at each.** N is measured from `sift rows`, never assumed — the
   first draft assumed N=12 and the seam reported 14, which is why it is derived now.
4. `cmake --build --preset release --clean-first` → **0 warnings**; jump **13/0**; live **6/0**.

## Scope of the claim (降級執行就要降級宣稱)

The 25-item **agent** verifier ran against the pre-repair source. The repair itself was
verified by orchestrator transform — the mechanical property that failed, with a control
proving the check has teeth, plus the seven behaviours at both widths and both suites. The
independent agent verifier was NOT re-run on the final source. The behaviours are about to be
pinned adversarially by the `tests` node against a negative control, which is the durable
coverage; this probe is the interim gate. Stated plainly rather than reported as a second
full verification pass.
