# verify-tui — adversarial audit of the `sift` Rust port (interactive TUI half)

Auditor: independent verifier. I did not write this code and had no part in the port.
Under audit: `/home/fenrir/.tmux/tools/sift/src/main.rs` lines 726–1519 (`SavedTermios`,
`on_winch`, `cooked`, `cooked_at_exit`, `raw`, `term_size`, `Key`, `Input`, `read_byte`,
`read_key`, `out`, `render_line`, `Ui`, `refilter`, `draw`, `run_ui`, `main`'s dispatch).
Authority on conflict: `/home/fenrir/.tmux/tools/sift/src/main.cpp` lines 380–800.
Contract: `nodes/spec/spec.md` §5 (terminal control), §6 (key map), §7 (rendering),
§9 (hazards).

**Verdict: 0 blocking, 4 latent. L5 is resolved — measured, not argued.**

Binaries driven: `baseline/sift-cpp` (C++) and `target-dev/release/sift` (Rust, confirmed
`panic = "abort"` via `tools/Cargo.toml` `[profile.release]`). The user's live
`~/.tmux/tools/target/release/sift` was never executed. All tmux work on throwaway
`-L sift_tuiaudit*` / `-L sift_a4_*` / `-L sift_a5` / `-L sift_a6` sockets, every one
killed on exit.

**One honest caveat about socket hygiene:** in run 2 two exploratory probes
(`sift rows <pane> <0x80>` and `sift $'\xff\xfe'`) were launched from my Bash tool rather
than from inside a throwaway pane. `sift` resolves tmux through the inherited `$TMUX`, so
those two invocations talked to the user's **live default tmux server**. They issued only
`display-message -p`, `capture-pane -p` and one `display-message -l` (a transient status
line) — no pane, window, mode or option was modified. Every subsequent probe was moved
inside a throwaway server. Stated because it happened, not because it changed a result.

---

## What I actually ran — 6 differential runs (budget: 6)

| # | harness | what it measures |
|---|---|---|
| 1 | `render_diff.sh` | 15 scenarios × both binaries in identical 80×24 tmux panes; `capture-pane -e` frame compared byte-for-byte |
| 2 | `run2.sh` | `rows` probe with a lone-`0x80` pattern; `say()` byte transparency via a `tmux` argv-logging shim; `#{alternate_on}` header warning |
| 3 | `run3.sh` | first (failed) attempt at driving an invalid-UTF-8 pattern to the "pane moved" message — tmux sanitises invalid bytes out of pane *content*, so the pattern found no hits. Kept because it is the reason run 4 exists |
| 4 | `run4.sh` | **L5**: pattern `one\|<0x80>` typed into the TUI, target session killed before `Enter`, `display-message -l` argv captured through the shim; plus a control where the target survives |
| 5 | `run5.py` | pty-based, **non-interactive** termios probe: Esc exit, Enter exit, SIGTERM, SIGHUP, SIGABRT, SIGKILL, and the non-tty `raw()` failure path |
| 6 | `run6.py` | **raw pty byte-stream** diff over 11 key scripts (not screen state — the actual bytes sift writes), plus the SIGWINCH-terminates-the-process test with a no-resize control |

Scripts live in the session scratchpad; the harness hook has copied them into
`records/2026-08-27-2240-tmux-sift/assets/scripts/`.

**Why run 6 exists even though run 1 was green.** `capture-pane -e` re-serialises the
screen *buffer*; it is not sift's output. Run 1 proved the two binaries paint the same
screen, which is what a user sees, but it would not catch a difference in the escape bytes
that renders identically (`\x1b[27m` came back as `\x1b[0m` in every capture, proving the
normalisation is real). Run 6 reads sift's bytes off a pty master and compares those.
Both layers agree.

---

## Axis 1 — Key map fidelity

**Result: clean. No divergence, blocking or latent.**

Read line-by-line against `main.cpp:449-527` and spec §6.1/§6.2:

- Single-byte table (main.rs:893-923) — `13|10→Enter`, `127|8→Backspace`, `23→KillWord`,
  `21→KillLine`, `3|7→Esc`, `16→Up`, `14→Down` — same bytes, same order, checked **before**
  the `0x1b` branch exactly as the C++ `switch` is.
- Escape decoder (main.rs:925-966): `ESC` → 40 ms → `[`/`O` → 40 ms → `A/B/H/F` and
  `5/6` (+ one more 40 ms read whose result is discarded, main.rs:952-953). Timeout on
  byte 2 or byte 3 → `Key::Esc`. Any other second byte → `Key::None`, byte consumed and
  lost. Unknown CSI final → `Key::None`. Identical.
- `ESC_SEQ_MS = 40` (main.rs:880) == the three literal `40`s in the C++. The first byte
  uses `read_byte(-1)`; UTF-8 continuation bytes use 40 ms and `break` on failure
  (main.rs:987-993). All three of spec §5.5's timeout call sites match.
- `read_byte` (main.rs:858-876) conflates `poll` == 0, `poll` == −1 (incl. `EINTR`), a
  short `read` and EOF into −1, and **does not retry** — the load-bearing conflation of
  spec §5.5. Identical to `main.cpp:439-447`.
- Bytes < 32 not otherwise handled → `Key::None`; bytes ≥ 32 → `Key::Text` with the
  `0xE0/0xC0`, `0xF0/0xE0`, `0xF8/0xF0` lead-byte table and **no continuation validation**.
  Identical.
- Selection/paging arithmetic (main.rs:1365-1394) vs `main.cpp:707-719`: initial
  `sel = hits.len()-1` set in `refilter` (main.rs:1136-1140, "nearest the bottom"); `Up`
  guarded by `sel > 0`, `Down` by `sel + 1 < len` — **no wrap at either end**; `Home` → 0
  unconditionally, `End` → `len-1` if non-empty; page `step = if h > 4 { h-3 } else { 1 }`
  re-queried from a fresh `term_size()`, with `PgUp`'s **strict** `sel > step`. Every
  constant and every comparison operator matches.

Measured (run 6, raw pty bytes; run 1, screen state): `needle` typing, `C-p`/`C-p`/`C-n`,
`ESC[5~`/`ESC[6~` paging, `ESC[1~`/`ESC[4~`, `ESC[H`/`ESC[F`/`ESC OH`/`ESC OF`,
`C-w`/`BS`/`DEL`/`C-u`, an eight-byte run of ignored control bytes
(`TAB C-a C-d C-e C-k C-l C-r C-z`), a CJK character, bare `ESC`, `C-g`, `C-c`,
`ESC[C`/`ESC[D`, `ESC[3~`. **11/11 scripts byte-identical, 15/15 screens byte-identical.**
The cancel scripts (`slowesc`, `cg`, `cc`) exited both processes, so the harness
distinguishes cancel from no-op.

No finding on this axis.

---

## Axis 2 — Rendering

**Result: clean. No divergence, blocking or latent.**

Every string literal in the rendering path was extracted from both source files and
hex-compared. All 20 are byte-identical, including the two that are easy to get wrong:

```
⚠ visible screen only ·    e2 9a a0 20 76 69 73 69 62 6c 65 20 73 63 72 65 65 6e 20 6f 6e 6c 79 20 c2 b7 20
↑↓ select  Enter jump …    e2 86 91 e2 86 93 20 73 65 6c 65 63 74 20 20 45 6e 74 65 72 …
```

(two-space separators, U+2191 U+2193, U+26A0, U+00B7, all present and correct.)

Structure checked against spec §7.1–§7.5 and `main.cpp:610-668`:

- `h < 4 || w < 20` → emit **nothing** (main.rs:1148-1150); `list_rows = h-2` with the
  (dead in both) `< 1` clamp; the three `top` clamps in the same order.
- Header status priority — `invalid regex: ` / `type an extended regex` / `no match` /
  `<N>[+] matches [(capped)]` — and the alternate-screen warning **prepended
  unconditionally on top of whichever status won**, including the invalid-regex one.
- `plen = 7 + utf8_chars(pattern)` and `slen = utf8_chars(status)` — **characters, not
  cells**, in both, which is what produces the §7.4 cursor-parking bug.
- Row format: `\x1b[1m> ` / two spaces, `\x1b[2m`, `{:>numw}` line number, `\x1b[22m `,
  `render_line(…, w - numw - 3)`, `\x1b[0m\r\n`. `numw` from `lines.len()-1`, not from
  the hits.
- `render_line` (main.rs:1021-1087) vs `main.cpp:519-565`: the `width - 12` trigger, the
  `width / 3` integer division, `skip_cells` floor at 0, the one-cell `…`, the
  `seen + w > skip_cells` / `cells + w > width` pair, the byte-unit `want` test against
  the cell-unit cut, and closing the invert with `\x1b[27m` (not `\x1b[0m`) so the
  selected row's unclosed bold survives. Expression for expression identical.

Measured states, all byte-identical between the binaries:

| state | evidence |
|---|---|
| empty pattern | `regex> ` + `type an extended regex` |
| matches | `regex> needle` + `5 matches` (no pluralisation) |
| no match | `regex> zzzznomatch` + `no match` |
| invalid regex | `regex> (` + `invalid regex: Unmatched ( or \(` |
| alternate screen | `⚠ visible screen only · 2 matches` with `#{alternate_on}=1` |
| CJK content | highlight lands on `中文` with the correct cell math |
| CJK **pattern** | header overruns the 80th column and wraps, scrolling the frame — the §7.4 character-vs-cell bug, reproduced identically |
| horizontal scroll | `…xxxx… needle yyy` with the leading `…`, on a 128-cell line at `text_w = 75` |
| `text_w` small | not separately driven; `w < 20` early return covers it |

**Not covered:** the `<N>+ matches (capped)` branch (would need ≥ 20000 hits in a live
pane). The literal is byte-identical and `u.capped = u.hits.len() >= MATCH_CAP`
(main.rs:1130) matches `main.cpp:604`; verify-core already measured the 20000 cap itself
as identical through the `rows` seam. Stated as read-only coverage, not claimed as
measured.

No finding on this axis.

---

## Axis 3 — Terminal control and restore

**Result: clean on every path I could drive. One latent deviation (T1).**

### Flags — exact (main.rs:793-798 vs main.cpp:401-406)

`c_lflag &= !(ECHO|ICANON|ISIG|IEXTEN)`, `c_iflag &= !(IXON|ICRNL|INLCR|BRKINT|ISTRIP)`,
`c_oflag &= !OPOST`, `VMIN=1`, `VTIME=0`, `TCSAFLUSH`. Nothing more: **not** `cfmakeraw`,
`IGNBRK`/`PARMRK`/`INPCK`/`IGNCR`/`IXANY` untouched, `CS8` not forced — spec §5.1
satisfied literally. `G_RAW` is set **before** the `\x1b[?1049h\x1b[2J` write, as
`g_raw = true` is in the C++. Both failure exits (`tcgetattr`, `tcsetattr`) return false
with `G_RAW` still clear, so `cooked()` is correctly a no-op afterwards.

`term_size` (main.rs:814-824): `TIOCGWINSZ` on **stdout**, the `ws_col > 0 && ws_row > 0`
guard, the 80×24 fallback, never cached — re-queried in `draw` and again in the PgUp/PgDn
handler. Matches spec §5.3.

### The restore re-test — non-interactive, with a control that fails differently

The porter's warning is correct and I did not fall into it: `stty` read from an
interactive prompt is masked by bash's own termios restore. My probe is a Python driver
(`run5.py`) that opens a pty, `fork`s, `execve`s the binary on the slave, and reads
`termios.tcgetattr(master)` **from the parent process** — no shell anywhere in the path.

The control is the `during` column: it must show raw mode while sift runs, or the probe
is not reaching the thing under test. It does, in all 12 trials.

```
                          during (probe reaching?)          after
cpp  Esc exit             ICANON/ECHO/ISIG all False   →   all True    alt_out=1
cpp  Enter jump exit      all False                    →   all True    alt_out=1
cpp  SIGTERM              all False                    →   all False   alt_out=0
cpp  SIGHUP               all False                    →   all False   alt_out=0
cpp  SIGABRT              all False                    →   all False   alt_out=0
cpp  SIGKILL              all False                    →   all False   alt_out=0
rust Esc exit             all False                    →   all True    alt_out=1
rust Enter jump exit      all False                    →   all True    alt_out=1
rust SIGTERM              all False                    →   all False   alt_out=0
rust SIGHUP               all False                    →   all False   alt_out=0
rust SIGABRT              all False                    →   all False   alt_out=0
rust SIGKILL              all False                    →   all False   alt_out=0
```

Read this the right way round: the six "all False → all False" rows are the proof that the
probe **can** report "not restored". The two restored rows in each binary are therefore
real measurements, not a probe that always says "cooked".

- **SIGTERM / SIGHUP: neither binary restores.** That is the deliberate parity spec §5.2
  demands ("do not add signal-based cleanup without being told to"). The port did not add
  any. Correct.
- **`cooked` is genuinely idempotent on the double-call paths.** Both normal exits reach
  `cooked()` explicitly *and* again through `atexit`, and the pty stream carries exactly
  **one** `\x1b[?1049l` in each. The Rust guard is `G_RAW.swap(false)` (main.rs:758) —
  strictly stronger than the C++'s read-then-write, and observationally identical.
- **`raw()` failure path**: with stdin on `/dev/null`, both binaries exit 0, write nothing
  to stdout, and write nothing to stderr (the `sift: no terminal …` message goes to tmux
  via `display-message -l`, not to stderr — spec §1.3 agrees).
- **SIGABRT is the row that matters for the panic hook.** `panic = "abort"` ends the
  process in `abort()`, i.e. SIGABRT, and the table shows SIGABRT leaves the terminal raw
  and on the alternate screen. That is exactly the hole the hook is there to close, and it
  confirms the porter's premise is not invented.

### T1 (latent, deviation from the C++) — the panic hook is an addition, and is unmeasured

`main.rs:1309-1313`. The C++ registers `atexit(cooked)` and nothing else. The port
registers `atexit(cooked_at_exit)` **and** a chained panic hook that calls `cooked()`
before delegating to the previous hook. Per the "a deviation in either direction is a
finding" rule, this is one.

Two honest statements about it:

1. **It is dead code today.** I audited the TUI half for a reachable panic and found none.
   Every index is guarded: `u.hits[idx]` in `draw` sits under `idx >= u.hits.len() →
   continue` (main.rs:1228); `u.hits[u.sel]` on `Enter` (main.rs:1346) sits under
   `!u.hits.is_empty()`, and `sel` is only ever assigned `0`, `hits.len()-1`, a
   `min(_, len-1)`, or a guarded ±1 — it can never exceed `len-1`; `u.pattern[i-1]` in the
   Backspace and `C-w` loops is guarded by `i > 0`; `u.top = u.sel - rows + 1` runs only
   under `u.sel >= u.top + rows`, so it cannot underflow (and release builds do not check
   overflow anyway); `render_line`'s slice end is clamped to `s.len()`; `u.lines.get(…)`
   is `Option::unwrap_or`. There is no `unwrap`/`expect`/`panic!`/`assert` anywhere in
   lines 800–1519 (grepped). `run_ui` also has no `eprintln!`, so verify-core's L4 does
   not reach this half.
2. **I did not measure the hook firing**, because I could not construct a reachable panic
   to fire it with. That it runs before `abort()` under `panic = "abort"` is a property of
   `std::panicking::rust_panic_with_hook`, asserted from knowledge, not from an experiment
   on this binary. Calling it "verified" would be false. What I *did* verify is the
   premise (SIGABRT does not restore) and that the hook cannot misfire: `cooked()` is
   `swap`-guarded, so the hook and `atexit` cannot double-restore.

Severity **latent**: unreachable today, and the direction of the deviation is
strictly safer than the C++.

### T2 (latent) — `out()` swallows `EPIPE` where the C++ takes `SIGPIPE`

`main.rs:1003-1008`. Rust's runtime sets `SIGPIPE` to `SIG_IGN`, so a `write` to a closed
stdout returns −1/`EPIPE` and is discarded by `let _ = r`; the C++'s identical
`(void)r` still dies, because its `SIGPIPE` disposition is the default. This is the TUI
instance of verify-core's L3. Reaching it needs stdin to be a tty (or `raw()` fails first)
while stdout is a pipe whose reader has gone — impossible under `display-popup -E`, which
is the only shipped invocation. **latent.**

---

## Axis 4 — Bug fidelity

All three deliberate bugs are **reproduced**, and I confirm them rather than report them.

1. **`Home`/`End` leak a literal `~`.** Measured at raw-byte level (run 6, script
   `homeend`) and at screen level (run 1): typing `foo` then sending `ESC[1~` then
   `ESC[4~` yields, from **both** binaries, byte-identically:
   `regex> foo~~` with status `no match`. The `ESC[1`/`ESC[4` prefix falls into
   `_ => Key::None` (main.rs:960) and the trailing `~` is read as a fresh printable key.
   The recognised `ESC[H`/`ESC[F`/`ESC OH`/`ESC OF` forms were also driven (script
   `csihome`) and are identical.
2. **SIGWINCH cancels the search.** Measured on a pty with `TIOCSCTTY` so the signal is
   really delivered: after `TIOCSWINSZ` on the master, **both** binaries are gone
   (`still_alive_after_resize = False`) and each emitted exactly one `\x1b[?1049l` — i.e.
   they took the `Key::Esc` branch and restored cleanly, they did not crash. The control
   (identical script, no resize) leaves **both** alive. So the port did *not* silently
   acquire an `EINTR` retry, and the `signal(SIGWINCH, …)` install — whose
   `on_winch as *const () as sighandler_t` cast is the kind of thing that fails
   silently, since SIGWINCH's default disposition is *ignore* — genuinely took effect.
   Had the cast been wrong, this test would have shown the Rust binary surviving the
   resize; it did not.
3. **The `G_RESIZED` redraw branch is dead code.** Retained at main.rs:1334-1336 in the
   same position as `main.cpp:686`, and the measurement above is what makes it dead.

**No deviation in either direction on any of the three.** The only deviations found
anywhere in this half are T1–T4, none of which is a bug the C++ has or a bug the port
introduced on a reachable path.

Two further divergences, recorded for completeness because they *are* differences even
though no input reaches them:

### T3 (latent) — line-number formatting has no 32-byte truncation
`main.rs:1243` uses `format!("{:>width$}", hit.line, width = numw)`; `main.cpp:653` uses
`snprintf(num, sizeof num, "%*ld", numw, hit.line)` into a `char num[32]`. For
`numw >= 32` the C++ truncates and the Rust does not. `numw` is the digit count of
`lines.len() - 1`, so this needs a 10^31-line scrollback. **Unreachable. latent.**

### T4 (latent) — out-of-range `hit.line` is `Option`-handled, not UB
`main.rs:1246-1250` uses `u.lines.get(hit.line as usize).unwrap_or(b"")` where
`main.cpp:660` uses an unchecked `std::vector::operator[]`. `hit.line` is always a valid
capture index, so neither branch is taken; the port's version is the safer one.
**Unreachable. latent.**

---

## Axis 5 — L5 and byte transparency

**Result: L5 is resolved. Measured, both halves of the claim.**

verify-core's L5 said `say` took `&str` and so could not carry the raw pattern into
`sift: the pane moved — landed on the nearest match of /<pattern>/`. The port now has
`fn say(text: &[u8])` (main.rs:140) handing bytes to `OsStr::from_bytes` →
`Command::arg`, and `run_ui` builds all three of its messages as `Vec<u8>`
(main.rs:1280-1282, 1287-1289, 1353-1357) with no conversion anywhere.

Measured with a `tmux` shim on `PATH` that logs every argv element before `exec`ing the
real tmux — so this is the byte string tmux actually received, not sift's intent.

**Probe A — `say` in general (run 2).** Invoking the binary with a pane argument of
`\xff\xfe`:

```
cpp : CALL <display-message> <-l> <sift: nothing to search in M-^?M-~>
rust: CALL <display-message> <-l> <sift: nothing to search in M-^?M-~>
```

Identical; the invalid bytes survive.

**Probe B — the "pane moved" message specifically, with an invalid-UTF-8 pattern
(run 4).** Constructing the pattern took two attempts, and the first failure is worth
recording: a pattern of a lone `0x80` compiles under glibc but can never *match*, because
tmux sanitises invalid UTF-8 out of pane content before `capture-pane` returns it — so
there were no hits and `Enter` was a no-op. The working construction is
`one|<0x80>`: valid ERE, invalid UTF-8, and it matches `one` in the pane. `jump` was then
forced to fail deterministically by killing the target session before `Enter`, so
`pane_geom` fails at jump time and `jump` returns false on its first branch.

```
cpp_bad : CALL <display-message> <-l> <sift: the pane moved M-bM-^@M-^T landed on the nearest match of /one|M-^@/>
rust_bad: CALL <display-message> <-l> <sift: the pane moved M-bM-^@M-^T landed on the nearest match of /one|M-^@/>
          (M-bM-^@M-^T = e2 80 94, the em dash; M-^@ = the 0x80 pattern byte)
```

Byte-identical, em dash intact, `0x80` intact. The same run also shows the pattern reaching
tmux intact one step earlier, in the jump sequence itself:
`<search-backward> <one|M-^@>` from both binaries.

**Control:** the same probe with a valid pattern and the target session left alive
produced **no** `display-message -l` call from either binary (and `<search-backward> <one>`
in both), so the probe distinguishes "message sent" from "message not sent" — it is not a
grep that always matches.

No finding on this axis. L5 is closed.

---

## Summary

| id | file:line | severity | one line |
|---|---|---|---|
| T1 | main.rs:1309-1313 | latent | panic hook is an addition the C++ does not have; unreachable today, and unmeasured (no reachable panic exists to fire it) |
| T2 | main.rs:1003-1008 | latent | `out()` discards `EPIPE`; the C++ dies of `SIGPIPE`. Needs tty stdin + closed-pipe stdout |
| T3 | main.rs:1243 | latent | no 32-byte truncation of the line number; needs a 10^31-line scrollback |
| T4 | main.rs:1246-1250 | latent | out-of-range `hit.line` returns `""` instead of being UB; unreachable |

**Nothing blocking.** Across 26 independently scripted differential comparisons — 15
screen-state, 11 raw-byte-stream — plus 12 pty termios trials and 4 argv-shim probes, the
Rust TUI and the C++ TUI produced identical bytes on every input I could construct,
including the two deliberate bugs and the invalid-UTF-8 message path this node was tasked
to resolve.
