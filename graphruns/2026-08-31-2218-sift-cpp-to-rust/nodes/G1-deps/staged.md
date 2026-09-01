# G1 — dependency / regex-dialect decision  (STAGED, nothing created yet)

Pattern 2+3. Nothing under `tools/sift/` has been created. Fail-closed: no explicit
choice → halt.

## Evidence from `spec` (measured, not inferred)

- Compile flags: **`REG_EXTENDED` only** — no `REG_ICASE`, `REG_NEWLINE`, `REG_NOSUB`.
  `REG_NOTBOL` on continuation scans; `REG_NOTEOL` deliberately absent.
- glibc 2.41 probed directly through ctypes over ~50 patterns: **`\w \W \s \S \b \< \>`
  all work** (GNU extensions) and **backreferences work**. Only `\d` and lazy
  quantifiers are absent.
- POSIX is **leftmost-longest**: `a|ab|abc` on `xabc` → `abc`. Rust's `regex` crate is
  leftmost-first → `a`. Different span, different highlight, different `char_end`, so a
  different cursor seat.
- `(a+)\1` compiles under glibc, is a **hard error** under `regex`.
- `\d` works under `regex`, silently matches a literal `d` under glibc.
- Spec hazard #1: sift's hit list must agree with **tmux's own** search, because
  ADR-0005's bargain is that tmux performs the real search after the jump. A dialect
  change desynchronises the list from `n`/`N`.
- Spec hazard #3: `wcwidth` must keep the exact three-case rule — `>=1` as-is, `0` for
  combining, and **`1` for `wcwidth < 0`**. `unicode-width` does not reproduce that
  third case on its own.

## Ground truth on dependencies

- `libc` is **not** currently in `tools/Cargo.lock` — it would be a new dependency.
- `unicode-width` **is** already a workspace dependency (`seek` uses it).
- The C++ CMakeLists states zero-external-dependencies as a deliberate design property
  ("Adding ncurses would put a dev package in the fresh-machine bootstrap").

## Options

**A — libc FFI for everything platform-level (recommended).**
`regcomp`/`regexec`, `wcwidth`, `termios`, `poll`, `ioctl` all through the `libc` crate.
Behaviour-identical **by construction** — it is the same C library the C++ binary calls.
Cost: one new dependency (`libc`, ubiquitous, ~1 s to compile, no C toolchain needed at
build time) and `unsafe` blocks around each call. Keeps the fresh-machine bootstrap
story: still no dev packages.

**B — hybrid.** libc FFI for regex + termios; `unicode-width` for cell widths.
One less unsafe surface, but re-opens hazard #3 — the `wcwidth < 0` case must be
hand-patched on top and verified separately.

**C — pure Rust.** `regex` crate + `unicode-width` + `rustix`. Idiomatic and safe, no
`unsafe`. **Changes user-visible search behaviour**: no backreferences, leftmost-first
alternation, `\d` starts working, `\<`/`\>` stop working, and the hit list can disagree
with tmux's `n`/`N`. This is a feature change wearing a port's clothes.

## Second question staged with it — the three measured bugs

The spec found three divergences between the docs and the shipped binary:

1. **`Home` / `End` are broken.** The runbook's key table promises first/last-match
   jumps. The decoder accepts only `ESC[H`/`ESC OH`/`ESC[F`/`ESC OF`; tmux sends
   `ESC[1~`/`ESC[4~`, so the trailing `~` is **typed into the pattern**. Measured:
   `foo` + Home → header `regex> foo~`, `no match`.
2. **Resizing the popup cancels the search.** Undocumented. `poll()` is never restarted
   after SIGWINCH → `EINTR` → `read_byte` −1 → `read_key` reports `K_ESC` → cancel. The
   `g_resized` redraw branch is dead code. Confirmed twice against an idle control.
3. **The runbook is wrong about `\w`/`\s`** — a docs-only fix, folded into `integration`
   either way.

Porting (1) and (2) faithfully keeps the equivalence comparison clean; fixing them
inside the port means the verifier can no longer diff against the baseline for those
paths.
