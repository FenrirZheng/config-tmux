# G2 — the swap  (STAGED, nothing applied)

Working tree confirmed untouched: integration held no write permission.

## Diffstat

FILE                                                       CHANGE
.tmux/claude.conf                                          +4 / -6
.tmux/runbooks/sift.md                                     +48 / -23
.tmux/docs/adr/0005-own-the-interaction-loop-for-regex-search.org +9 / -1
.tmux/docs/adr/0006-port-sift-from-cpp-to-rust.org         CREATE (141 lines)
.tmux/tools/ARCHITECTURE.org                               +16 / -10
.tmux/tools/atlas/index.org                                +1 / -1
.tmux/tools/atlas/sift.org                                 +77 / -30
.tmux/CLAUDE.md                                            +3 / -2
.tmux/tools/sift/src/main.rs                               +8 / -3
.tmux/tools/sift/src/main.cpp                              DELETE (800 lines)
.tmux/tools/sift/CMakeLists.txt                            DELETE (33 lines)
CLAUDE.md                                                  +4 / -5

## Residual findings — require an EXPLICIT waiver, never waived by silence

Three adversarial audits produced **0 blocking** findings and **16 latent** ones. Latent
means: real, but not user-observable on a reachable path. The four worth a decision:

| # | finding | what a user could actually see |
|---|---|---|
| **L3** | `out()`/`rows` swallow `EPIPE` where the C++ dies of `SIGPIPE` | `sift rows … \| head -1` — C++ exits 141 and stops early; Rust exits 0 and writes all remaining rows into a dead pipe. Same visible output, wasted work, different exit status. |
| **new** | the Home/End fix accepts `ESC[15~` and `ESC[1;2A` as Home | pressing **F5** (or Shift-Up on some terminals) in the popup jumps to the first match instead of doing nothing. Disclosed in the adjacent code comment. |
| **T1** | the panic hook is an addition the C++ lacks, and is **unmeasured** | nothing — the verifier searched for a reachable panic and found none, so the hook is dead code. It said so rather than dressing reasoning up as a measurement. |
| **new** | a resize arriving while not blocked in `poll` defers the redraw to the next keypress | a resize between keystrokes shows a stale frame until you type. Inherited from the C++, only now observable because resize no longer kills the process. |

The remaining twelve (L1 lossy `regerror` text, L2 raw-byte `text` field, L4 `eprintln!` +
`panic=abort` → SIGABRT on a broken stderr pipe, L6 `strtol` negative-extreme saturation,
T2, T3 no 32-byte line-number truncation, T4 `Option`-guarded out-of-range index, the
`read_byte_seq` UTF-8-continuation extension, the per-`EINTR` 40 ms window restart, the
double repaint per resize) are unreachable or cosmetic.

## What applying this does

1. Copies 10 staged files over their targets across **two git repos**.
2. Deletes `tools/sift/src/main.cpp` (800 lines) and `tools/sift/CMakeLists.txt` — both
   recoverable from git history; neither is committed by this run.
3. Runs `cd ~/.tmux/tools && cargo build --release`, which finally puts the **Rust**
   binary on `tools/target/release/sift` — the live `prefix /` path.
4. Re-runs `verify-sift-live.sh`, so that its **test 6** (the real binding through
   `claude.conf`) measures the port for the first time — see REVISION 4.

Nothing is committed. The C++ binary remains at `<run>/baseline/sift-cpp` as a restore.
