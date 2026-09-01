# verify-fixes — regression & scope audit of the two-bug fix

Subject: `/home/fenrir/.tmux/tools/sift/src/main.rs` (1573 lines, mtime 2026-09-01 00:31:29).
Binary under test: `graphruns/…/target-dev/release/sift` (mtime 00:31:32 — 3 s newer than the
source, so it is built from the file audited here).
Control binary: `graphruns/…/baseline/sift-cpp`.
Reference: `tools/sift/src/main.cpp` (800 lines, the shipped C++).

**Verdict: 0 blocking, 6 latent.** The change set is what was authorised. None of the ten
queued latent findings was silently fixed. The `ByteOff`/`CharOff`/`CellOff` newtypes and the
`SAFETY:` discipline are intact. `READ_INTR` is handled correctly at every one of its call
sites, cannot reach the pattern buffer, and cannot spin. A bare `Escape` still cancels
(measured). One comment — the module header — is now stale.

---

## Method note (own instrument)

The orchestrator's Home/End and resize results were taken with `probe-sift-keys.py`, written by
the node that wrote the fix. I did not re-run it. I built two instruments of my own, sharing no
code with it, both driving a **throwaway `-L` tmux server with `TMUX` unset**:

* `scratchpad/homeend-probe.sh` — drives the popup through a real tmux pane and reads the
  header back with `capture-pane`. Three measurements in one server:
  * **premise check** — a pane running `cat -v`, sent `Home` then `End`, printed
    `^[[1~^[[4~`. So tmux really does emit `ESC[1~` / `ESC[4~`, which is the input the fix
    claims to decode. (This is the leg `probe-sift-keys.py` asserted rather than measured.)
  * **subject** — Rust sift, type `12` → `regex> 12 … 11 matches`; press `Home` → header still
    `regex> 12`, `11 matches`, and the selection marker `>` moved to the **first** hit (`112`);
    press `End` → header still `regex> 12`.
  * **can-fail control** — the C++ binary in the identical harness: `Home` → `regex> 12~` /
    `no match`; `End` → `regex> 12~~` / `no match`. The instrument reports failure when
    failure is there.
* `scratchpad/esc-resize-probe.sh` — Escape-cancel and SIGWINCH survival. The resize is induced
  by **splitting the sift pane's window** (29 → 14 rows), which delivers a genuine SIGWINCH:
  * `Escape` after typing `12` → the Rust process **returned** (marker file created) and the
    target pane is **not** in copy-mode (`#{pane_in_mode}` = 0) — cancel still cancels, pane
    untouched.
  * resize → Rust **alive**, header still `regex> 12`, list row 2 still rendering a hit.
  * can-fail control: the C++ binary in the same harness **exited** on the split, with an empty
    pane after it.

Two tmux runs total, both on throwaway sockets.

---

## Q1 — Scope

**Clean.** Nothing beyond the two authorised fixes, with one in-family extension noted as L1 below.

### The ten queued latents are all still present (none silently fixed)

| # | finding | still there at |
|---|---|---|
| L1 | lossy `regerror` text | `main.rs:458` `String::from_utf8_lossy(&bytes).into_owned()` |
| L2 | raw-byte `text` field in `rows` | `main.rs:1517-1533` (`w.write_all(text)`, un-escaped, un-truncated) |
| L3 | SIGPIPE-on-stdout (cpp 141 / rust 0) | no `SIGPIPE` handling anywhere — `libc::signal` appears once, at `main.rs:1375`, for `SIGWINCH` only |
| L4 | `eprintln!` + `panic=abort` → SIGABRT | `main.rs:1511`, `1558`, `1568` |
| L5 | `say` byte transparency (resolved earlier) | `main.rs:142` `fn say(text: &[u8])` |
| L6 | `strtol` negative-extreme saturation | `main.rs:314` `saturating_mul(10).saturating_add(...)`, `main.rs:317-318` `if neg { -v }` |
| T1 | added panic hook | `main.rs:1363-1367` |
| T2 | `out()` swallowing EPIPE | `main.rs:1057-1061` (write result discarded) |
| T3 | no 32-byte line-number truncation | `main.rs:1297` `format!("{:>width$}", …)` vs the C++ `snprintf(num, sizeof num, "%*ld", …)` at `main.cpp:654` |
| T4 | `Option`-guarded out-of-range `hit.line` | `main.rs:1300-1304` and `main.rs:1523-1526` (`.get(...).unwrap_or(b"")`) vs the C++ raw `lines[h.line]` at `main.cpp:777` |

### No refactor, rename or restructure

* The `run_ui` match arms are **branch-for-branch identical** to `main.cpp:706-752`: `Up`
  (`sel > 0`), `Down` (`sel + 1 < len`), `Home` (`sel = 0`, unguarded, exactly as C++), `End`
  (guarded), `PgUp`/`PgDn` (re-query `term_size`, `h > 4 ? h - 3 : 1`, strict `>` on PgUp),
  `Backspace`, `KillWord` (ASCII-space only), `KillLine`, `Text`, `None`. Same order, same
  guards, same no-wrap.
* `read_key`'s control-byte table (`13|10`, `127|8`, `23`, `21`, `3|7`, `16`, `14`), the
  `c1 == '[' || c1 == 'O'` gate, the `c < 32 → None` fallthrough and the 1/2/3 continuation-byte
  count all match `main.cpp:455-506` exactly. Nothing was removed from the CSI table; `1`, `7`,
  `4`, `8` were **added**. `ESC[3~` (Delete) still falls to `_ => Key::None` and still leaks its
  `~` — the port spec §8 item that was *not* in scope, correctly left alone.
* `draw`'s small-terminal guard `if h < 4 || w < 20 { return }` (`main.rs:1202`) is the C++
  `main.cpp:613` verbatim; `list_rows` clamp, `top` arithmetic and `numw` derivation likewise.
* `rows` output is byte-identical to the golden: `cmp baseline/rows-golden.txt
  nodes/fix-bugs/rows-after-fix.txt` → identical. The whole non-TUI half is behaviourally pinned.

### Newtypes and SAFETY discipline preserved

* `ByteOff` / `CharOff` / `CellOff` still declared at `main.rs:81/85/89` with the unit commentary
  above them, and still threaded through the three consumption sites: `utf8_chars` returns
  `CharOff`, `render_line` takes `ByteOff`, `jump` takes `CellOff`, `run_rows` prints `.0`.
  Nothing in the changed regions bypasses them (the key decoder deals in raw `i32` bytes, which
  is not a unit any of the three names).
* 19 `unsafe {}` blocks, 19 `SAFETY:` comments, plus a `SAFETY:` block above the one
  `unsafe impl Sync for SavedTermios`. Every `unsafe` in the changed region (`read_byte`'s
  `poll` at 880 and `read` at 889) carries its own. One pre-existing site,
  `main.rs:816 unsafe { std::mem::zeroed() }` in `term_size`, has its `SAFETY:` comment placed
  *after* it (covering the `ioctl` on 819) rather than before — that is port-tui's code, not the
  fix's, and `zeroed::<winsize>()` is sound. Not counted as a finding.

---

## Q2 — Correctness of the EINTR change

`READ_INTR` (`main.rs:856`, value −2) is produced only by `read_byte` (`main.rs:873-897`), on
two branches: `poll` < 0 with `errno == EINTR` (882) and `read` < 0 with `errno == EINTR` (890-892).

**`read_byte` has exactly two callers** (grep over the file confirms no others):

| # | caller | site | handling | correct? |
|---|---|---|---|---|
| 1 | `read_byte_seq` | `main.rs:905` | `loop { let c = read_byte(t); if c != READ_INTR { return c } }` — swallows it, retries | yes; `READ_INTR` can never escape this function |
| 2 | `read_key`, first byte | `main.rs:921-932` | `if c == READ_INTR { Key::None; return }` **before** `if c < 0 { Key::Esc }` | yes — and this ordering is the whole fix |

**`read_byte_seq` has six call sites**, all inside `read_key`, and since it provably never
returns −2, every one of them faces the pre-fix two-valued contract:

* `main.rs:970` `c1` — `c1 < 0 → Key::Esc` (the bare-Escape path)
* `main.rs:976` `c2` — `c2 < 0 → Key::Esc`
* `main.rs:998` / `1002` — consume the `~` of `ESC[1~`/`ESC[7~` / `ESC[4~`/`ESC[8~`, result discarded
* `main.rs:1006` — consume the `~` of `ESC[5~`/`ESC[6~`, result discarded (C++-faithful: no check it really is a `~`)
* `main.rs:1042` — UTF-8 continuation bytes, `if n < 0 { break }`

**Riskiest caller: #2, `read_key`:921-932.** It is the only place a −2 is interpreted rather
than retried, and its correctness rests entirely on the `== READ_INTR` test preceding the
`< 0` test. Swapping those two lines silently restores the original bug — every resize would
map to `Key::Esc` and cancel. There is no test in the tree pinning that ordering; it is pinned
only by the live/pty harnesses.

**Can `READ_INTR` leak into the pattern buffer?** No. The only writer of `u.pattern` from key
input is `Key::Text` (`main.rs:1486`), whose bytes come from (a) the first byte, reached only on
the `c >= 32` path, so `c` is a real byte, and (b) `read_byte_seq` at 1042, which cannot return
−2, and is additionally guarded by `if n < 0 { break }`. Even a hypothetical −2 there would be
caught by that guard rather than cast to `u8`.

**Does a bare `Escape` still cancel?** Yes — established two ways:
1. By code: `read_byte_seq` returns −1 on *timeout* (`poll` == 0 → `main.rs:884-886`), which is
   not `READ_INTR`, so the loop does not retry; `read_key`:971 turns that into `Key::Esc`.
   Timeout, EOF and short read remain conflated as −1 exactly as before; only EINTR was split out.
2. By measurement: my own `esc-resize-probe.sh` sent `Escape` to the running Rust popup — the
   process returned and the target pane was left out of copy-mode.

**Spin loop under a repeating signal?** No. Both loops block in `poll` between iterations
(`read_byte_seq` for up to 40 ms, `read_key`'s outer loop indefinitely), and each `READ_INTR`
requires an actually-delivered signal, so the iteration rate is bounded by the terminal's
SIGWINCH rate, not by the CPU. See L2/L3 below for the two bounded consequences.

---

## Q3 — New failure modes

**None blocking.** Four things checked, then the latents.

* **Can the resize path loop without progress?** No. `read_key` returns `Key::None`, the loop
  clears `G_RESIZED` with a `swap` (`main.rs:1388`) and redraws; the next iteration blocks in
  `poll(-1)`. Progress requires a fresh external event. The `swap` (not a load-then-store) also
  means a signal arriving between the `poll` return and the check is coalesced into the same
  redraw rather than lost or double-counted.
* **Can a resize during an escape sequence corrupt the decode?** No — that is precisely what
  `read_byte_seq` prevents: the interrupted wait is restarted instead of being read as a −1 and
  terminating the sequence early. Pre-fix, a SIGWINCH inside `ESC[` would have produced a spurious
  `Key::Esc` (cancel). `G_RESIZED` stays set through the retry and is serviced when `read_key`
  returns.
* **Terminal shrink / 1 row / 0 columns.** No panic path.
  * `draw` **re-queries** `term_size()` as its first statement (`main.rs:1199`) — nothing is
    cached; the PgUp/PgDn handler re-queries independently (`main.rs:1440`).
  * `h < 4 || w < 20` → emit nothing and return (`main.rs:1202-1204`). A 1-row terminal therefore
    draws nothing, the previous frame stays and keys still work — identical to `main.cpp:613`.
  * 0 columns never reaches `draw`: `term_size`'s zero-guard (`main.rs:820`) substitutes 80×24,
    faithful to `main.cpp:417`.
  * Shrink to fewer rows than the selection: `if u.sel >= u.top + rows { u.top = u.sel - rows + 1 }`
    (`main.rs:1216-1217`) is `usize` arithmetic, but the guard implies `u.sel >= rows` and
    `rows >= 1` is enforced at 1207-1209, so the subtraction cannot underflow. `u.hits.len() <= rows
    → top = 0` then re-clamps. `u.hits[idx]` at 1286 is guarded by `idx >= u.hits.len()` at 1282,
    and `u.hits[u.sel]` at 1400 by `!u.hits.is_empty()` plus the invariant that every writer of
    `sel` clamps to `len - 1`.
  * `text_w = w - numw - 3` can only go non-positive for an implausible `numw`; `render_line`
    returns empty on `width <= 0` (`main.rs:1076`).
* **Alternate-screen consistency.** `\x1b[?1049h` is written once, by `raw()` (`main.rs:808`);
  `\x1b[?1049l` once, by `cooked()` (`main.rs:769`), whose `G_RAW.swap` makes the `atexit` /
  panic-hook / explicit-call overlap idempotent. The resize path calls neither — `draw` emits only
  `\x1b[H\x1b[2J`, repainting **inside** the alternate screen. State stays consistent, and the
  typed pattern survives (measured: header still `regex> 12` after the 29 → 14 row shrink).

### Latent findings

**L1 (latent, scope) — `read_byte_seq` also covers UTF-8 continuation bytes, which is a third
divergence from the C++.** `main.rs:1042`. The authorisation says "a new `read_byte_seq` retries
`EINTR` inside an escape sequence"; it is also used for the continuation bytes of a multi-byte
character. Pre-fix (and in the C++, `main.cpp:502-504`), a SIGWINCH arriving between the lead byte
and a continuation byte returned −1 and `break` truncated the character, appending a partial
sequence to the pattern. That no longer happens. Strictly beneficial and in-family, but it is a
behaviour change on a path the fix brief does not name, and `read_byte_seq`'s own doc comment
(`main.rs:899-902`) describes only the escape-sequence use, so the file does not disclose it.

**L2 (latent) — a resize delivered while not blocked in `poll` is deferred until the next
keypress.** `G_RESIZED` is examined at `main.rs:1388`, i.e. *after* `read_key()` returns, and
`read_key` blocks indefinitely. A SIGWINCH landing during `draw` or key handling sets the flag but
is not acted on until some key arrives. The structure is inherited verbatim from `main.cpp:685-686`,
but it was unobservable there because any resize killed the process; it is observable now. The
window is one repaint wide, only the last signal of a burst can fall in it, the symptom is a
popup drawn at the previous size, and it self-heals on the next keystroke. Not measured — this is
a structural reading, not an attempt to race the window.

**L3 (latent) — `read_byte_seq` restarts the *full* 40 ms window on each `EINTR`.**
`main.rs:903-910` passes the same `timeout_ms` on every retry rather than tracking a deadline.
A sustained SIGWINCH stream (a drag-resize) therefore extends the Escape-vs-sequence
disambiguation window, and in the limit an incomplete escape sequence would not return while the
stream continues. Not a spin — each iteration blocks in `poll` — and the signal source is
externally bounded, so a drag ends and the loop returns. Worth knowing if a future change ever
calls `read_byte_seq(-1)`.

**L4 (latent) — two full repaints per resize.** On the resize iteration `draw` runs at
`main.rs:1389` and again at `main.rs:1493` (`Key::None` falls through to the unconditional
end-of-loop draw). Same shape as `main.cpp:686` + `main.cpp:752`, so it is faithful; the cost is
one redundant full-screen write per resize.

**L5 (latent) — the new numeric CSI arms make some longer sequences decode as Home.**
`ESC[15~` (F5) now takes the `b'1'` arm at `main.rs:997`, consumes the `5`, reports `Key::Home`
and leaks the trailing `~` into the pattern; `ESC[1;2A` (Shift-Up) likewise reports `Home`,
consumes the `;`, and leaks `2A`. The C++ reports `Key::None` for both and leaks more bytes
(`5~` and `;2A`). This is the single-byte-parameter assumption the pre-existing `5`/`6` arms
already made, and the code comment at `main.rs:994-996` is explicit that the trailing byte is
consumed unchecked — so the file is honest about it. Not on any documented key of the popup.

**L6 (latent, comment) — see Q4.**

---

## Q4 — Comment honesty

**Mostly yes; one stale claim.**

Every site comment in the changed regions describes what the code now does, and refers to the C++
behaviour in the past tense or as the C++'s, never as something retained:

* `main.rs:723-725` — "the SIGWINCH handler that makes a resize redraw the popup at the new size
  (port spec §5.4 — the C++ cancels the search instead; that is the interrupt handling fixed in
  `read_byte`/`read_key`)". Accurate.
* `main.rs:864-872` — states the new tri-valued contract, keeps timeout/EOF/short-read conflated as
  −1, and says folding `EINTR` in "is what **used to** cancel the search on every popup resize".
  Accurate.
* `main.rs:922-927` — "A signal, not a keypress… Reporting `Key::Esc` is what **used to** cancel the
  search on a resize." Accurate.
* `main.rs:984-996` — enumerates the accepted Home/End shapes, states that "The C++ recognises only
  the four letter forms, and so leaks the trailing `~` into the pattern", and openly notes that the
  `~` is consumed with no check that it really is a `~`. Accurate, and it is what makes L5 a
  disclosed consequence rather than a hidden one.
* `main.rs:1380-1387` — explains the EINTR → `READ_INTR` → `Key::None` → flag chain, that `draw`
  re-queries `term_size` and repaints without leaving the alternate screen, and that "The C++
  reports `Esc` instead and exits, which left this branch unreachable in practice". Accurate.

**No comment anywhere still asserts either bug is deliberate or must stay.** Grep for
`deliberat|bug-for-bug|faithful|improved|must stay` returns only: the module header (below), the
FFI-choice paragraph, `utf8_decode`'s permissiveness, `utf8_chars`' mid-character note,
`cell_width`'s `wcwidth < 0` divergence, the ellipsis note, the `EINTR` note at 869 (which is the
*fix* being called deliberate), and the no-SIGTERM/SIGHUP note at 1369 — all still true.

**L6 (latent) — the module header is now stale.** `main.rs:34-36`:

> **This is a bug-for-bug port.** Where the runbook/ADR/atlas and the shipped binary disagree, the
> shipped binary wins; nothing here is "improved". The divergences are inventoried in the port
> spec's §8.

That blanket claim is false on exactly the two authorised paths, and the Home/End fix is precisely
the case the sentence rules out: the runbook's key table says Home/End jump to the first/last
match, the shipped binary disagrees, and the port now sides with the **runbook**. The header does
not name the two bugs, so this is not a surviving "the bug is deliberate" assertion — but it is the
general policy statement that produced them, and it now contradicts the site comments 690 lines
below. It should name the two carve-outs (§5.4 and the §8 Home/End item) rather than claim there
are none.

---

## Result

0 blocking. The swap is not blocked by this audit. Six latent items, none user-observable on a
reachable path except L2 (a stale-size popup until the next keystroke, one repaint wide) and L6
(a comment, not behaviour).
