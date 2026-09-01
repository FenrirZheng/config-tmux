# verify-impl — returned & verified. Verdict: REPAIR

buildWarnings 0 · jump 13/0 · live 6/0 · 25-item ADR-0007 checklist walked with pasted
capture evidence per item.

- **23 implemented**, 1 `out-of-scope` (Home/End — unreachable key, pre-existing decode gap),
  **2 unmet**.
- `unmet` = ["the goto> prompt is never invisible", "the runbook says left Alt"].
  The second is not an impl defect — it is the `docs` node, which has not run yet. Only the
  first is routed to repair.
- **1 blocking finding** (`biases-deliverable`), 4 latent.

## The blocking finding — INDEPENDENTLY REPRODUCED by the orchestrator

`main.cpp:873`. The footer literal grew 54 → 75 cells ("left-Alt-digit goto" adds 21).
`draw()` emits it unconditionally with **no width guard**, so at any sift width ≤ 74 it wraps
onto a second terminal line. The screen then holds header(1) + rows(h-2) + footer(2) = h+1
lines, the pane scrolls by one, and the header — carrying the `goto>` prompt AND the match
count — goes off the top. The cursor park `\x1b[1;{}H` then lands inside a result row.

Orchestrator reproduction at w=74, new binary, after `M-1`:
```
>  1  0 for i in $(seq -w 1 12); do echo "row$i ZAP$i tail"; done   <- line 1 is a RESULT row
   2  1 …1 12); do echo "row$i ZAP$i tail"
capture-pane | grep -c 'goto>'  ->  0
last lines: "↑↓ select  left-Alt-digit goto  Enter jump  Esc cancel  C-w word  C-u clea" / "r"
```
**It is a regression, not merely a pre-existing hazard**: the old 54-cell footer occupied one
line at w=60 and w=74 (verifier measured cursor_y=0 for OLD, cursor_y=1 for NEW at both).
`claude.conf:207` sizes the popup at `-w 95%`, so any tmux client narrower than ~81 columns
loses the prompt and the match count. The user's own 282-col client does not.

This is precisely the Goodhart case §11 predicted: Lens A passed 25/25 at 100 columns while
the feature was invisible at 74. Verifier's own words: "the failure is a rendering side effect
that no keystroke-and-state check would ever see."

## Latent (do not block; routed to map-close + G1)

1. Home/End decode gap — pre-existing, out of scope (orchestrator-verified separately).
2. `runbooks/sift.md` has no ordinal content — the `docs` node's pending work, not a defect.
3. Leaving the mode does not restore the pre-entry selection. The ADR does not speak to it and
   an argument exists that it is correct. Design question, not a contradiction.
4. The selected row's ordinal inherits the row marker's bold (bold-cyan vs plain cyan).
   Cosmetic; does not harm distinguishability.

## Notes carried forward

- **For `tests`**: every ordinal assertion written at 100 cols passes while the feature is
  invisible — include a **narrow-width (74x20)** case. Assert `capture-pane -p | sed -n 1p`
  matches `^goto> `, not a grep over the whole screen.
- **For `docs`**: runbook still has no ordinal section; CONTEXT.org already carries both
  glossary entries (Match ordinal / Ordinal mode).
- **For `atlas`**: row layout is now `"> "(2) + ordinal(ordw) + 1 + line#(numw) + 1 + text`,
  `textw = w - numw - ordw - 4`; `read_key()` returns `Key::AltDigit` for ESC + '0'..'9'.
