You are verifying a **deliberately scoped two-bug fix**. This is a regression and scope
audit, not a re-audit of the whole program — two prior adversarial passes already cleared
it (0 blocking each).

You are **read-only**. Do not edit or fix anything.

## What changed and why

`/home/fenrir/.tmux/tools/sift/src/main.rs` is a Rust port of a C++ tmux search tool. It
was ported **bug-for-bug faithful** so equivalence could be measured with no carve-outs,
and that measurement passed. A fix node was then allowed to break equivalence on exactly
**two** paths:

1. **Home/End** — the CSI decoder now accepts `ESC[1~`/`ESC[7~` as Home and
   `ESC[4~`/`ESC[8~` as End, so the trailing `~` no longer leaks into the pattern.
2. **SIGWINCH** — `read_byte` now returns a distinct `READ_INTR` (−2) on `EINTR` instead
   of conflating it with error/timeout; a new `read_byte_seq` retries `EINTR` inside an
   escape sequence; `read_key` maps a `READ_INTR` first byte to `Key::None`; the
   previously-dead `G_RESIZED` branch now redraws and the loop continues.

Everything else was to remain untouched, including **ten known latent findings** that are
queued for a separate waiver decision: L1 lossy `regerror` text, L2 raw-byte `text` field,
L3 SIGPIPE-on-stdout (cpp 141 / rust 0), L4 `eprintln!` + `panic=abort` → SIGABRT,
L5 `say` byte transparency (already resolved earlier), L6 `strtol` negative-extreme
saturation, T1 the added panic hook, T2 `out()` swallowing EPIPE, T3 no 32-byte line-number
truncation, T4 `Option`-guarded out-of-range `hit.line`.

## Already established by the orchestrator — do not re-run these

- Home/End fixed, measured on a pty with the C++ binary as a control that **still leaks**
  the `~`; PgUp/PgDn intact (`ESC[5~` → sel 102→75, `ESC[6~` → back to 102).
- Resize fixed: Rust survives and redraws at height 40; no-resize control alive but draws
  nothing; C++ control still dies.
- `verify-sift-jump.sh` 13/0, `verify-sift-live.sh` 6/0, `rows` output still byte-identical
  to the C++ golden.
- Zero build warnings; the live `tools/target/release/sift` is still the C++ binary.

**Important**: all of the Home/End and resize measurements above were taken with
`probe-sift-keys.py` — a probe **written by the node that wrote the fix**. Its control
(the C++ binary failing in the same harness) shows the probe *can* report failure, which
is why the result is credible. But you must not simply re-run it. **Build your own
instrument**, however small, for at least one of the two behaviours, and say which.

## Your job — four questions

1. **Scope.** Read the current `main.rs` around the changed regions (roughly lines
   850-1000 and the `run_ui` loop) and confirm the change set is what was authorised and
   nothing more. Specifically: was any of the ten latent findings silently "fixed"? Was
   anything refactored, renamed, or restructured beyond the two fixes? Were the
   `ByteOff`/`CharOff`/`CellOff` newtypes and the `SAFETY:` comment discipline preserved?
2. **Correctness of the EINTR change.** `READ_INTR` is a new third return value from a
   function whose callers previously handled only "byte" and "−1". Enumerate **every**
   caller and confirm each handles the new value correctly. Pay attention to whether a
   bare `Escape` keypress still cancels (it must — live harness test 4 depends on it),
   and whether `READ_INTR` can leak into the pattern buffer or cause a spin loop under a
   repeating signal.
3. **New failure modes the fix could introduce.** Can the resize path now loop without
   progress? Can a resize during an escape sequence corrupt the decode? Does the redraw
   handle a terminal that shrank to fewer rows than the current selection, or to 1 row,
   or to 0 columns? Does it re-read the terminal size, and does it leave the
   alternate-screen state consistent?
4. **Comment honesty.** The old comments asserted both bugs were deliberate and must
   stay. Confirm none of them still says that, and that the new comments describe what
   the code now does.

Throwaway `-L` tmux sockets only, and **`unset TMUX`** in any command running a tmux
client — a previous node leaked read-only probes to the user's live server by inheriting
it. Budget: **at most 5 runs.**

## Severity

- `biases-deliverable` — a user would observe wrong behaviour, or the code can panic,
  spin, or hang on reachable input. Blocks the swap.
- `latent` — real but not user-observable on a reachable path.

Finding nothing blocking is a legitimate verdict. Say so plainly if that is the answer.

## Output

Write findings to `/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/nodes/verify-fixes/findings.md`.

```result
status: ok | failed
findings_path: <path or ->
blocking: <count>
latent: <count>
blocking_list: <none | "file:line — summary", semicolon-separated>
scope_clean: <yes|no — was anything beyond the two fixes changed, and were any of the ten latents silently fixed>
read_intr_callers: <how many callers, all handled correctly? which one is riskiest>
escape_still_cancels: <yes|no — how you established it>
own_instrument: <what you built and which behaviour it measured>
new_failure_modes: <none | list>
comments_honest: <yes|no>
notes: <one line>
```

On failure write NO artifact — report it in the result block only. Return a terminal
result: do not background any self-check and do not end your turn waiting on anything.
