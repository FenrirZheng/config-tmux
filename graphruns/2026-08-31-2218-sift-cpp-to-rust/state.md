# state.md — sift C++ → Rust

Dispatch tally: **this wave 1 / cumulative 10 / §10 sizing 10 planned, 14 ceiling, 21 = stop-and-ask**
Counters: `CORE_ROUND` **0/3 — closed, no repair round** · `TUI_ROUND` **0/3 — closed, no repair round**

## Negative control (pre-run goal check — must FAIL on missing work)

Ran `cargo build --release -p sift` in `~/.tmux/tools` before any dispatch:

```
error: package ID specification `sift` did not match any packages
help: a package with a similar name exists: `seek`
```

FAILS, and on the missing work — not on a crash or a bad path. Note the shell reported
`exit=0` because `$?` followed a pipe into `tail`; the error text, not that status, is
the evidence. Control that the check can also pass: the same metadata query matches the
existing package `seek`.

## Node table

| id | status | attempt | model | bound | dispatched-at | verified-by | result-ref |
|---|---|---|---|---|---|---|---|
| `spec` | **verified** | 1 | opus → sonnet | 15 min | 2026-08-31 22:22 | orchestrator: 1415 lines, 9/9 sections, all §6 fields present, tree untouched | `nodes/spec/spec.md` |
| `baseline` | **verified** | — (transform) | — | 5 min | 2026-08-31 22:22 | orchestrator: binary rescued off the collision path; harness ran | `baseline/{sift-cpp,jump-golden.txt,rows-golden.txt}` |
| `G1 deps` | **approved** | — (gate) | — | — | 2026-08-31 22:40 | user chose libc-FFI-throughout + faithful-port-then-fix | `nodes/G1-deps/staged.md` |
| `crate-init` | **verified** | 1 | sonnet | 10 min | 2026-08-31 22:43 | orchestrator rebuilt independently; live binary sha256 `105156f2…` == baseline; only Cargo.toml/lock + 2 new files changed; profile.release untouched; no .cargo/config.toml | `tools/sift/{Cargo.toml,src/main.rs}` |
| `port-core` | **verified** | 1 | opus → sonnet | 25 min | 2026-08-31 22:46 | orchestrator re-ran the harness itself: 13/0; rows output byte-identical to golden `525579d1…`; 0 warnings on forced recompile; no `regex` crate in Cargo.lock; live binary intact | `tools/sift/src/main.rs` (801 lines) |
| `verify-core` | **verified** | 1 | opus → sonnet | 15 min | 2026-08-31 22:57 | 0 blocking / 6 latent, confirmed on disk (`grep -c biases-deliverable` = 0); 5/5 axes; 4 differential runs; FFI layout checked by ABI probe | `nodes/verify-core/findings.md` (355 lines) |
| `port-tui` | **verified** | 1 | opus → sonnet | 30 min | 2026-08-31 23:14 | orchestrator ran both harnesses itself: live **6/0**, jump **13/0**, both == C++ baseline; 0 warnings on forced rebuild (LSP dead_code report was a stale snapshot); `jump()` reachable at main.rs:1352; live binary intact | `tools/sift/src/main.rs` (1519 lines) |
| `verify-tui` | **verified** | 1 | opus → sonnet | 20 min | 2026-09-01 00:07 | 0 blocking / 4 latent, confirmed on disk; 5/5 axes; 26 differential comparisons (15 screen + 11 raw pty byte-stream) all identical; panic-restore re-tested with a control that can fail | `nodes/verify-tui/findings.md` (361 lines) |
| `fix-bugs` | **verified** | 1 | opus → sonnet | 15 min | 2026-09-01 00:26 | orchestrator re-measured both fixes with the C++ as a failing control: Home→sel 3, End→sel 102, PgUp/PgDn intact; resize survives+redraws h40, no-resize control draws nothing, C++ control dies; harnesses 13/0 + 6/0; rows still byte-identical to golden | `tools/sift/src/main.rs` |
| `verify-fixes` | **verified** | 1 | opus → sonnet | 10 min | 2026-09-01 00:47 | 0 blocking / 6 latent on disk; scope clean, all ten queued latents still present and located; built its own instruments; binary mtime newer than source | `nodes/verify-fixes/findings.md` (275 lines) |
| `G2 swap` | **approved** | — (gate) | — | — | 2026-09-01 01:25 | user approved apply-everything + waive-all-16-and-record | `nodes/integration/staged.md` |
| `integration` | **verified** | 1 | sonnet | 20 min | 2026-09-01 01:07 | staged only, tree confirmed untouched; staged main.rs diff proven comment-only; 3 no-change claims independently checked; manifest applied by orchestrator post-approval | `nodes/integration/staged/` (12 entries) |
| `use-node` | **verified** | 1 | sonnet | 10 min | 2026-09-01 01:30 | fields 2-5 identical on all 135 rows; field 1 (volatile scrollback index) constant delta 1 — see REVISION 6; Q1 100 ✓, Q2 `5\t10` ✓ vs harness, Q3 POSIX leftmost-longest ✓ | `nodes/use-node/rows-usenode.txt` |

Terminal success states: `verified` for nodes, `approved` for gates.

## Baseline (transform, verified)

- `baseline/sift-cpp` — the C++ binary **rescued off `tools/target/release/sift`** before
  any cargo build can overwrite that path (§3 collision row).
- `baseline/jump-golden.txt` — `verify-sift-jump.sh`: **passed 13, failed 0**. This pass
  count is the structural equality target for the goal condition.
- `baseline/rows-golden.txt` — `sift rows` over the fixture for 4 patterns
  (`aa1[0-9][0-9]`, `中文測試`, `bb0(1|2)[0-9]`, `^row19[0-9] `): 135 lines,
  sha256 `525579d1d08107c6…`. Byte-comparison target for the use-node.
- Output format observed: 5 tab-separated fields — `line`, `char_start`, `char_end`,
  `cell_col`, `text`.

## depsDecision (G1, user-chosen — never written by an agent)

```
regex   = libc regcomp/regexec, REG_EXTENDED, REG_NOTBOL on continuation scans
width   = libc wcwidth  (preserves the `wcwidth < 0 => 1` case)
termios = libc tcgetattr/tcsetattr; poll + ioctl via libc
bugs    = faithful port first; Home/End and SIGWINCH fixed after both verifiers pass
```

## Baseline addendum (REVISION 3)

`baseline/live-golden.txt` — `verify-sift-live.sh` against the C++ binary:
**passed 6, failed 0**. This is the harness that actually exercises sift's own `jump()`;
`verify-sift-jump.sh` does not. Second structural equality target for §9.

## Residuals pending an explicit waiver at G2 (never waived by silence)

L1 `regerror` text lossy-converted · L2 `text` field raw bytes · L3 SIGPIPE on stdout
(cpp 141, rust 0) · L4 `eprintln!` + `panic=abort` gives SIGABRT where cpp gives SIGPIPE
· L5 `say(&str)` narrows a byte-transparent signature (handed to `port-tui`) ·
L6 `strtol` saturation off by one at the negative extreme.

## Measurement limitations carried forward (REVISION 4)

`verify-sift-live.sh` **test 6** sources the shipped `claude.conf`, whose binding points
at `tools/target/release/sift` — still the **C++** binary. It passed identically for both
runs because both ran the C++ binary. **5 of 6** live assertions exercise the port.
Test 6 becomes informative only after `integration` puts the Rust binary on the real
path; the orchestrator must re-run the live harness then. Until it does, no claim may be
made that the real `prefix /` binding drives the port.

## Residual addendum — verify-tui latents (also pending G2 waiver)

T1 panic hook is an addition the C++ lacks, and is **unmeasured** (no reachable panic
found; the verifier said so rather than dressing reasoning as measurement) · T2 `out()`
swallows EPIPE where the C++ takes SIGPIPE · T3 no 32-byte `snprintf` truncation on line
numbers · T4 out-of-range `hit.line` returns `""` instead of UB.

## Disclosed hygiene lapse

`verify-tui` reported on itself: two exploratory probes in its run 2 inherited `$TMUX`
and talked to the **user's live default tmux server**. Read-only (`display-message -p`,
`capture-pane -p`, one transient `display-message -l`); nothing modified; every later
probe used throwaway sockets. Recorded because it happened, not because it changed a
result. `fix-bugs` was instructed to `unset TMUX` explicitly.

## Note on instrument provenance (fix-bugs)

Every Home/End and resize measurement was taken with `probe-sift-keys.py`, **written by
the same node that wrote the fix**. That is a real weakness. It is mitigated, not
dismissed, by the control: the C++ binary run through the identical probe still exhibits
both bugs, so the instrument demonstrably *can* report failure. `verify-fixes` was
therefore instructed to build its own instrument for at least one behaviour rather than
re-run this one.

The probe was captured into `records/2026-08-27-2240-tmux-sift/assets/scripts/probe-sift-keys.py`
(untracked) — the `scratch-script-capture.sh` hook's normal behaviour for investigation
tools, not a mutation-set violation.

## G2 applied — 2026-09-01 01:25

10 files copied, 2 deleted, across two git repos. `cd ~/.tmux/tools && cargo build
--release` **alone** now produces `tools/target/release/sift`; the C++ source and
CMakeLists are gone. Nothing committed.

Goal condition re-measured against the **live installed binary** (not the one the
verifiers saw — integration's comment edit forced a recompile):

- `verify-sift-jump.sh` → **13 / 0**
- `verify-sift-live.sh` → **6 / 0**, and its **test 6 measures the port for the first
  time** — `claude.conf`'s real `prefix /` binding now resolves to the Rust binary
  (REVISION 4 closed).
- Every remaining `cmake` mention in the tree is historical provenance or an ADR
  option, checked by grep.

Residual waiver recorded in `docs/adr/0006-port-sift-from-cpp-to-rust.org` under
"Known divergences from the C++, accepted" — all 16, with the 4 noticeable ones named.

## Use-node outcome — 2026-09-01 01:40

A fresh agent, allowed to read **only** `runbooks/sift.md`, drove the shipped tool and:

- **Q1** 100 matches — matches the golden's 100 rows.
- **Q2** `202 5 10 9` — `char_start`/`char_end` = 5/10 exactly as `verify-sift-jump.sh`
  asserts, and it correctly identified field 4 as a wcwidth **cell** column diverging
  from the **character** index on the CJK line (5 vs 9).
- **Q3** `aa1|aa10|aa100` matched **`aa100`** (5 chars), proving **POSIX leftmost-longest**
  — the dialect property the libc-FFI decision existed to preserve, confirmed from
  outside by someone who did not know that was the goal.
- **Q4** output identical to the C++ golden on every non-volatile field (REVISION 6).

**Documentation defect found — NOT yet routed.** The runbook "never documents the
`sift rows <pane> <pattern>` subcommand's invocation syntax or its TSV output format at
all"; it only name-drops "sift rows output" once, in Verify. The agent had to discover
the subcommand, the argument order, and all five field meanings empirically with CJK
probes. Everything the runbook *did* state checked out correct — nothing was wrong, it
was incomplete. This gap pre-dates the port.
