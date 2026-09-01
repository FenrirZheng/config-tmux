# verify-core — adversarial audit of the `sift` Rust port (non-interactive half)

Auditor: independent verifier. I did not write this code.
Under audit: `/home/fenrir/.tmux/tools/sift/src/main.rs` (801 lines, mtime 2026-08-31 23:35).
Authority: `/home/fenrir/.tmux/tools/sift/src/main.cpp`; contract:
`/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/nodes/spec/spec.md` §1–§4, §9.

**Verdict: 0 blocking, 6 latent.** The live path (`rows` + tmux plumbing + UTF-8 counters
+ POSIX scan) is byte-identical to the C++ baseline across every differential probe I
could construct, including the ones the existing harness does not reach. Every latent
finding below is either unreachable on a real input or is a divergence in a failure mode
the C++ also fails (differently).

Binaries driven: `baseline/sift-cpp` (76696 B, 2026-08-31 22:21) and
`target-dev/release/sift` (349528 B, 2026-08-31 23:35; confirmed to be the Rust build —
it carries the `not yet ported` string). The user's live
`~/.tmux/tools/target/release/sift` (2026-08-28) was never touched or executed.
All tmux work was on throwaway `-L sift_audit*` sockets, each killed on exit.

---

## Coverage note: what the "already passed" harness does NOT reach

`records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-jump.sh` lines 63–69
**re-implement the jump with raw `tmux` commands** (`t copy-mode`, `t send-keys -X
history-top`, `t send-keys -X goto-line …`). It never invokes sift's own `jump()`. Since
`jump()` is `#[allow(dead_code)]` in this node (its only caller is the un-ported
`run_ui`), **the entire jump function in the Rust port is unexecuted by every green
check the orchestrator has**. It is verified here by reading only. See axis 3.

Likewise, the harness's regex coverage is `bb1[0-9][0-9]`, `^row150 `, `aa999`,
`中文測試`, `^`. It exercises none of the ERE dialect edges (leftmost-longest,
backreferences, GNU `\<`/`\w`, `\d`-as-literal-d), none of the malformed-pattern paths,
and never reaches the 20000 cap. Those are the gaps I aimed at.

---

## Axis 1 — Regex parity

**Result: clean. No divergence found.**

Read: `Regex::compile` (main.rs:428-461), `find_from` (main.rs:477-499),
`Drop for Regex` (main.rs:502-508), `find_all` (main.rs:530-573), `MATCH_CAP`
(main.rs:67).

Checked and confirmed correct against `main.cpp:265-292` and `main.cpp:763-780`:

- `regcomp(&re, pat, REG_EXTENDED)` and nothing else — no `REG_ICASE`, `REG_NEWLINE`,
  `REG_NOSUB`. `nmatch = 1`, one `regmatch_t`. (spec §3.1)
- `flags = if off == 0 { 0 } else { REG_NOTBOL }` (main.rs:479) == `main.cpp:274`.
  `REG_NOTEOL` correctly absent.
- **The subject is genuinely sliced.** `find_from` copies the line, appends a NUL, and
  passes `subject.as_ptr().add(off)` (main.rs:486) — the exact analogue of
  `s.c_str() + off` (main.cpp:275). Measured: `\<a` over the fixture (which contains
  `aa a`) gives the same 12 rows from both binaries, i.e. the port reproduces the
  three-hits-on-`aa a` behaviour spec §3.2 requires and that `regex::find_at` would
  have given as two. This is spec §9 hazard 2 and the port passes it.
- **Empty-match advance is +1 BYTE** (`off = if e == b { e.0 + 1 } else { e.0 }`,
  main.rs:569) == main.cpp:288. Measured: `x*` over a fixture containing `中文`,
  `中文測試 aa999 尾巴` and a blank line → **175 rows, identical md5 in both binaries**,
  including the hits landing mid-character. spec §9 hazard 7 passed.
- **Loop bound `off <= s.len()`** (main.rs:547), not `<` — the len+1 hit is reproduced.
- **Empty lines skipped before any regex work** (main.rs:536) == main.cpp:268.
- **The cap is global and is exactly 20000, with no invented per-line cap.**
  `if hits.len() >= cap { return hits; }` returns mid-line (main.rs:560-564) ==
  main.cpp:286. Measured on a 900×90-char pane: `.` → **cpp 20000 rows, rust 20000
  rows, identical md5**; `z*` (empty-match blowup) → 20000 / 20000. Had a per-line cap
  been invented, the row *distribution* would differ and the md5 would not match.
- **Malformed patterns**: `(`, `[[.hyphen.]]`, `[z-a]`, `a{3,1}`, `[a`, `(?i)abc` — all
  six produce byte-identical stdout+stderr from both binaries, including the exact
  glibc text (`sift: invalid regex: Unmatched ( or \(`), and both exit 0.
- **ERE dialect**: `a|ab|abc`, `(a+)\1`, `\d+`, `\w+`, `a+?`, `a**`, `a{2,3}`,
  `[[:alpha:]]+`, `[[=a=]]`, `^a`, `$`, empty pattern — 26 patterns total, every one
  byte-identical. Binding glibc rather than the `regex` crate (spec §9 hazard 1) is
  correct and is what makes this pass.
- **`regfree` lifecycle**: exactly one `regfree`, in `Drop` (main.rs:506); `Regex` is
  neither `Copy` nor `Clone`, so no double free. On the *failure* path the Rust does not
  call `regfree` where the C++ does (main.cpp:770) — this is correct, not a leak:
  glibc's `regcomp` frees the fastmap and sets `preg->buffer = NULL` before returning an
  error, so the C++'s `regfree` is a no-op and there is nothing for Rust to reproduce.
  The source comment at main.rs:443-445 states this and it is accurate.

### L1 (latent) — `regerror` text is lossy-converted
`main.rs:457` — `String::from_utf8_lossy(&bytes)`. The C++ (`main.cpp:769`) hands
`regerror`'s bytes to `printf("%s")` unchanged. If `LC_MESSAGES` selected a locale whose
translated regex diagnostics are not UTF-8, the port would emit U+FFFD where the C++
emitted the raw bytes. Not reachable in any UTF-8 or `C` locale; measured identical here.
**latent.**

---

## Axis 2 — Text handling

**Result: clean. No divergence found.**

Read: `utf8_decode` (main.rs:154-180), `utf8_chars` (188-197), `cell_width` (210-221),
`utf8_cells` (224-233), `setlocale` (774).

- **The decoder is the permissive hand-rolled table, not `str::chars()`.** Operates on
  `&[u8]`. No overlong check, no surrogate check, no `> U+10FFFF` check, `0xFFFD`/len-1
  fallback (main.rs:179). Table compared expression-by-expression against
  main.cpp:114-136 — masks, shifts, `cont(k)` guard (`k < avail`), and the
  `(c & 0xE0)/(c & 0xF0)/(c & 0xF8)` lead-byte tests are identical. Rust operator
  precedence checked: `as` binds tighter than `|` and `<<`, so the parenthesisation is
  equivalent to the C++'s integer-promoted arithmetic.
  Measured: a fixture line `BAD:\xff\x80\xc0\x80\xf5\x80\x80\x80:END` written into a
  live pane, then `.`, `x*`, `BAD`, `\w+`, `[[:alpha:]]+`, empty-pattern — all
  byte-identical between the two binaries, so the port agrees on both the offsets and
  the widths derived from whatever bytes tmux hands back.
- **`cell_width` three-case rule is exact**: `if w < 0 { 1 } else { w as usize }`
  (main.rs:216-220) == `return w < 0 ? 1 : w;` (main.cpp:117). The **negative → 1** case
  — the one spec §4.3 and §9 hazard 3 single out — is present.
- **The port binds glibc `wcwidth` + `setlocale` rather than reimplementing an
  East-Asian-width table.** Spec §4.3 asserted "a Rust port has no `setlocale`"; that
  assertion is wrong, and the port's choice is strictly *more* faithful than the spec's
  own suggested mitigation. `setlocale(LC_ALL, "")` is the first statement of `main`
  (main.rs:774), before any locale-dependent call, exactly as main.cpp:784. The empty
  locale name is passed correctly (`b"\0"` is a valid empty C string).
  Verified constants: `wchar_t` = 4 bytes (i32) and `LC_ALL` = 6 in libc 0.2.189 on
  x86_64-gnu, matching `/usr/include/x86_64-linux-gnu/bits/locale.h:32`.
  Measured proof it is in force: the CJK line yields `char_start=5 / cell_start=9` for
  `aa999` in both binaries (a broken locale would give 5/5).
- **Mid-character `byte_end` counts the straddling character in full** — both loops bound
  on the *start* byte (main.rs:191, 227) == main.cpp:104, 122. This is what makes the
  175-row `x*` output match.
- **Unit discipline**: `ByteOff`/`CharOff`/`CellOff` newtypes; `rows` prints
  `line, char_start, char_end, cell_start` (main.rs:753-757) in the same order and with
  the same fields as `main.cpp:777`; byte offsets are computed and never emitted.

### L2 (latent) — the `text` field is written as raw bytes, not `%s`
`main.rs:758` (`w.write_all(text)`) vs `main.cpp:777` (`printf("…%s\n", …c_str())`).
`%s` stops at an embedded NUL; `write_all` does not. Spec §1.6 states capture lines
cannot contain NUL, so this is unreachable — and the port's behaviour is the more
faithful one for the *stated* contract ("the whole capture line, unmodified"). **latent.**

---

## Axis 3 — tmux interaction

**Result: clean by inspection. Note that `jump()` is dead code and untested by the
harness (see the coverage note above).**

- **Argv shapes.** Every tmux invocation compared element by element against the C++ and
  spec §2.7:
  - `display-message -p '#{pane_id}'` (main.rs:261) == main.cpp:149.
  - `display-message -p -t P '#{history_size}\t#{pane_height}\t#{alternate_on}'`
    (main.rs:346-352) == main.cpp:166-167. Format strings extracted mechanically from
    both files and compared: identical (the C++'s is split by adjacent-literal
    concatenation and rejoins to the same string).
  - `capture-pane -p -t P -S -<H> -E <height-1>` (main.rs:376-385) == main.cpp:192-194.
    **No `-J`, no `-e`, no `-N`/`-C`/`-a`** — confirmed absent (spec §2.3, §9 hazard 11).
  - `display-message -l <text>` in `say` (main.rs:141) — the `-l` is present
    (spec §2.6, §9 hazard 11).
  - Jump list (main.rs:609-686): `copy-mode -t P ; send-keys -X -t P history-top ;
    send-keys -X -t P goto-line N [; send-keys -X -N D -t P cursor-down]
    [; send-keys -X -N C -t P cursor-right] ; send-keys -X -t P search-backward
    <pattern>`. Element-for-element identical to main.cpp:318-346, **including the
    argument-order asymmetry** (`-X -N n -t P cmd` for counted sends, `-X -t P cmd` for
    uncounted) that spec §2.4 tells the port to reproduce. The `;` separators are
    separate `OsString` argv elements, never a shell string (spec §9 hazard 9).
  - Verification: `display-message -p -t P '#{history_size}\t#{scroll_position}\t
    #{copy_cursor_y}\t#{copy_cursor_x}\t#{search_present}'` (main.rs:704-712) ==
    main.cpp:363-366, all five fields in **one** call, positional and never textual.
  - No `set-option`, no `select-pane`, no `list-panes`, no non-`-X` `send-keys`,
    no `refresh-client` anywhere in the file. Confirmed by grep.
- **Jump sequence order** — `copy-mode`, then `history-top`, then `goto-line`, then the
  optional `cursor-down`/`cursor-right`, then `search-backward`. The `history-top`-first
  pin (spec §9 hazard 5) is present.
- **`history_size` IS re-read at jump time**: `let now = pane_geom(pane);` is the first
  statement of `jump` (main.rs:603), and every branch uses `now.history_size`, never a
  capture-time value. == main.cpp:320-322.
- **Landing verification units**: `f[4] == 1 && landed == line && f[3] == cell_start.0 as
  i64` (main.rs:720) — row compared in **line** units, column in **cell** units, while
  the cursor was moved with a **character** count. == main.cpp:378-379. The asymmetry
  spec §2.5 demands is reproduced.
- **`tmux_out` contract** (main.rs:112-126): argv vector via `Command::arg` per element,
  argv[0] = "tmux" (supplied by `Command`); **stdout piped, stderr left inherited**
  (only `.stdout(Stdio::piped())` is set); stdout read to EOF; success ==
  `status.success()`; spawn failure returns `(empty, false)` mirroring the C++'s
  `execvp` → `_exit(127)` path. Measured: tmux's own `can't find pane: %999` and
  `error connecting to /nonexistent/sock …` reach the user's stderr from both binaries,
  byte-identical.
- **`origin_pane`** (main.rs:255-271): honours a non-empty argument that does not start
  with `#{`; an empty argument or a `#{…}` argument falls through to tmux resolution;
  trailing `\n`/`\r` trimmed repeatedly; empty string on failure. ==
  `arg && *arg && strncmp(arg,"#{",2) != 0` (main.cpp:147). Measured for both the empty
  and the `#{pane_id}` argument: identical.
- **`strtol` / `tab_fields`** (main.rs:294-343): base 10, leading whitespace, optional
  sign, saturating, non-numeric ⇒ 0; the splitter stops early on a missing tab leaving
  later fields 0. Behaviourally equal to `strtol(f.c_str(), nullptr, 10)` +
  main.cpp:169-177 / 368-377 (spec §9 hazard 10 — no `parse().unwrap()`).

No finding on this axis. The only caveat is coverage, stated above: `jump()` has never
been executed in the Rust build.

---

## Axis 4 — CLI surface and exit codes

**Result: clean.** Every in-scope shape measured against the C++ side by side; exit code,
stderr and stdout all compared.

| invocation | cpp | rust | verdict |
|---|---|---|---|
| `sift rows` | exit 0, `usage: sift rows <pane-id> <regex>` | identical | OK |
| `sift rows P` (3 args) | exit 0, same usage line | identical | OK |
| `sift rows %999 x` | exit 0, silent from sift; tmux's `can't find pane: %999` on stderr | identical | OK |
| `sift rows P '('` | exit 0, `sift: invalid regex: Unmatched ( or \(` | identical | OK |
| `sift rows "" foo` | exit 0, no output | identical | OK |
| `sift rows P foo extra junk` | exit 0, extra args ignored | identical | OK |
| `sift rows '#{pane_id}' foo` | exit 0, self-heals to tmux resolution | identical | OK |
| `sift rows P $'a\xffb'` (non-UTF-8 regex) | exit 0 | identical — `args_os` is used, not `args` | OK |
| `TMUX=/nonexistent/sock,0,0 sift rows %1 foo` | exit 0, tmux's connect error only | identical | OK |
| `sift rowsX` | enters TUI | prints the not-yet-ported line | **out of scope** (TUI absent by design) |

Stderr literals compared byte-for-byte against the C++ source: `usage: sift rows
<pane-id> <regex>` and `sift: no pane \xe2\x80\x94 run it inside tmux, or pass a pane id`
are identical, em dash U+2014 included. `sift: the pane moved …` correctly lives in the
un-ported `run_ui`, not here. The `main` dispatch is an exact `argv[1] == b"rows"`
(main.rs:780), so `rowsX` is a pane id, matching `strcmp` (spec §1.1).

Exit code is 0 on every reachable path: no `process::exit`, no `main() -> Result`, no
`unwrap`/`expect`/`panic!`/`assert` anywhere in the file (grepped).

### L3 (latent) — SIGPIPE on stdout: exit 141 (cpp) vs 0 (rust), and the scan runs to completion
`main.rs:746-761`. Rust's runtime sets `SIGPIPE` to `SIG_IGN` at startup, and every
write in `run_rows` is `let _ = …`, so a closed stdout reader is swallowed.
Measured, `sift rows P . | head -1` with `pipefail`: **cpp `PIPESTATUS[0]=141`
(killed by SIGPIPE), rust `0`** — and the Rust process goes on to write all 20000 rows
into the dead pipe instead of dying at the first one. Neither behaviour is wrong for the
tool as deployed (`prefix /` opens a popup on a pty; `verify-sift-jump.sh` pipes into
`head` but sets no `pipefail` and reads no exit code), and the Rust behaviour is the one
that actually satisfies the "always exit 0" invariant. But it *is* a measured divergence
in exit status on an ordinary shell pipeline. **latent.**

---

## Axis 5 — Rust-specific hazards

**FFI soundness — verified by measurement, not by reading.** I compiled and ran ABI
probes on both sides rather than trusting the bindings:

```
C   : sizeof(regex_t)=64 align=8   sizeof(regmatch_t)=8  sizeof(regoff_t)=4  offsetof(rm_eo)=4
Rust: sizeof(regex_t)=64 align=8   sizeof(regmatch_t)=8  wchar_t=4  LC_ALL=6  REG_EXTENDED=1  REG_NOTBOL=1
```

This was the highest-risk thing in the file: glibc's `regex.h` defines
`regoff_t = ssize_t` under `_REGEX_LARGE_OFFSETS` and `int` otherwise, and libc 0.2.189
hardcodes `pub type regoff_t = c_int` for `linux-gnu`
(`src/unix/linux_like/linux/gnu/mod.rs:8`). Had the header taken the `ssize_t` branch,
`regexec` would have written 16 bytes into the port's 8-byte `regmatch_t` — a stack
overflow plus a garbage `rm_eo`. `/usr/include/regex.h:481-491` confirms the `int`
branch is the one in force here, and the two probes agree. **No hazard.** Same for
`regex_t`: 64 bytes on both sides, so `MaybeUninit::<regex_t>` is correctly sized for
`regcomp` to fill.

**Each `unsafe` block, checked:**

- main.rs:215 `wcwidth(cp as wchar_t)` — by-value, touches no memory of ours. The
  decoder's maximum output is `0x1FFFFF` (4-byte branch, main.rs:170-177), so the cast
  to `i32` cannot go negative. Sound.
- main.rs:435-441 `regcomp` — `pat` is `pattern.to_vec()` + a pushed `0` (main.rs:429-430),
  so it is NUL-terminated, and it outlives the call. `assume_init` only on `rc == 0`.
  Sound. (An *interior* NUL in the pattern truncates it, exactly as `std::string::c_str()`
  does in the C++ — parity, not a bug.)
- main.rs:451 `regerror(rc, re.as_ptr(), …)` on a still-uninitialised `regex_t` —
  matches the C++, which passes an uninitialised stack `regex_t` too; glibc's `regerror`
  indexes a static message table and never dereferences `preg`. `buf.len()` is 128,
  matching `sizeof buf` in main.cpp:767. Sound.
- main.rs:483-491 `regexec(&self.0, subject.as_ptr().add(off), 1, &mut m, flags)` —
  the stated precondition is `off <= subject.len() - 1`. It holds: `subject.len() ==
  s.len() + 1`, the caller's loop guard is `off <= s.len()` (main.rs:547), and the two
  ways `off` advances are `e.0` (bounded by `s.len()`, since `rm_eo <= strlen(subject +
  off)`) and `e.0 + 1` (which, when it exceeds `s.len()`, exits the loop before the next
  call). `nmatch = 1` matches the single `regmatch_t`. Sound.
- main.rs:506 `regfree(&mut self.0)` — exactly once, from `Drop`, on a value `regcomp`
  initialised. Sound.
- main.rs:774 `setlocale(LC_ALL, b"\0")` — single-threaded, first statement, static
  NUL-terminated literal. Sound.

**Panic audit (this matters because `panic = "abort"` in the workspace
`[profile.release]` — a panic is SIGABRT/134, not a catchable 101).** Every indexing site
is bounded: `s[i]`/`s[i+k]` in `utf8_decode` are guarded by the callers' `i < s.len()`
and by `cont`'s `k < avail`; `field[i]` in `strtol`, `s[start..]`/`s[start..t]` in
`tab_fields` (`start` can equal but never exceed `s.len()`), `blob[start..]` in
`capture`, and `argv[1..3]` are all length-checked. `lines.get(…).unwrap_or(b"")`
(main.rs:752) is `Option::unwrap_or`, not a panic. Release builds do not check integer
overflow, so the `i64` arithmetic wraps rather than panicking, as the C++ does. Even the
degenerate `rm_so == -1` case (unreachable for group 0) terminates without panicking —
the counters clamp at `s.len()` and the loop guard rejects the huge `off`.

That leaves exactly one panic vector:

### L4 (latent) — `eprintln!` panics on a failing stderr write; `panic = "abort"` turns it into SIGABRT
`main.rs:737, 784, 794, 800`. Rust's `eprintln!` panics on a write error
("failed printing to stderr"); the C++'s `fprintf(stderr, …)` just returns −1.
Combined with `SIGPIPE = SIG_IGN` and `panic = "abort"`, a broken stderr pipe turns an
error path into an abort.

Measured, deterministically (`os.pipe()`, read end closed, then `sift rows P '('` with
that write end as fd 2):

```
sift-cpp  rc = -13   (SIGPIPE)     → shell exit 141
sift      rc = -6    (SIGABRT)     → shell exit 134
```

I graded this **latent**, not blocking, and here is the honest reasoning: spec §1.2 says
the port "must not panic on any reachable input path", and this *is* a panic. But (a) the
C++ baseline also dies on the same input, just with a different signal, so it is not a
parity regression in the "sift used to survive this" sense; and (b) stderr is a pty in
the only shipped invocation (`display-popup -E`), and Rust's runtime reopens `/dev/null`
over any of fds 0/1/2 that are closed at startup — I verified `2>&-` with an invalid
regex exits **0** on both binaries for exactly that reason. A broken stderr *pipe* is the
sole trigger and it is exotic. If the port wants the invariant literally, the four
`eprintln!` calls should become `let _ = writeln!(io::stderr(), …)`. **latent.**

### L5 (latent, forward hazard for the TUI node) — `say` takes `&str` where the C++ takes `std::string`
`main.rs:140`. The C++'s `say` (main.cpp:100) accepts arbitrary bytes, and its one
message that embeds user data — `sift: the pane moved — landed on the nearest match of
/<pattern>/` (main.cpp:702) — concatenates the raw pattern the user typed, which is not
required to be valid UTF-8. A `&str` parameter cannot carry that. Dead code today, so no
user-visible effect; it will force either a lossy conversion or a signature change when
`run_ui` lands. **latent.**

### L6 (latent) — `strtol` saturation is off by one on the negative extreme
`main.rs:313-319`. On overflow the hand-rolled parser saturates the magnitude at
`i64::MAX` and then negates, giving `-i64::MAX` (`i64::MIN + 1`), where C's `strtol`
returns `LONG_MIN`. Requires a tmux format to reply with a 19-digit negative number.
Unreachable. **latent.**

---

## What I ran

4 differential runs, all on throwaway `-L` sockets, all killed on exit:

1. 26 patterns (ERE dialect edges, empty-match blowup, malformed patterns, empty
   pattern, CJK, combining mark, invalid UTF-8 bytes, embedded tab, blank line) ×
   both binaries, md5-compared including stderr. **26/26 identical.**
2. Global cap on a 900-line × 90-char pane (`.` and `z*`), plus 9 CLI shapes with
   exit code + stderr + stdout compared, plus no-server behaviour, plus SIGPIPE.
   **cap 20000/20000 identical; all in-scope CLI shapes identical; SIGPIPE diverges (L3).**
3. Broken-stderr probe via a shell pipeline (indicative).
4. Broken-stderr probe via `os.pipe()` with the read end pre-closed (deterministic) —
   the measurement behind L4.

Plus two non-differential compile-and-run ABI probes (C and Rust) for the `regex_t` /
`regmatch_t` / `regoff_t` / `wchar_t` / `LC_ALL` / `REG_*` layout question, and a
byte-level comparison of every stderr literal and tmux format string between the two
source files. `valgrind` is not installed on this machine, so the leak/double-free
question is answered by reading glibc's documented `regcomp` failure behaviour plus the
single-`Drop` argument above, not by instrumentation — stated here rather than dressed up
as a measurement.
