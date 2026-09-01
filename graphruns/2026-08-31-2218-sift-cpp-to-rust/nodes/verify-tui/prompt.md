You are an adversarial verifier auditing the **interactive TUI half** of a C++ → Rust
port. You did not write it and you have no stake in it passing. Find where it is wrong,
and say honestly which of those would bite a user.

You are **read-only**. Do not create, edit or delete any file except your one output file.
Do not fix anything.

## Under audit

`/home/fenrir/.tmux/tools/sift/src/main.rs`, lines ~800-1519 — `raw`, `cooked`,
`on_winch`, `term_size`, `read_byte`, `read_key`, `Input`, `out`, `render_line`, `Ui`,
`refilter`, `draw`, `run_ui`, and `main`'s dispatch. The non-interactive half (lines
1-800) was already audited and cleared — do not re-audit it except where the TUI's use of
it changes something.

## References

- **Contract**: `/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/nodes/spec/spec.md` §5 (terminal control), §6 (key map),
  §7 (rendering), §9 (hazards). Measured, not inferred.
- **Original**: `/home/fenrir/.tmux/tools/sift/src/main.cpp`. Authority on conflict.
- **Prior findings**: `/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/nodes/verify-core/findings.md` — read L5 in particular,
  which this node was tasked to resolve.
- **Runnable**: C++ `/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/baseline/sift-cpp`; Rust `/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/target-dev/release/sift`.

## Already established — do not spend budget re-establishing

The orchestrator ran these itself and they passed:

- `verify-sift-live.sh` → 6 passed / 0 failed (C++ baseline: identical).
- `verify-sift-jump.sh` → 13 passed / 0 failed (no regression).
- Zero warnings on a forced rebuild; `jump()` is genuinely reachable (called at line 1352);
  the live `tools/target/release/sift` is still the C++ binary.

## Two known measurement limitations — inherit these, do not rediscover them

1. **Live test 6 does not exercise the port.** It `source-file`s the shipped
   `claude.conf`, whose binding points at `~/.tmux/tools/target/release/sift` — still the
   C++ binary. It passed for both runs because both ran the same C++ binary. Only 5 of
   the live harness's 6 assertions actually drive the Rust code. Do not cite test 6 as
   evidence about the port.
2. The harnesses together cover: header match count, the Enter jump landing, self-exit,
   Esc cancel, and filter latency. **They do not cover** most of the key map, any
   rendering detail beyond the presence of `regex>`, CJK width in the rendered list,
   paging arithmetic, or the terminal-restore paths. That is your territory.

## Audit axes — cover all five

1. **Key map fidelity.** Every sequence in spec §6 against `read_key`: the arrow/`C-p`/
   `C-n` set, `PgUp`/`PgDn`, `Backspace`/`C-w`/`C-u`, `Esc`/`C-g`/`C-c`, and the escape-
   sequence decoder's timeout discipline. Selection/paging arithmetic: initial selection
   (nearest the bottom), clamping at both ends, page size, wrap or no wrap.
2. **Rendering.** Header text in every state against §7 — including the alternate-screen
   warning and the no-match / empty-pattern states — the row format, the highlight span
   (in **bytes**) against the truncation width (in **cells**), and CJK behaviour. Drive
   both binaries and diff `capture-pane -e` output; a measured pixel difference beats an
   opinion.
3. **Terminal control and restore.** The termios flags set/cleared versus §5. Then the
   claim you must actually re-test: the port adds a **panic hook chained to `atexit`**
   because `[profile.release]` sets `panic = "abort"` and `Drop` does not run. The porter
   measured this and warns that the obvious probe is **masked** — measuring `stty` from
   an interactive prompt shows "cooked" even after a child died in raw mode, because bash
   restores it. Any re-test you run must use a **non-interactive** shell and a control
   case that fails differently. Also check SIGTERM/SIGHUP, and whether `cooked` is
   genuinely idempotent given it can be reached twice.
4. **Bug fidelity — this port is deliberately bug-for-bug.** Confirm, do not report as
   defects: `Home`/`End` leak a literal `~` into the pattern (`ESC[1~`/`ESC[4~` undecoded);
   SIGWINCH cancels the search via `EINTR` → `K_ESC`; the `g_resized` redraw branch is
   retained as dead code. A *deviation* from the C++ here IS a finding — in either
   direction. Report anything the port fixed or broke that the C++ did not.
5. **L5 and byte transparency.** Verify `say` now carries bytes end to end and the
   "pane moved" message reaches tmux with an invalid-UTF-8 pattern intact. Construct
   such a pattern and drive it.

Throwaway `-L` sockets only; kill them. Budget: **at most 6 harness or differential runs.**

## Severity

- `biases-deliverable` — a user would observe wrong behaviour, or the code is unsound or
  can panic/hang on reachable input. Blocks the port.
- `latent` — real but not user-observable on a reachable path.

Do not inflate, do not soften. Finding nothing blocking is a legitimate verdict.

## Output

Write full findings to `/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/nodes/verify-tui/findings.md`, one section per axis, each
finding with file:line, what is required, what the code does, the input that exposes it,
and the severity.

End your reply with a fenced result block, exactly this shape:

```result
status: ok | failed
findings_path: <path or ->
axes_covered: <n of 5>
blocking: <count>
latent: <count>
blocking_list: <none | "file:line — summary" per finding, semicolon-separated>
bug_fidelity: <both bugs faithfully reproduced? any deviation from the C++ in either direction>
panic_restore_retested: <yes|no — your method and control>
differential_runs: <how many you ran>
notes: <one line; on failure, why>
```

On failure write NO artifact — report it in the result block only. Return a terminal
result: do not background any self-check and do not end your turn waiting on anything.
