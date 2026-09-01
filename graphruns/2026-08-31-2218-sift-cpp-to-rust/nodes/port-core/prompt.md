Port the **non-interactive half** of a C++ tool to Rust. This is a walking skeleton: the
thinnest end-to-end slice that a real test harness can exercise without a TTY. The
interactive TUI is a **later** node — do not write it.

## Your inputs

1. **The specification**: `/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/nodes/spec/spec.md` (1415 lines). It was written by an
   engineer who probed glibc's regex through ctypes and drove the shipped binary against
   throwaway tmux servers, so its claims are measured. Read §1 (CLI surface), §2 (tmux
   interaction), §3 (regex semantics), §4 (text handling) and §9 (port hazards) in full.
2. **The C++ source**: `/home/fenrir/.tmux/tools/sift/src/main.cpp`. The spec is the
   contract, but the source is the authority — where they disagree, follow the source
   and say so in your result block.
3. **The crate**: `/home/fenrir/.tmux/tools/sift/` already has a `Cargo.toml` and a stub
   `src/main.rs`. Build on it.

## Scope — port exactly these, nothing else

From the C++ source: `tmux_out`, `tmux_run`, `say`, `utf8_decode`, `utf8_chars`,
`cell_width`, `utf8_cells`, `origin_pane`, `Geom`/`pane_geom`, `capture`, `Hit`,
`find_all`, `jump`, `run_rows`, and the `main` dispatch for the `rows` subcommand.

**Do NOT port**: `raw`/`cooked`, `on_winch`, `term_size`, `read_byte`, `read_key`, `out`,
`render_line`, `Ui`, `refilter`, `draw`, `run_ui`. Leave the interactive path as a
`todo!()` or an explicit "not yet ported" stderr message — the later node fills it in.

## The decision that was already made for you

The user chose **libc FFI throughout**, and this is not negotiable:

- Regex: `libc::regcomp` / `libc::regexec` with `REG_EXTENDED`, and `REG_NOTBOL` on
  continuation scans. **Do not use the `regex` crate.** POSIX is leftmost-longest,
  supports backreferences and `\<`/`\>`, and treats `\d` as a literal `d` — Rust's
  `regex` crate is the near-mirror-image on every one of those, and sift's hit list must
  agree with tmux's own search or the `n`/`N` chain after a jump desynchronises.
- Cell width: `libc::wcwidth`, preserving the exact three-case rule the spec gives —
  `>= 1` as-is, `0` for combining, and **`1` when `wcwidth` returns `< 0`**. Do not
  substitute `unicode-width`.
- `libc::setlocale(LC_ALL, "")` at startup, as the C++ does. Without it `wcwidth` is
  wrong for CJK and every jump on such a line falsely reports "the pane moved".

## Port faithfully — including the bugs

This is a **bug-for-bug** port. The spec documents divergences between the docs and the
shipped binary; you reproduce the shipped behaviour, not the documented behaviour. A
later node fixes them deliberately, after equivalence has been proven. If you find
yourself "improving" something, stop and record it in your result block instead.

## Engineering standards

- Read `/home/fenrir/.tmux/tools/ARCHITECTURE.org` and one existing crate (`seek/`)
  first, and match the house style — module layout, error handling, comment density.
  These crates comment *why*, especially where a tmux behaviour is a measured surprise;
  carry those explanations across rather than dropping them.
- **Spec hazard #4 is the one to engineer against**: bytes, characters and cells are
  three different units consumed at three different sites (`regexec` reports bytes;
  `cursor-right -N` takes characters; `#{copy_cursor_x}` reports cells). The spec
  recommends three distinct newtypes rather than three `usize`s. Do that.
- `unsafe` is expected around every libc call. Keep each block minimal and put a comment
  above it stating the invariant that makes it sound.
- Warnings are errors in spirit: the C++ built with `-Wall -Wextra -Wpedantic`. Leave no
  `cargo build` warnings behind.

## Hard constraints — violating any of these breaks the user's live environment

- **Build only with `CARGO_TARGET_DIR=/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/target-dev`.** The tmux binding `prefix /`
  runs `/home/fenrir/.tmux/tools/target/release/sift` directly; writing your binary there
  would replace the user's working tool with a half-finished one.
- **Never a bare `cargo build --release` in `tools/`**; **never `cargo clean`** (it
  deletes the C++ binary too).
- Do not touch `src/main.cpp` or `CMakeLists.txt` — a later gated step removes them.
- Do not modify any other crate, any doc, `claude.conf`, or `tmux.conf`. Do not commit.

## Prove it before you return

The real harness is `/home/fenrir/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-jump.sh`.
It takes the binary through `$SIFT` and spins its own throwaway tmux server. The C++
baseline scores **passed 13, failed 0** — that is your target.

```
SIFT=/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/target-dev/release/sift timeout 180 bash verify-sift-jump.sh
```

Run it. Report the real numbers, whatever they are. A partial pass is a legitimate
result to return; a fabricated one is not. You may iterate, but keep it to **at most 8
cargo invocations** — if you are still failing after that, return with the failures
described.

## Output contract

End your reply with a fenced result block, exactly this shape:

```result
status: ok | failed
files_written: <comma-separated paths>
build: <pass|fail>
warnings: <count>
harness_passed: <N>
harness_failed: <N>
harness_baseline: 13 passed, 0 failed
divergences_from_spec: <none | list, each with source-line evidence>
improvements_resisted: <none | what you were tempted to fix and did not>
live_binary_intact: <yes|no — sha256 vs /home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/baseline/sift-cpp>
notes: <one line; on failure, why>
```

On failure write NO artifact — report it in the result block only. Return a terminal
result: do not background any self-check and do not end your turn waiting on anything.
