Complete a C++ → Rust port by writing its **interactive TUI half**. The non-interactive
half is already ported, verified, and byte-identical to the C++ binary across 26 regex
patterns — do not rewrite it, do not "improve" it, build on it.

## Your inputs

1. **The specification**: `/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/nodes/spec/spec.md`. Read **§5 (terminal control),
   §6 (key map), §7 (rendering)** in full, plus §9 (port hazards). It is measured, not
   inferred — the author drove the shipped binary against throwaway tmux servers and
   captured its rendering with `capture-pane -e`.
2. **The original**: `/home/fenrir/.tmux/tools/sift/src/main.cpp`. The authority where it
   and the spec disagree.
3. **The work so far**: `/home/fenrir/.tmux/tools/sift/src/main.rs` — 801 lines, the
   plumbing/regex/text half. Match its style; it already carries the C++'s section
   banners and its measured-surprise comments.
4. **An inherited finding you must resolve, not rediscover**: verify-core's **L5**, at
   `main.rs:140`. `say` currently takes `&str`, but the C++ `say(std::string)` is
   byte-transparent, and its one message embedding user data —
   `sift: the pane moved — landed on the nearest match of /<pattern>/` (main.cpp:702) —
   concatenates the raw pattern the user typed, **which need not be valid UTF-8**. You
   are the node that makes that message reachable. Change the signature to carry bytes;
   do not silently lossy-convert.

## Scope — port exactly these

`raw`, `cooked`, `on_winch`, `term_size`, `read_byte`, `read_key`, `Input`, `out`,
`render_line`, `Ui`, `refilter`, `draw`, `run_ui`, and `main`'s dispatch into `run_ui`.
`jump`, `say` and `tmux_run` already exist and currently carry `#[allow(dead_code)]` —
your work makes them live, so remove those attributes as they become reachable.

## Port faithfully — the bugs are in scope, and this is not negotiable

The user explicitly chose a **bug-for-bug** port so that equivalence can be proven
against the C++ baseline with no known-divergence carve-outs. A separate, already-planned
node fixes these afterwards. Reproduce, do **not** fix:

- **`Home` / `End` are broken.** The decoder accepts only `ESC[H`/`ESC OH`/`ESC[F`/`ESC OF`.
  tmux sends `ESC[1~`/`ESC[4~`, so the trailing `~` gets typed into the pattern —
  measured: `foo` + Home → header reads `regex> foo~`, `no match`. Reproduce exactly.
- **SIGWINCH cancels the search.** `poll()` is never restarted after the handler runs, so
  `EINTR` → `read_byte` returns −1 → `read_key` reports `K_ESC` → the cancel path fires and
  sift exits. The `g_resized` redraw branch is **dead code**. Reproduce that too,
  including keeping the dead branch, so the fix node has something to delete.

If you catch yourself improving something, stop and list it under `improvements_resisted`.

## The hazard that will not announce itself

`[profile.release]` sets **`panic = "abort"`**. There is no unwinding, so a `Drop`-based
termios restore **will not run on a panic** — the user's terminal is left in raw mode with
no echo. The C++ did not have this problem: it restored through an explicit `cooked()` and
a signal handler. Your restore path must survive an abort (a panic hook, `libc::atexit`,
or equivalent), not RAII alone. Prove it works: make the binary panic on purpose once in a
throwaway pane and show the terminal came back. Then remove the deliberate panic.

Also: the C++ installs a SIGWINCH handler. Whether it uses `SA_RESTART` is settled in the
spec — read it there rather than assuming, because it is the reason the resize bug exists.

## Engineering standards

- `unsafe` around each libc call, minimal, each with a comment stating the invariant that
  makes it sound.
- Keep the byte/char/cell newtype discipline (`ByteOff`/`CharOff`/`CellOff`) already
  established. `render_line`'s highlight span is in **bytes**; the rendering width is in
  **cells**. Do not collapse them.
- Zero `cargo build` warnings. The C++ built `-Wall -Wextra -Wpedantic`.

## Hard constraints — violating any of these breaks the user's live environment

- **Build only with `CARGO_TARGET_DIR=/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/target-dev`.** `prefix /` runs
  `/home/fenrir/.tmux/tools/target/release/sift` directly; writing there hands the user a
  half-finished tool.
- **Never a bare `cargo build --release` in `tools/`**; **never `cargo clean`**.
- Throwaway `-L` tmux sockets only — never the user's server. Kill every one you start.
- Do not touch `src/main.cpp` or `CMakeLists.txt`. Do not modify any doc, `claude.conf`,
  or `tmux.conf`. Do not commit.

## Prove it before you return

Two harnesses, both in
`/home/fenrir/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/`:

```
SIFT=/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/target-dev/release/sift timeout 300 bash verify-sift-live.sh   # C++ baseline: passed 6, failed 0
SIFT=/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/target-dev/release/sift timeout 180 bash verify-sift-jump.sh   # C++ baseline: passed 13, failed 0 — must not regress
```

`verify-sift-live.sh` is the one that matters most here: it is the **only** harness that
drives sift's own `jump()` end to end, by launching sift in a window, typing keys into it
and asserting on the target pane. Until it passes, `jump()` has never actually executed.

Report the real numbers, whatever they are. A partial pass is a legitimate result; a
fabricated one is not. Budget: **at most 8 cargo invocations.**

## Output contract

End your reply with a fenced result block, exactly this shape:

```result
status: ok | failed
files_written: <comma-separated paths>
build: <pass|fail>
warnings: <count>
live_passed: <N>   live_failed: <N>    (C++ baseline: 6 / 0)
jump_passed: <N>   jump_failed: <N>    (C++ baseline: 13 / 0)
panic_restores_terminal: <yes|no — and how you proved it>
bugs_reproduced: <Home/End: yes|no; SIGWINCH-cancel: yes|no>
L5_resolved: <how `say` now carries bytes>
improvements_resisted: <none | list>
live_binary_intact: <yes|no — sha256 vs /home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/baseline/sift-cpp>
notes: <one line; on failure, why>
```

On failure write NO artifact — report it in the result block only. Return a terminal
result: do not background any self-check and do not end your turn waiting on anything.
