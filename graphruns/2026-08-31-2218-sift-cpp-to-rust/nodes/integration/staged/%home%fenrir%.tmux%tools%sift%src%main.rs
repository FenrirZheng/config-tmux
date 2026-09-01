//! `sift` — popup regex search over a pane's scrollback, bound to `prefix /`.
//!
//! The regex sibling of `seek` (prefix Space). seek can lean on tmux's built-in
//! incremental search because that search is plain-text; tmux has no incremental
//! *regex* search to borrow, so this tool owns the interaction loop instead.
//! Rationale and the scope line against ADR-0001:
//!   docs/adr/0005-own-the-interaction-loop-for-regex-search.org
//!
//! This file is the Rust port of `src/main.cpp`, done in two steps: first the
//! non-interactive half — tmux plumbing, the UTF-8 counters, the POSIX-regex
//! scan, the jump arithmetic and the `rows` seam — then the popup TUI (raw
//! mode, key decoding, `render_line`, the `Ui` loop). Both halves are here now.
//!
//! Usage:
//!   sift [pane-id]                  the popup TUI
//!   sift rows <pane-id> <regex>     one line per match, no TUI — the seam every
//!                                   headless test asserts against, mirroring
//!                                   `cc-fleet rows`. Fields:
//!                                   line, char_start, char_end, cell_start, text
//!
//! Invariants inherited from tools/ARCHITECTURE.org:
//!   * Always exit 0. A non-zero exit from a key binding surfaces as a tmux
//!     error popup. There is therefore no `main() -> Result`, no `exit(1)`, and
//!     no reachable `unwrap()` — a panic would exit 101 (and, with
//!     `panic = "abort"` in the release profile, 134).
//!   * Pane text never enters a format context: no pane options are stamped and
//!     every display-message carries -l. Same proof obligation seek discharges;
//!     filtering the text instead would corrupt what the user wants to read.
//!   * Never write pane titles.
//!   * tmux is always spawned with an argv vector — never a shell string — so a
//!     scrollback line full of shell metacharacters is inert. `Command::arg`
//!     per element, including the `;` command-list separators.
//!
//! **This is a faithful port, with two named exceptions.** Where the
//! runbook/ADR/atlas and the shipped binary disagree on anything else, the
//! shipped binary wins — nothing else here is "improved" over the C++. The two
//! exceptions, both verified against the differential test suite: `Home`/`End`
//! now jump to the first/last match instead of leaking a literal `~` into the
//! pattern, and a popup resize now redraws at the new size instead of
//! cancelling the search. Every other divergence is inventoried in the port
//! spec's §8.
//!
//! Two deliberate FFI choices, both load-bearing (port spec §9 hazards 1 and 3):
//!
//!   * **glibc `regcomp`/`regexec`, not the `regex` crate.** tmux performs the
//!     actual jump, so sift's hit list must be the set tmux's own search would
//!     find. POSIX ERE is leftmost-*longest*, supports backreferences and
//!     `\<`/`\>`, and reads `\d` as a literal `d`; the `regex` crate is the
//!     near-mirror-image on every one of those. Swapping dialects desynchronises
//!     the `n`/`N` chain after a jump.
//!   * **`setlocale` + glibc `wcwidth`, not `unicode-width`.** Without the
//!     locale, `wcwidth` returns −1 for every non-ASCII code point, every CJK
//!     character counts 1 cell instead of 2, and the landing verification then
//!     reports "the pane moved" on every CJK line — a perfect jump described as
//!     a failure.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

// glibc's `wcwidth` is not bound by the `libc` crate; everything else here is.
extern "C" {
    fn wcwidth(c: libc::wchar_t) -> libc::c_int;
}

/// Hard ceiling on collected matches. `.` over a full 100k-line scrollback is a
/// legitimate keystroke on the way to a real pattern; the cap keeps that from
/// costing seconds. There is exactly one cap and it is **global, not per line**
/// — it aborts the whole scan mid-line.
const MATCH_CAP: usize = 20000;

// ── units ──────────────────────────────────────────────────────────────────
//
// Bytes, characters and cells are three different things consumed at three
// different sites, and confusing them is the classic bug here (ADR-0004 was
// written about its sibling in seek):
//   * regexec reports BYTE offsets;
//   * copy-mode's `cursor-right -N` moves by CHARACTER;
//   * #{copy_cursor_x} and the screen are measured in CELLS (CJK = 2).
// They are three distinct types so the compiler refuses the silent mix.

/// Byte offset into a capture line — what `regexec` reports.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct ByteOff(usize);

/// Characters before a byte offset — what `cursor-right -N` counts.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct CharOff(usize);

/// Terminal cells before a byte offset — what `#{copy_cursor_x}` reports.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct CellOff(usize);

// ── tmux plumbing ──────────────────────────────────────────────────────────
//
// The other Rust crates get this from tmuxlib; sift deliberately depends on
// nothing but libc (the C++ original had no tmuxlib to borrow from either), so
// these three functions are the whole of what has to exist on this side.

fn os(bytes: &[u8]) -> &OsStr {
    OsStr::from_bytes(bytes)
}

/// Run tmux with `args` (argv[0] is supplied by `Command`) and return its
/// stdout plus whether it exited 0.
///
/// stderr is left attached to ours so a real tmux error (`can't find pane: %999`)
/// is still visible when debugging by hand — only stdout is piped. The captured
/// stdout is returned whether or not the command succeeded; callers that care
/// check the flag.
///
/// Output is `Vec<u8>`, never `String`: `capture-pane` will happily hand back
/// bytes that are not valid UTF-8, and lossy conversion would change the byte
/// offsets every counter below is written in.
fn tmux_out<S: AsRef<OsStr>>(args: &[S]) -> (Vec<u8>, bool) {
    let mut cmd = Command::new("tmux");
    for a in args {
        cmd.arg(a);
    }
    cmd.stdout(Stdio::piped());
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return (Vec::new(), false),
    };
    match child.wait_with_output() {
        Ok(o) => (o.stdout, o.status.success()),
        Err(_) => (Vec::new(), false),
    }
}

fn tmux_run<S: AsRef<OsStr>>(args: &[S]) -> bool {
    tmux_out(args).1
}

/// Every user-facing message is literal (-l): pane text must never be parsed as
/// a format string.
///
/// **Bytes, not `&str`.** The C++ `say(std::string)` is byte-transparent, and
/// its one message embedding user data — `sift: the pane moved — landed on the
/// nearest match of /<pattern>/` — concatenates the raw pattern the user typed,
/// which need not be valid UTF-8 (`read_key` appends continuation bytes without
/// validating them, so a half-typed multi-byte character or a lone 0x80 can be
/// in there). `&str` would force a lossy conversion and change the message tmux
/// displays; `OsStr::from_bytes` hands the same bytes to `Command::arg`.
fn say(text: &[u8]) {
    tmux_run(&[OsStr::new("display-message"), OsStr::new("-l"), os(text)]);
}

// ── UTF-8 ──────────────────────────────────────────────────────────────────

/// Decode one code point at `i`; returns `(code point, byte length)`. The length
/// is always ≥ 1, so a malformed byte advances rather than looping forever.
///
/// Deliberately permissive, and deliberately *not* `str::chars()`: there is no
/// overlong-encoding check, no surrogate check, no `cp > 0x10FFFF` check. A
/// 5-byte lead byte, an overlong `C0 80` and a lone `0x80` all take the
/// U+FFFD/length-1 path or decode as written. Capture output is not guaranteed
/// to be valid UTF-8 and Rust's replacement behaviour differs from this table.
fn utf8_decode(s: &[u8], i: usize) -> (u32, usize) {
    let c = s[i];
    let avail = s.len() - i;
    let cont = |k: usize| k < avail && (s[i + k] & 0xC0) == 0x80;
    if c < 0x80 {
        return (c as u32, 1);
    }
    if (c & 0xE0) == 0xC0 && cont(1) {
        return ((((c & 0x1F) as u32) << 6) | (s[i + 1] & 0x3F) as u32, 2);
    }
    if (c & 0xF0) == 0xE0 && cont(1) && cont(2) {
        return (
            (((c & 0x0F) as u32) << 12) | (((s[i + 1] & 0x3F) as u32) << 6) | (s[i + 2] & 0x3F) as u32,
            3,
        );
    }
    if (c & 0xF8) == 0xF0 && cont(1) && cont(2) && cont(3) {
        return (
            (((c & 0x07) as u32) << 18)
                | (((s[i + 1] & 0x3F) as u32) << 12)
                | (((s[i + 2] & 0x3F) as u32) << 6)
                | (s[i + 3] & 0x3F) as u32,
            4,
        );
    }
    (0xFFFD, 1)
}

/// Characters in `s[0, byte_end)` — this is what `cursor-right -N` counts.
///
/// Note the loop bound is on the *start* byte of each character, so a `byte_end`
/// that falls mid-character counts the straddling character **in full**. That is
/// exactly the situation the empty-match `+1 byte` advance in `find_all`
/// creates, and it is faithful behaviour, not a rounding bug.
fn utf8_chars(s: &[u8], byte_end: ByteOff) -> CharOff {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < byte_end.0 && i < s.len() {
        let (_, len) = utf8_decode(s, i);
        i += len;
        n += 1;
    }
    CharOff(n)
}

/// One code point's width in terminal cells.
///
/// Three cases, and the third is the interesting one:
///   * `wcwidth >= 1` — used as-is (CJK, fullwidth forms and most emoji give 2);
///   * `wcwidth == 0` — combining marks, ZWJ: zero cells;
///   * `wcwidth < 0`  — control/unassigned code points, including the U+FFFD the
///     malformed-byte path produces: rendered as **one** cell rather than
///     vanishing. This is the one place the program deliberately diverges from
///     what the terminal will actually do.
///
/// Correct only after `setlocale(LC_ALL, "")`; see the module header.
fn cell_width(cp: u32) -> usize {
    // SAFETY: `wcwidth` is a pure lookup over the process locale, takes a
    // by-value wchar_t and touches no memory we own. Our decoder never yields a
    // value above 0x1FFFFF, so the `as` cast cannot go negative on 32-bit
    // wchar_t and misrepresent the code point.
    let w = unsafe { wcwidth(cp as libc::wchar_t) };
    if w < 0 {
        1
    } else {
        w as usize
    }
}

/// Cells occupied by `s[0, byte_end)` — what `#{copy_cursor_x}` counts.
fn utf8_cells(s: &[u8], byte_end: ByteOff) -> CellOff {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < byte_end.0 && i < s.len() {
        let (cp, len) = utf8_decode(s, i);
        i += len;
        n += cell_width(cp);
    }
    CellOff(n)
}

// ── pane resolution ────────────────────────────────────────────────────────

/// Which pane are we searching?
///
/// The obvious answer — have the key binding pass `#{pane_id}` — does not work.
/// Measured on tmux 3.5a: `display-popup`'s shell-command is NOT format-expanded,
/// so the program receives the literal seven characters `#{pane_id}`; neither is
/// `-e VAR=#{pane_id}`. (`run-shell` does expand, which is what makes the
/// difference easy to miss.) And `$TMUX_PANE` inside a popup names the POPUP's
/// own pseudo-pane, not the pane it was opened over — using it would search the
/// wrong thing rather than fail loudly.
///
/// What does work is asking tmux directly: while a popup is open the client's
/// active pane is still the one the key was pressed in.
///
/// An explicit argument is honoured when given (the `rows` seam and the tests
/// rely on it) — except when it still looks like an unexpanded format, which
/// means a binding regressed and is better self-healed than flashed away. An
/// *empty* argument falls through to tmux resolution too, matching the C++
/// `arg && *arg && strncmp(arg, "#{", 2) != 0`.
fn origin_pane(arg: Option<&[u8]>) -> Vec<u8> {
    if let Some(a) = arg {
        if !a.is_empty() && !a.starts_with(b"#{") {
            return a.to_vec();
        }
    }
    let (out, ok) = tmux_out(&["display-message", "-p", "#{pane_id}"]);
    let mut s = out;
    while matches!(s.last(), Some(b'\n') | Some(b'\r')) {
        s.pop();
    }
    if ok {
        s
    } else {
        Vec::new()
    }
}

// ── pane inspection ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default)]
struct Geom {
    history_size: i64,
    height: i64,
    /// Read but not acted on: the TUI only prepends a header warning, and even
    /// that overstates the case — with `less` running, `history_size` was 30 and
    /// the identical capture returned the 30 pre-alternate history lines *plus*
    /// the 12 alternate rows. sift changes nothing about what it captures.
    alternate: bool,
    ok: bool,
}

/// `strtol(field, nullptr, 10)`, not `str::parse().unwrap()`.
///
/// tmux's replies carry a trailing newline and the tab-splitter below stops
/// early when a separator is missing, so both a `"0\n"` field and a wholly
/// absent one must yield 0 rather than an error — and certainly rather than a
/// panic (every path here exits 0).
fn strtol(field: &[u8]) -> i64 {
    let mut i = 0usize;
    while i < field.len() && field[i].is_ascii_whitespace() {
        i += 1;
    }
    let neg = match field.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let mut v: i64 = 0;
    while i < field.len() && field[i].is_ascii_digit() {
        // strtol saturates at LONG_MIN/LONG_MAX rather than wrapping or trapping.
        v = v.saturating_mul(10).saturating_add((field[i] - b'0') as i64);
        i += 1;
    }
    if neg {
        -v
    } else {
        v
    }
}

/// Split a tmux reply on tabs into `N` numeric fields, **stopping early** when a
/// tab is absent so later fields keep their zero. Same splitter for the
/// three-field geometry reply and the five-field landing probe.
fn tab_fields<const N: usize>(s: &[u8]) -> [i64; N] {
    let mut v = [0i64; N];
    let mut start = 0usize;
    for slot in v.iter_mut() {
        let tab = s[start..].iter().position(|&b| b == b'\t').map(|p| start + p);
        match tab {
            Some(t) => {
                *slot = strtol(&s[start..t]);
                start = t + 1;
            }
            None => {
                *slot = strtol(&s[start..]);
                break;
            }
        }
    }
    v
}

fn pane_geom(pane: &[u8]) -> Geom {
    let (s, ok) = tmux_out(&[
        OsStr::new("display-message"),
        OsStr::new("-p"),
        OsStr::new("-t"),
        os(pane),
        OsStr::new("#{history_size}\t#{pane_height}\t#{alternate_on}"),
    ]);
    if !ok {
        return Geom::default();
    }
    let v = tab_fields::<3>(&s);
    Geom {
        history_size: v[0],
        height: v[1],
        alternate: v[2] != 0,
        ok: true,
    }
}

/// Capture history + visible screen. Deliberately no `-J`: joining wrapped lines
/// would desynchronise our indices from the physical lines copy-mode navigates.
/// No `-e` either: escape sequences would be matched by the regex and rendered
/// raw. Index `i` in the result is physical line `i` counting from the top of
/// history — the coordinate the jump arithmetic below is written in.
///
/// Lines stay as raw bytes. Trailing whitespace is trimmed by tmux itself; do
/// not re-pad.
fn capture(pane: &[u8], g: &Geom) -> Vec<Vec<u8>> {
    let start_arg = format!("-{}", g.history_size);
    let end_arg = format!("{}", g.height - 1);
    let (blob, ok) = tmux_out(&[
        OsStr::new("capture-pane"),
        OsStr::new("-p"),
        OsStr::new("-t"),
        os(pane),
        OsStr::new("-S"),
        OsStr::new(&start_arg),
        OsStr::new("-E"),
        OsStr::new(&end_arg),
    ]);
    let mut lines: Vec<Vec<u8>> = Vec::new();
    if !ok {
        return lines;
    }
    // A trailing newline does not produce a final empty element; a run of `\n\n`
    // does. Empty elements are kept — they occupy an index — and skipped by the
    // matcher.
    let mut start = 0usize;
    while start <= blob.len() {
        match blob[start..].iter().position(|&b| b == b'\n') {
            Some(p) => {
                let nl = start + p;
                lines.push(blob[start..nl].to_vec());
                start = nl + 1;
            }
            None => {
                if start < blob.len() {
                    lines.push(blob[start..].to_vec());
                }
                break;
            }
        }
    }
    lines
}

// ── matching ───────────────────────────────────────────────────────────────
//
// POSIX regcomp(REG_EXTENDED) through libc, not Rust's `regex` crate: the jump
// is ultimately performed by tmux's own search, so our match set has to be the
// same one tmux would find or the list would show hits the jump cannot
// reproduce. (Verified by the differential test in
// records/.../verify-sift-regex-parity.sh, and by assertion 9 of
// verify-sift-jump.sh.)

/// RAII wrapper over `regex_t`. `regfree` runs on every path, as in the C++.
struct Regex(libc::regex_t);

impl Regex {
    /// `regcomp(&re, pattern, REG_EXTENDED)` — and nothing else. No `REG_ICASE`
    /// (search is always case sensitive), no `REG_NEWLINE` (input is already
    /// split), no `REG_NOSUB` (nmatch is 1; capture groups are never read).
    fn compile(pattern: &[u8]) -> Result<Regex, String> {
        let mut pat = pattern.to_vec();
        pat.push(0);
        let mut re = std::mem::MaybeUninit::<libc::regex_t>::uninit();
        // SAFETY: `pat` is NUL-terminated and outlives the call (regcomp copies
        // what it needs). regcomp fully initialises `*re` on rc == 0 and is the
        // only writer; we assume_init only on that branch.
        let rc = unsafe {
            libc::regcomp(
                re.as_mut_ptr(),
                pat.as_ptr() as *const libc::c_char,
                libc::REG_EXTENDED,
            )
        };
        if rc != 0 {
            // The C++ calls regfree on the failed regex_t; glibc tolerates that,
            // but there is nothing for Rust to reproduce — the value was never
            // initialised, so it is simply dropped.
            let mut buf = [0 as libc::c_char; 128];
            // SAFETY: regerror writes at most buf.len() bytes including the NUL
            // and, in glibc, never dereferences `preg` — it only indexes a static
            // message table by errcode. The pointer is valid-but-uninitialised,
            // which is legal to form and pass.
            unsafe { libc::regerror(rc, re.as_ptr(), buf.as_mut_ptr(), buf.len()) };
            let bytes: Vec<u8> = buf
                .iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as u8)
                .collect();
            return Err(String::from_utf8_lossy(&bytes).into_owned());
        }
        // SAFETY: regcomp returned 0, so `re` is initialised.
        Ok(Regex(unsafe { re.assume_init() }))
    }

    /// One `regexec` against the **suffix** `subject[off..]`.
    ///
    /// `subject` must be NUL-terminated (`subject.len()` counts that NUL) and
    /// `off` must be at most the line length. The returned span is re-based onto
    /// the whole line.
    ///
    /// `REG_NOTBOL` on every scan after the first is why an anchored pattern
    /// like `^foo` yields exactly one hit per line. `REG_NOTEOL` is *not* set:
    /// the suffix's end is the line's end, so `$` still anchors correctly.
    ///
    /// The subject is genuinely sliced rather than searched from an offset with
    /// preceding context. That is observable: `\<a` over `aa a` gives hits at
    /// bytes 0, 1 and 3 here, where a context-preserving API would give two.
    /// Faithful; the port must slice.
    fn find_from(&self, subject: &[u8], off: usize) -> Option<(ByteOff, ByteOff)> {
        let mut m = libc::regmatch_t { rm_so: 0, rm_eo: 0 };
        let flags = if off == 0 { 0 } else { libc::REG_NOTBOL };
        // SAFETY: `subject` is NUL-terminated and `off <= subject.len() - 1`, so
        // the offset pointer is in bounds and points at a NUL-terminated string.
        // nmatch = 1 matches the single regmatch_t we hand over.
        let rc = unsafe {
            libc::regexec(
                &self.0,
                subject.as_ptr().add(off) as *const libc::c_char,
                1,
                &mut m,
                flags,
            )
        };
        if rc != 0 {
            return None;
        }
        Some((
            ByteOff(off + m.rm_so as usize),
            ByteOff(off + m.rm_eo as usize),
        ))
    }
}

impl Drop for Regex {
    fn drop(&mut self) {
        // SAFETY: `self.0` was initialised by a successful regcomp and is freed
        // exactly once, here.
        unsafe { libc::regfree(&mut self.0) };
    }
}

#[derive(Clone, Copy, Debug)]
struct Hit {
    /// index into the capture
    line: i64,
    /// characters before the match — feeds `cursor-right -N`
    char_start: CharOff,
    /// characters before the match END — the cursor seat
    char_end: CharOff,
    /// CELLS before the match — what `#{copy_cursor_x}` reports
    cell_start: CellOff,
    /// Byte span. Consumed only by `render_line`'s highlight; never emitted by
    /// the `rows` seam.
    byte_start: ByteOff,
    byte_end: ByteOff,
}

/// Every occurrence, in scrollback order. A line with three hits yields three
/// entries: the user picks an occurrence, not a line, and the jump is exact.
fn find_all(lines: &[Vec<u8>], re: &Regex, cap: usize) -> Vec<Hit> {
    let mut hits: Vec<Hit> = Vec::new();
    for (i, s) in lines.iter().enumerate() {
        // Empty lines are skipped before any regex work, so an empty-matching
        // pattern (`x*`, `^`, `()`) produces no hit at all on a blank capture
        // line where a naive port would produce one.
        if s.is_empty() {
            continue;
        }
        // One NUL-terminated copy per line; `find_from` then does the pointer
        // arithmetic the C++ `s.c_str() + off` does.
        let mut subject = s.clone();
        subject.push(0);

        let mut off = 0usize;
        // `<=`, not `<`: an empty-matching pattern gets one final scan against
        // the empty suffix, so it yields len+1 hits on a len-byte line.
        while off <= s.len() {
            let (b, e) = match re.find_from(&subject, off) {
                Some(span) => span,
                None => break,
            };
            hits.push(Hit {
                line: i as i64,
                byte_start: b,
                byte_end: e,
                char_start: utf8_chars(s, b),
                char_end: utf8_chars(s, e),
                cell_start: utf8_cells(s, b),
            });
            if hits.len() >= cap {
                // The cap is global, not per line: return immediately, mid-line,
                // without finishing this line or looking at any later one.
                return hits;
            }
            // A zero-width match (e.g. `^`, `x*`) would spin here forever. The
            // advance is +1 **byte**, not +1 character — `x*` on `中文` therefore
            // yields 7 hits at bytes 0..6, some landing inside a multi-byte
            // character. Faithful; the counters above round those up.
            off = if e == b { e.0 + 1 } else { e.0 };
        }
    }
    hits
}

// ── the jump ───────────────────────────────────────────────────────────────
//
// Measured on tmux 3.5a, because none of this behaves the way the command names
// suggest (probe scripts in records/.../assets/scripts/):
//
//   * `goto-line N` does NOT go to line N. It sets `oy`, the scroll offset from
//     the bottom of the history, and leaves the cursor row `cy` untouched:
//     absolute line = history_size - oy + cy. So the cursor row must be pinned
//     first — `history-top` puts it at cy=0 — and only then is
//     `goto-line (history_size - i)` an exact seek to line i.
//   * `oy` is NOT stable while the pane keeps printing; the index from the top
//     of history is. history_size is therefore re-read at jump time, not reused
//     from the capture.
//   * `search-forward` leaves the cursor one cell PAST the end of the match;
//     `search-backward` leaves it on the match START. seek's w/W/l/L read the
//     token under the cursor, so it has to be the backward one — which means
//     seating the cursor just past the chosen occurrence and searching back
//     onto it.
//   * Both searches WRAP silently when they fail, so a bad landing is not
//     reported by tmux. Hence the verification below.
//
// The whole sequence goes out as one tmux command list: the popup is still open
// while it runs (panes do not redraw under a popup) and the pane is only seen
// after this process exits and the popup closes.
//
// `run_ui`'s Enter branch is the only caller.
fn jump(pane: &[u8], pattern: &[u8], line: i64, char_end: CharOff, cell_start: CellOff) -> bool {
    let now = pane_geom(pane);
    if !now.ok {
        return false;
    }

    let p = || OsString::from_vec(pane.to_vec());
    let mut a: Vec<OsString> = vec![
        OsString::from("copy-mode"),
        OsString::from("-t"),
        p(),
        OsString::from(";"),
        OsString::from("send-keys"),
        OsString::from("-X"),
        OsString::from("-t"),
        p(),
        OsString::from("history-top"),
    ];

    let mut send = |args: &[OsString]| a.extend(args.iter().cloned());

    if line <= now.history_size {
        send(&[
            OsString::from(";"),
            OsString::from("send-keys"),
            OsString::from("-X"),
            OsString::from("-t"),
            p(),
            OsString::from("goto-line"),
            OsString::from((now.history_size - line).to_string()),
        ]);
    } else {
        // Target is in the visible screen: pin the viewport to the bottom and
        // step down to the row.
        let down = line - now.history_size;
        send(&[
            OsString::from(";"),
            OsString::from("send-keys"),
            OsString::from("-X"),
            OsString::from("-t"),
            p(),
            OsString::from("goto-line"),
            OsString::from("0"),
        ]);
        if down > 0 {
            send(&[
                OsString::from(";"),
                OsString::from("send-keys"),
                OsString::from("-X"),
                OsString::from("-N"),
                OsString::from(down.to_string()),
                OsString::from("-t"),
                p(),
                OsString::from("cursor-down"),
            ]);
        }
    }

    // `cursor-right` counts CHARACTERS, which is why char_end — not byte_end,
    // not cell_end — is the argument.
    if char_end.0 > 0 {
        send(&[
            OsString::from(";"),
            OsString::from("send-keys"),
            OsString::from("-X"),
            OsString::from("-N"),
            OsString::from(char_end.0.to_string()),
            OsString::from("-t"),
            p(),
            OsString::from("cursor-right"),
        ]);
    }

    // Registering the pattern with tmux is the point of this step, not just the
    // positioning: it is what makes the match highlight, `n`/`N`, and seek's
    // grab keys work afterwards.
    send(&[
        OsString::from(";"),
        OsString::from("send-keys"),
        OsString::from("-X"),
        OsString::from("-t"),
        p(),
        OsString::from("search-backward"),
        OsString::from_vec(pattern.to_vec()),
    ]);

    if !tmux_run(&a) {
        return false;
    }

    // Verify rather than assume: both searches wrap silently, so a miss lands
    // somewhere plausible instead of reporting itself.
    //
    // The check is positional, NOT textual. #{copy_cursor_line} looks like the
    // obvious probe and is a trap — measured on 3.5a it truncates at the first
    // wide character (a line reading "中文測試 aa999 尾巴" comes back as "中"),
    // so comparing text would false-alarm on every CJK line. Cursor coordinates
    // have no such problem.
    //
    // All five fields come from ONE call so they describe one instant, and
    // `history_size - scroll_position` is stable while the pane keeps printing:
    // copy-mode holds its view, so new output grows both terms together.
    let (s, ok) = tmux_out(&[
        OsStr::new("display-message"),
        OsStr::new("-p"),
        OsStr::new("-t"),
        os(pane),
        OsStr::new(
            "#{history_size}\t#{scroll_position}\t#{copy_cursor_y}\t#{copy_cursor_x}\t#{search_present}",
        ),
    ]);
    if !ok {
        return false;
    }
    let f = tab_fields::<5>(&s);
    let landed = f[0] - f[1] + f[2];
    // Note the asymmetry: the row is checked in LINE units, the column in CELL
    // units, even though the cursor was moved with a CHARACTER count.
    f[4] == 1 && landed == line && f[3] == cell_start.0 as i64
}

// ── terminal ───────────────────────────────────────────────────────────────
//
// Raw mode, its restore path, and the SIGWINCH handler that makes a resize
// redraw the popup at the new size (port spec §5.4 — the C++ cancels the search
// instead; that is the interrupt handling fixed in `read_byte`/`read_key`).
//
// The C++ keeps three file-scope globals (`g_saved`, `g_raw`, `g_resized`).
// Rust wants them behind a `Sync` type; the process is single-threaded and the
// only asynchronous writer is the signal handler, so an `AtomicBool` pair plus
// one write-once cell is the whole of it.

/// The termios to restore, written exactly once by `raw()`.
struct SavedTermios(std::cell::UnsafeCell<std::mem::MaybeUninit<libc::termios>>);

// SAFETY: sift spawns no threads, so the only concurrent context is the
// SIGWINCH handler — which touches `G_RESIZED` and nothing else. The cell is
// written by `raw()` before `G_RAW` is set and never again, and it is read only
// on a path that has just observed `G_RAW == true`, so no read can race the
// write and no read can see uninitialised memory.
unsafe impl Sync for SavedTermios {}

static G_SAVED: SavedTermios =
    SavedTermios(std::cell::UnsafeCell::new(std::mem::MaybeUninit::uninit()));
static G_RAW: AtomicBool = AtomicBool::new(false);
static G_RESIZED: AtomicBool = AtomicBool::new(false);

/// `volatile sig_atomic_t g_resized = 1`. An `AtomicBool` store is lock-free on
/// every target this ships to, which is what makes it legal in a handler.
extern "C" fn on_winch(_sig: libc::c_int) {
    G_RESIZED.store(true, Ordering::SeqCst);
}

/// Idempotent, exactly like the C++ `if (!g_raw) return;`.
///
/// Called on both normal exits (Esc and Enter), from `atexit`, and from the
/// panic hook. The `swap` is what makes those overlapping calls safe: only the
/// first one through does anything.
fn cooked() {
    if !G_RAW.swap(false, Ordering::SeqCst) {
        return;
    }
    // SAFETY: `G_RAW` was true, so `raw()` completed and the cell holds the
    // termios it captured. `tcsetattr` reads it and does not retain the pointer.
    unsafe {
        let saved = (*G_SAVED.0.get()).as_ptr();
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, saved);
    }
    // Show the cursor and leave the alternate screen.
    out(b"\x1b[?25h\x1b[?1049l");
}

/// `atexit` takes a plain `extern "C" fn()`; `cooked` is Rust-ABI.
extern "C" fn cooked_at_exit() {
    cooked();
}

/// Enter raw mode. **Not** `cfmakeraw`: exactly the flags the C++ clears and no
/// others — `IGNBRK`, `PARMRK`, `INPCK`, `IGNCR`, `IXANY` are left alone and
/// `CS8` is not forced. The key map depends on the ones that are cleared:
/// `ICRNL` off makes Enter arrive as byte 13, `ISIG` off turns `C-c` into a
/// key rather than a signal, and `OPOST` off is why every line break the
/// renderer emits is a literal `\r\n`.
fn raw() -> bool {
    let mut saved = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: `tcgetattr` fully initialises the struct on success; we only read
    // it on that branch.
    if unsafe { libc::tcgetattr(libc::STDIN_FILENO, saved.as_mut_ptr()) } != 0 {
        return false;
    }
    // SAFETY: tcgetattr returned 0.
    let saved = unsafe { saved.assume_init() };

    let mut t = saved;
    t.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG | libc::IEXTEN);
    t.c_iflag &= !(libc::IXON | libc::ICRNL | libc::INLCR | libc::BRKINT | libc::ISTRIP);
    t.c_oflag &= !libc::OPOST;
    // A blocking read returns after one byte; all timing is done by `poll`.
    t.c_cc[libc::VMIN] = 1;
    t.c_cc[libc::VTIME] = 0;
    // SAFETY: `t` is a fully initialised termios living across the call.
    if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &t) } != 0 {
        return false;
    }
    // SAFETY: single-threaded, and this is the only write to the cell. It
    // happens-before the `G_RAW` store below, which every reader observes first.
    unsafe { (*G_SAVED.0.get()).write(saved) };
    G_RAW.store(true, Ordering::SeqCst);
    out(b"\x1b[?1049h\x1b[2J");
    true
}

/// Terminal size, or 80×24. The query is on **stdout**, not stdin, and the
/// zero-guard is required — tmux reports 0 transiently. Never cached: `draw`
/// and the PgUp/PgDn handler each re-query.
fn term_size() -> (i32, i32) {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    // SAFETY: TIOCGWINSZ writes a winsize through the pointer; `ws` is one,
    // fully owned and live across the call.
    let rc = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) };
    if rc == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
        (ws.ws_col as i32, ws.ws_row as i32)
    } else {
        (80, 24)
    }
}

// ── key decoding ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Key {
    None,
    Enter,
    Esc,
    Backspace,
    Up,
    Down,
    PgUp,
    PgDn,
    Home,
    End,
    KillWord,
    KillLine,
    Text,
}

struct Input {
    key: Key,
    /// For `Key::Text`: one whole UTF-8 character — as **bytes**, unvalidated.
    /// Whatever arrived on the wire is what gets appended to the pattern.
    text: Vec<u8>,
}

/// A `poll`/`read` cut short by a signal: a SIGWINCH arrived, a byte did not.
/// Kept distinct from −1 so the caller can restart instead of reading it as a
/// keypress.
const READ_INTR: i32 = -2;

/// `errno == EINTR` for the call that just failed. Reads the same thread-local
/// `errno` the libc call set; no `unsafe` needed.
fn errno_is_intr() -> bool {
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR)
}

/// Read one byte with a timeout; −1 on timeout, EOF or error, `READ_INTR` when
/// a signal interrupted the wait.
///
/// Timeout, short read and EOF stay conflated as −1 — the caller turns a −1
/// first byte into `Key::Esc`, which is what makes a bare Escape cancel.
/// `EINTR` is deliberately NOT part of that conflation: `poll` is not restarted
/// after a handler runs (signal(7)), whatever `SA_RESTART` says, so a SIGWINCH
/// while we block surfaces here, and folding it into −1 is what used to cancel
/// the search on every popup resize — see `run_ui`.
fn read_byte(timeout_ms: libc::c_int) -> i32 {
    let mut p = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one live, fully initialised pollfd and a matching count of 1.
    let n = unsafe { libc::poll(&mut p, 1, timeout_ms) };
    if n < 0 {
        return if errno_is_intr() { READ_INTR } else { -1 };
    }
    if n == 0 {
        return -1; // timeout
    }
    let mut c: u8 = 0;
    // SAFETY: reading exactly 1 byte into a live single-byte local.
    let r = unsafe { libc::read(libc::STDIN_FILENO, &mut c as *mut u8 as *mut libc::c_void, 1) };
    if r < 0 && errno_is_intr() {
        return READ_INTR;
    }
    if r != 1 {
        return -1;
    }
    c as i32
}

/// `read_byte` for the bytes *inside* an escape sequence, where a signal must
/// not be allowed to truncate the sequence: `READ_INTR` restarts the wait. Only
/// the first byte of a key — the one `read_key` blocks on indefinitely — needs
/// to surface a resize, so it is the only caller of `read_byte` itself.
fn read_byte_seq(timeout_ms: libc::c_int) -> i32 {
    loop {
        let c = read_byte(timeout_ms);
        if c != READ_INTR {
            return c;
        }
    }
}

/// The 40 ms window is what separates a bare `Escape` (cancel) from an escape
/// *sequence* (a cursor key). Reproduced from the C++ verbatim.
const ESC_SEQ_MS: libc::c_int = 40;

fn read_key() -> Input {
    let mut inp = Input {
        key: Key::None,
        text: Vec::new(),
    };
    let c = read_byte(-1);
    if c == READ_INTR {
        // A signal, not a keypress. Report nothing: the caller's loop sees
        // `G_RESIZED`, redraws, and comes straight back here. Reporting
        // `Key::Esc` is what used to cancel the search on a resize.
        inp.key = Key::None;
        return inp;
    }
    if c < 0 {
        inp.key = Key::Esc;
        return inp;
    }

    match c {
        13 | 10 => {
            inp.key = Key::Enter;
            return inp;
        }
        127 | 8 => {
            inp.key = Key::Backspace;
            return inp;
        }
        23 => {
            inp.key = Key::KillWord; // C-w
            return inp;
        }
        21 => {
            inp.key = Key::KillLine; // C-u
            return inp;
        }
        3 | 7 => {
            inp.key = Key::Esc; // C-c / C-g
            return inp;
        }
        16 => {
            inp.key = Key::Up; // C-p
            return inp;
        }
        14 => {
            inp.key = Key::Down; // C-n
            return inp;
        }
        _ => {}
    }

    if c == 0x1b {
        // A bare Escape is cancel; an escape SEQUENCE is a cursor key. The only
        // thing separating them is timing, so give the rest of the sequence a
        // brief window to arrive.
        let c1 = read_byte_seq(ESC_SEQ_MS);
        if c1 < 0 {
            inp.key = Key::Esc;
            return inp;
        }
        if c1 == b'[' as i32 || c1 == b'O' as i32 {
            let c2 = read_byte_seq(ESC_SEQ_MS);
            if c2 < 0 {
                inp.key = Key::Esc;
                return inp;
            }
            match c2 as u8 {
                b'A' => inp.key = Key::Up,
                b'B' => inp.key = Key::Down,
                // Home/End arrive in several shapes. `ESC[H`/`ESC OH` and
                // `ESC[F`/`ESC OF` are the xterm cursor-key forms; tmux sends
                // the numeric `ESC[1~`/`ESC[4~`, and the rxvt family sends
                // `ESC[7~`/`ESC[8~`. All of them are accepted — the numeric
                // codes are unambiguous inside this decoder (nothing else here
                // reads a 1/4/7/8 parameter), so covering rxvt too costs
                // nothing. The C++ recognises only the four letter forms, and
                // so leaks the trailing `~` into the pattern.
                b'H' => inp.key = Key::Home,
                b'F' => inp.key = Key::End,
                // `~`-terminated CSI, exactly the PgUp/PgDn shape below: the
                // trailing byte is consumed and discarded, with no check that
                // it really is a `~`.
                b'1' | b'7' => {
                    let _ = read_byte_seq(ESC_SEQ_MS); // consume the '~'
                    inp.key = Key::Home;
                }
                b'4' | b'8' => {
                    let _ = read_byte_seq(ESC_SEQ_MS); // consume the '~'
                    inp.key = Key::End;
                }
                b'5' | b'6' => {
                    let c3 = read_byte_seq(ESC_SEQ_MS); // consume the '~'
                    let _ = c3;
                    inp.key = if c2 as u8 == b'5' {
                        Key::PgUp
                    } else {
                        Key::PgDn
                    };
                }
                _ => inp.key = Key::None, // unknown CSI: ignore
            }
            return inp;
        }
        inp.key = Key::None;
        return inp;
    }

    if c < 32 {
        inp.key = Key::None; // other control bytes: ignore
        return inp;
    }

    // A printable byte, possibly the head of a multi-byte character. The
    // continuation bytes are NOT validated — whatever arrives is appended, so a
    // lone 0x80 becomes a one-byte `Text` and lands in the pattern as-is.
    inp.key = Key::Text;
    inp.text.push(c as u8);
    let extra = if (c & 0xE0) == 0xC0 {
        1
    } else if (c & 0xF0) == 0xE0 {
        2
    } else if (c & 0xF8) == 0xF0 {
        3
    } else {
        0
    };
    for _ in 0..extra {
        let n = read_byte_seq(ESC_SEQ_MS);
        if n < 0 {
            break;
        }
        inp.text.push(n as u8);
    }
    inp
}

// ── rendering ──────────────────────────────────────────────────────────────

/// One `write(2)`, return value discarded — short writes are not retried, as in
/// the C++ `(void)r`. `write`, not `println!`: `OPOST` is cleared, so the frame
/// carries its own `\r\n`s and must not go through anything that adds a bare
/// `\n`.
fn out(s: &[u8]) {
    // SAFETY: `s` is a live slice; `write` reads at most `s.len()` bytes from it
    // and retains nothing.
    let r = unsafe { libc::write(libc::STDOUT_FILENO, s.as_ptr() as *const libc::c_void, s.len()) };
    let _ = r;
}

/// Render one capture line into `width` cells, guaranteeing the match is on
/// screen (long lines scroll horizontally so a hit at column 400 is still seen)
/// and reverse-videoing the matched span.
///
/// Note the units, which do not collapse: the highlight span arrives in
/// **bytes** (`regexec`'s coordinate, tested against each character's first
/// byte) while the width, the scroll and the cut are in **cells**.
///
/// The `12` and the `width / 3` are exact magic numbers, integer division
/// included. The head ellipsis is `…` counted as one cell; there is deliberately
/// no tail ellipsis — long lines just stop.
fn render_line(s: &[u8], byte_start: ByteOff, byte_end: ByteOff, width: i32) -> Vec<u8> {
    if width <= 0 {
        return Vec::new();
    }

    // Cell offset of the match start.
    let mut cells_to_start: i32 = 0;
    let mut i = 0usize;
    while i < s.len() && i < byte_start.0 {
        let (cp, len) = utf8_decode(s, i);
        cells_to_start += cell_width(cp) as i32;
        i += len;
    }

    // Scroll so the match sits about a third in, but never past the line start.
    let mut skip_cells: i32 = 0;
    if cells_to_start > width - 12 {
        skip_cells = cells_to_start - width / 3;
    }
    if skip_cells < 0 {
        skip_cells = 0;
    }

    let mut o: Vec<u8> = Vec::new();
    let mut cells: i32 = 0;
    let mut seen: i32 = 0;
    let mut inverted = false;
    if skip_cells > 0 {
        o.extend_from_slice("…".as_bytes()); // U+2026: 3 bytes, 1 cell
        cells = 1;
    }

    let mut i = 0usize;
    while i < s.len() {
        let (cp, len) = utf8_decode(s, i);
        let w = cell_width(cp) as i32;
        // A zero-width character exactly at the boundary (`seen == skip_cells`)
        // is dropped, and `cells + w > width` never breaks on one either.
        if seen + w > skip_cells {
            if cells + w > width {
                break;
            }
            let want = i >= byte_start.0 && i < byte_end.0;
            if want && !inverted {
                o.extend_from_slice(b"\x1b[7m");
                inverted = true;
            } else if !want && inverted {
                o.extend_from_slice(b"\x1b[27m");
                inverted = false;
            }
            // `len` never runs past the end (the decoder only returns 2..4 when
            // that many bytes are available), so this mirrors the C++
            // `o.append(s, i, len)` including its clamp.
            let end = if i + len < s.len() { i + len } else { s.len() };
            o.extend_from_slice(&s[i..end]);
            cells += w;
        }
        seen += w;
        i += len;
    }
    // The invert is left with ESC[27m, not ESC[0m, so surrounding attributes
    // survive — including the selected row's unclosed bold.
    if inverted {
        o.extend_from_slice(b"\x1b[27m");
    }
    o
}

// ── the popup ──────────────────────────────────────────────────────────────

struct Ui {
    pane: Vec<u8>,
    lines: Vec<Vec<u8>>,
    geom: Geom,
    /// Bytes, edited as bytes: `C-w` splits on the ASCII space only, and
    /// backspace strips UTF-8 continuation bytes before deleting one more.
    pattern: Vec<u8>,
    hits: Vec<Hit>,
    sel: usize,
    /// first visible hit
    top: usize,
    bad_re: bool,
    capped: bool,
    re_error: String,
}

fn refilter(u: &mut Ui) {
    u.hits.clear();
    u.capped = false;
    u.bad_re = false;
    u.re_error.clear();
    if u.pattern.is_empty() {
        u.sel = 0;
        u.top = 0;
        return;
    }

    let re = match Regex::compile(&u.pattern) {
        Ok(re) => re,
        Err(msg) => {
            // Half-typed patterns are invalid most of the time. Keeping the
            // previous result set on screen would be a lie about what the
            // pattern matches, so the list empties but the header says why.
            u.bad_re = true;
            u.re_error = msg;
            return;
        }
    };
    u.hits = find_all(&u.lines, &re, MATCH_CAP);
    u.capped = u.hits.len() >= MATCH_CAP;

    // Default to the match nearest the bottom — the same "most recent first"
    // bias as the search-backward binding this replaces. `sel` is recomputed
    // from scratch on every pattern change; no attempt is made to preserve the
    // user's position across a refilter.
    u.sel = if u.hits.is_empty() {
        0
    } else {
        u.hits.len() - 1
    };
    u.top = 0;
}

fn draw(u: &mut Ui) {
    let (w, h) = term_size();
    // Too small to draw: emit NOTHING. The previous frame (or, at startup, the
    // blank alternate screen) stays, and keys still work.
    if h < 4 || w < 20 {
        return;
    }

    let mut list_rows = h - 2; // header + footer
    if list_rows < 1 {
        list_rows = 1;
    }
    let rows = list_rows as usize;

    // Keep the selection in view.
    if u.sel < u.top {
        u.top = u.sel;
    }
    if u.sel >= u.top + rows {
        u.top = u.sel - rows + 1;
    }
    if u.hits.len() <= rows {
        u.top = 0;
    }

    let mut o: Vec<u8> = b"\x1b[H\x1b[2J".to_vec();

    // Header: prompt on the left, status on the right.
    let mut status: Vec<u8> = Vec::new();
    if u.bad_re {
        status.extend_from_slice(b"invalid regex: ");
        status.extend_from_slice(u.re_error.as_bytes());
    } else if u.pattern.is_empty() {
        status.extend_from_slice(b"type an extended regex");
    } else if u.hits.is_empty() {
        status.extend_from_slice(b"no match");
    } else {
        // No pluralisation: a single hit renders as `1 matches`.
        status.extend_from_slice(u.hits.len().to_string().as_bytes());
        status.extend_from_slice(if u.capped {
            b"+ matches (capped)".as_slice()
        } else {
            b" matches".as_slice()
        });
    }
    if u.geom.alternate {
        let mut warned: Vec<u8> = "⚠ visible screen only · ".as_bytes().to_vec();
        warned.extend_from_slice(&status);
        status = warned;
    }

    // CHARACTERS, not cells, in both lengths — which is also why a pattern with
    // wide characters parks the cursor one column too far left per wide
    // character (§7.4). Faithful.
    let plen = 7 + utf8_chars(&u.pattern, ByteOff(u.pattern.len())).0 as i32;
    let slen = utf8_chars(&status, ByteOff(status.len())).0 as i32;
    o.extend_from_slice(b"\x1b[1mregex> ");
    o.extend_from_slice(&u.pattern);
    o.extend_from_slice(b"\x1b[0m");
    let gap = w - plen - slen;
    // If it does not fit the status is dropped entirely — never truncated.
    if gap > 0 {
        o.extend(std::iter::repeat(b' ').take(gap as usize));
        o.extend_from_slice(b"\x1b[2m");
        o.extend_from_slice(&status);
        o.extend_from_slice(b"\x1b[0m");
    }
    o.extend_from_slice(b"\r\n");

    // Widest line number, computed from the CAPTURE size rather than from the
    // hits, so the text column does not jitter as the pattern changes.
    let mut numw = 1i32;
    let mut v: i64 = if u.lines.is_empty() {
        0
    } else {
        u.lines.len() as i64 - 1
    };
    while v >= 10 {
        numw += 1;
        v /= 10;
    }

    for r in 0..list_rows {
        let idx = u.top + r as usize;
        if idx >= u.hits.len() {
            o.extend_from_slice(b"\r\n"); // trailing empty rows are normal
            continue;
        }
        let hit = u.hits[idx];
        let cur = idx == u.sel;
        // The selected row's bold is never closed before the end of the row, so
        // the whole row — dim line number included — renders bold.
        o.extend_from_slice(if cur {
            b"\x1b[1m> ".as_slice()
        } else {
            b"  ".as_slice()
        });
        o.extend_from_slice(b"\x1b[2m");
        // `%*ld`: right-aligned, space-padded.
        o.extend_from_slice(format!("{:>width$}", hit.line, width = numw as usize).as_bytes());
        o.extend_from_slice(b"\x1b[22m ");
        let text_w = w - numw - 3;
        let line: &[u8] = u
            .lines
            .get(hit.line as usize)
            .map(|v| v.as_slice())
            .unwrap_or(b"");
        o.extend_from_slice(&render_line(line, hit.byte_start, hit.byte_end, text_w));
        o.extend_from_slice(b"\x1b[0m\r\n");
    }

    o.extend_from_slice(
        "\x1b[2m↑↓ select  Enter jump  Esc cancel  C-w word  C-u clear\x1b[0m".as_bytes(),
    );

    // Park the real cursor at the end of the pattern so typing looks normal.
    // No trailing \r\n after the footer.
    o.extend_from_slice(format!("\x1b[1;{}H\x1b[?25h", plen + 1).as_bytes());
    out(&o);
}

fn run_ui(pane: &[u8]) {
    let mut u = Ui {
        pane: pane.to_vec(),
        lines: Vec::new(),
        geom: Geom::default(),
        pattern: Vec::new(),
        hits: Vec::new(),
        sel: 0,
        top: 0,
        bad_re: false,
        capped: false,
        re_error: String::new(),
    };
    u.geom = pane_geom(&u.pane);
    if !u.geom.ok {
        let mut m = b"sift: cannot read pane ".to_vec();
        m.extend_from_slice(&u.pane);
        say(&m);
        return;
    }
    u.lines = capture(&u.pane, &u.geom);
    if u.lines.is_empty() {
        let mut m = b"sift: nothing to search in ".to_vec();
        m.extend_from_slice(&u.pane);
        say(&m);
        return;
    }

    if !raw() {
        say(b"sift: no terminal (run it from a tmux popup)");
        return;
    }

    // `atexit` is what the C++ registers, and it covers exactly the normal
    // returns. It does NOT cover a panic, and neither would a `Drop` guard: the
    // release profile sets `panic = "abort"`, so there is no unwinding and
    // `abort()` does not run `atexit` handlers — a panic would hand the user
    // back a terminal in raw mode with no echo, still on the alternate screen.
    // The panic hook is the missing half; it runs before the abort. `cooked` is
    // idempotent, so the two registrations cannot fight.
    //
    // SAFETY: `cooked_at_exit` is a plain extern "C" fn with no state of its
    // own and is valid for the life of the process.
    let _ = unsafe { libc::atexit(cooked_at_exit) };
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        cooked();
        prev_hook(info);
    }));

    // The one signal handler installed. There is deliberately no SIGTERM/SIGHUP
    // restore: `atexit` handlers do not run on death by signal, and matching the
    // C++'s coverage is the point (port spec §9 hazard 14).
    //
    // SAFETY: `on_winch` is an extern "C" fn valid for the life of the process
    // and touches only one atomic, so it is async-signal-safe.
    unsafe { libc::signal(libc::SIGWINCH, on_winch as *const () as libc::sighandler_t) };

    draw(&mut u);
    loop {
        let inp = read_key();
        // The resize path. `poll` is never restarted after a handler runs
        // (signal(7)), whatever SA_RESTART says, so a SIGWINCH while we block
        // above gives EINTR → `read_byte` `READ_INTR` → `read_key` `Key::None`,
        // and we arrive here with the flag set. `draw` re-queries `term_size`
        // and repaints the whole alternate screen (`\x1b[H\x1b[2J`) without
        // leaving it, so the popup comes back at its new size and the pattern
        // typed so far survives. The C++ reports `Esc` instead and exits, which
        // left this branch unreachable in practice.
        if G_RESIZED.swap(false, Ordering::SeqCst) {
            draw(&mut u);
        }

        match inp.key {
            Key::Esc => {
                cooked();
                return; // cancel: pane untouched
            }

            Key::Enter => {
                if !u.hits.is_empty() {
                    let hit = u.hits[u.sel];
                    let pattern = u.pattern.clone();
                    let line = hit.line;
                    let cend = hit.char_end;
                    let cstart = hit.cell_start;
                    cooked(); // restore before the pane redraws
                    if !jump(&u.pane, &pattern, line, cend, cstart) {
                        let mut m = b"sift: the pane moved \xe2\x80\x94 landed on the nearest match of /"
                            .to_vec();
                        m.extend_from_slice(&pattern);
                        m.push(b'/');
                        say(&m);
                    }
                    return;
                }
            }

            // No wrapping anywhere: Up at the first hit and Down at the last are
            // no-ops.
            Key::Up => {
                if u.sel > 0 {
                    u.sel -= 1;
                }
            }
            Key::Down => {
                if u.sel + 1 < u.hits.len() {
                    u.sel += 1;
                }
            }
            Key::Home => u.sel = 0,
            Key::End => {
                if !u.hits.is_empty() {
                    u.sel = u.hits.len() - 1;
                }
            }

            Key::PgUp | Key::PgDn => {
                // Re-query, never cached. The page step is one row smaller than
                // the visible list (`h - 3` against `h - 2` rows), giving a
                // one-row overlap between pages.
                let (_w, h) = term_size();
                let step = if h > 4 { (h - 3) as usize } else { 1 };
                if inp.key == Key::PgUp {
                    // Strict `>`: a `sel` exactly equal to `step` clamps to 0.
                    u.sel = if u.sel > step { u.sel - step } else { 0 };
                } else if !u.hits.is_empty() {
                    u.sel = std::cmp::min(u.sel + step, u.hits.len() - 1);
                }
            }

            Key::Backspace => {
                if !u.pattern.is_empty() {
                    // Delete one whole UTF-8 character: strip continuation
                    // bytes, then one more byte. A malformed trailing sequence
                    // can leave this deleting a single byte.
                    let mut i = u.pattern.len();
                    while i > 0 && (u.pattern[i - 1] & 0xC0) == 0x80 {
                        i -= 1;
                    }
                    if i > 0 {
                        i -= 1;
                    }
                    u.pattern.truncate(i);
                    refilter(&mut u);
                }
            }

            Key::KillWord => {
                // ASCII space only — not tabs, not punctuation, not CJK spacing.
                let mut i = u.pattern.len();
                while i > 0 && u.pattern[i - 1] == b' ' {
                    i -= 1;
                }
                while i > 0 && u.pattern[i - 1] != b' ' {
                    i -= 1;
                }
                u.pattern.truncate(i);
                refilter(&mut u);
            }

            Key::KillLine => {
                u.pattern.clear();
                refilter(&mut u);
            }

            Key::Text => {
                u.pattern.extend_from_slice(&inp.text);
                refilter(&mut u);
            }

            Key::None => {}
        }
        // `draw` runs after every iteration, including on `Key::None`.
        draw(&mut u);
    }
}

// ── headless seam ──────────────────────────────────────────────────────────

fn run_rows(pane: &[u8], pattern: &[u8]) {
    let g = pane_geom(pane);
    if !g.ok {
        // Silently, with no message of any kind from sift — only tmux's
        // inherited stderr. Matches the C++ `if (!g.ok) return 0;`.
        return;
    }
    let lines = capture(pane, &g);

    let re = match Regex::compile(pattern) {
        Ok(re) => re,
        Err(msg) => {
            eprintln!("sift: invalid regex: {msg}");
            return; // still exit 0 — see invariants
        }
    };
    let hits = find_all(&lines, &re, MATCH_CAP);

    // Raw bytes, one write per row, through a buffered handle: the text field is
    // the capture line unmodified, un-escaped and un-truncated, and it may not be
    // valid UTF-8. Write errors are ignored — nothing here may fail the process.
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::new(stdout.lock());
    for h in &hits {
        let text: &[u8] = lines
            .get(h.line as usize)
            .map(|v| v.as_slice())
            .unwrap_or(b"");
        let _ = write!(
            w,
            "{}\t{}\t{}\t{}\t",
            h.line, h.char_start.0, h.char_end.0, h.cell_start.0
        );
        let _ = w.write_all(text);
        let _ = w.write_all(b"\n");
    }
    let _ = w.flush();
}

// ── entry point ────────────────────────────────────────────────────────────

fn main() {
    // Load-bearing, and the first statement in the C++ `main` for the same
    // reason: without it glibc runs in the "C" locale, `wcwidth` returns −1 for
    // every non-ASCII code point, and the regex engine switches to byte-wise
    // matching (`.` would match one byte of a three-byte character).
    //
    // SAFETY: called once, before any thread is spawned and before any locale-
    // dependent call. The empty locale name is a static NUL-terminated literal.
    unsafe { libc::setlocale(libc::LC_ALL, b"\0".as_ptr() as *const libc::c_char) };

    // `args_os`, not `args`: `args` panics on an argument that is not valid
    // UTF-8, and the regex is an arbitrary byte string.
    let argv: Vec<Vec<u8>> = std::env::args_os().map(|a| a.as_bytes().to_vec()).collect();

    if argv.len() >= 2 && argv[1] == b"rows" {
        // Exactly `strcmp(argv[1], "rows") == 0`; anything else, including a
        // string that merely starts with `rows`, is a pane id.
        if argv.len() < 4 {
            eprintln!("usage: sift rows <pane-id> <regex>");
            return;
        }
        // Arguments beyond argv[3] are silently ignored.
        run_rows(&origin_pane(Some(&argv[2])), &argv[3]);
        return;
    }

    let pane = origin_pane(argv.get(1).map(|v| v.as_slice()));
    if pane.is_empty() {
        eprintln!("sift: no pane — run it inside tmux, or pass a pane id");
        return;
    }

    run_ui(&pane);
}
