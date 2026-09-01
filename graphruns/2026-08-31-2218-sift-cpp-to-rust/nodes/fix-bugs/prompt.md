Fix exactly two bugs in a Rust program. Equivalence with the original C++ has already
been proven; you are the node that deliberately breaks it, on two named paths and
nowhere else.

## Background you need

`/home/fenrir/.tmux/tools/sift/src/main.rs` (1519 lines) is a bug-for-bug Rust port of a
C++ tmux search tool. It was ported faithfully **on purpose** so that equivalence could be
measured against the C++ baseline with no carve-outs. That measurement is now done:
`verify-sift-jump.sh` 13/0, `verify-sift-live.sh` 6/0, 26 regex patterns byte-identical,
26 differential rendering comparisons byte-identical, 0 blocking findings from two
adversarial audits.

The user then chose to fix these two, and only these two.

## Bug 1 — `Home` / `End` type a literal `~` into the pattern

`main.rs:943-950`. The escape decoder accepts only `ESC[H` / `ESC OH` / `ESC[F` /
`ESC OF`. tmux sends `ESC[1~` for Home and `ESC[4~` for End, so the sequence is not
recognised and the trailing `~` is appended to the search pattern.

Measured symptom, from both binaries at raw-byte level: pattern `foo`, press Home then
End → header reads `regex> foo~~` and `no match`.

The runbook's key table has always promised these work (`Home` / `End` → first / last
match). The intended behaviour already exists at `main.rs:1375-1376` (`Key::Home =>
u.sel = 0`, and the `Key::End` arm) — it is simply unreachable. Make it reachable.

Note the decoder already handles `~`-terminated CSI sequences for `PgUp`/`PgDn`
(`ESC[5~` / `ESC[6~`), which do work — so there is an existing shape to extend, not a
new mechanism to invent. Some terminals send `ESC[7~` / `ESC[8~` for Home/End; decide
whether to accept those too and justify the choice. Do not break `PgUp`/`PgDn`.

## Bug 2 — resizing the popup cancels the search

`main.rs:855, 1328-1334`. `read_byte` treats every `poll`/`read` failure as `-1`,
**including `EINTR`**. A SIGWINCH while sift is blocked therefore surfaces as `-1` →
`read_key` reports `Esc` → the cancel path fires and sift exits. Resizing the popup
silently ends the search.

Consequence: the `G_RESIZED` redraw branch at `main.rs:1334` is **dead code** — it was
kept deliberately so you would have something to make live.

Measured symptom: a `TIOCSWINSZ` on a pty (with `TIOCSCTTY`) kills the process; the
control with no resize leaves it alive and rendering.

Fix: distinguish `EINTR` from a real error so the poll loop restarts, and let the
`G_RESIZED` branch actually redraw at the new terminal size. Mind that `term_size` must
be re-read on resize, and that the redraw must not corrupt the alternate-screen state.

## Scope discipline

- **Only these two.** The verifiers recorded four latent findings (T1 panic-hook
  addition, T2 EPIPE-vs-SIGPIPE, T3 no 32-byte truncation, T4 Option-guarded index) and
  six from the earlier audit (L1-L6). **Do not fix any of them.** They are queued for a
  separate explicit waiver decision.
- Do not refactor, rename, or restructure anything you are not fixing.
- Keep the house style: `unsafe` blocks minimal with a `SAFETY:` invariant comment; the
  `ByteOff`/`CharOff`/`CellOff` newtypes intact; zero `cargo build` warnings.
- Update the *code comments* at both sites — they currently say the bugs are deliberate
  and must stay. Leaving them would make the next reader distrust the code. Do not touch
  any `.md`/`.org` doc; a later node owns those.

## Hard constraints

- **Build only with `CARGO_TARGET_DIR=/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/target-dev`.** `prefix /` runs
  `/home/fenrir/.tmux/tools/target/release/sift`; writing there hands the user an
  unverified binary.
- Never a bare `cargo build --release` in `tools/`; never `cargo clean`.
- Throwaway `-L` tmux sockets **only**. A previous node leaked two read-only probes to
  the user's live server by inheriting `$TMUX` from its shell — explicitly `unset TMUX`
  or set it to your throwaway socket in every command that runs a tmux client.
- Do not touch `src/main.cpp` or `CMakeLists.txt`. Do not commit.

## Prove it — with controls, not assertions

For each bug, produce a **before/after** measurement with a control that would fail
differently. The prior nodes' methods are the standard to meet:

- Home/End: drive the binary on a pty, send the real `ESC[1~`/`ESC[4~` bytes, and read
  the header. Control: the C++ baseline at `/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/baseline/sift-cpp`, which must still
  show the `~` leak. Also confirm `PgUp`/`PgDn` still work.
- Resize: `TIOCSWINSZ` on a pty with `TIOCSCTTY`. Control: a no-resize trial (process
  stays alive) AND the C++ baseline (process still dies). After the fix the Rust binary
  must survive the resize **and** redraw at the new size.

Then confirm no regression — both harnesses must still match the baseline exactly:

```
SIFT=/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/target-dev/release/sift timeout 180 bash verify-sift-jump.sh   # must stay 13 / 0
SIFT=/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/target-dev/release/sift timeout 300 bash verify-sift-live.sh   # must stay 6 / 0
```
(in `/home/fenrir/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/`)

Budget: **at most 6 cargo invocations.**

## Output contract

```result
status: ok | failed
files_written: <comma-separated paths>
build: <pass|fail>
warnings: <count>
home_end_fixed: <yes|no — the measurement, and the C++ control's result>
pgup_pgdn_intact: <yes|no — how checked>
resize_fixed: <yes|no — the measurement, the no-resize control, and the C++ control>
dead_branch_now_live: <yes|no>
jump_passed: <N> / jump_failed: <N>    (must be 13 / 0)
live_passed: <N> / live_failed: <N>    (must be 6 / 0)
other_findings_touched: <must be "none">
live_binary_intact: <yes|no — sha256 vs /home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/baseline/sift-cpp>
notes: <one line; on failure, why>
```

On failure write NO artifact — report it in the result block only. Return a terminal
result: do not background any self-check and do not end your turn waiting on anything.
