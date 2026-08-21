//! avy — avy-goto-char-timer for a tmux pane, bound at `prefix Space`.
//!
//! Interaction (chosen by the user, 2026-08-21): type any number of characters
//! to narrow matches on the visible screen; after a pause (`@avy-timeout` ms,
//! default 500) single-key labels appear over the matches; pressing a label
//! moves the copy-mode cursor there. Exactly one match jumps immediately.
//! Enter forces labels early, Backspace edits, Escape / C-g / C-c cancel.
//! The jump leaves the pane in copy-mode, so seek's w/W/l/L grab keys chain
//! directly after a jump.
//!
//! Mechanics: `avy launch <pane>` opens a borderless popup exactly covering
//! the pane (`display-popup -B -x '#{popup_pane_left}' -y '#{popup_pane_top}'`
//! — the popup_pane_* format variables exist precisely for pane alignment, and
//! panes do not update while a popup is open, so the snapshot cannot go
//! stale). Inside the popup, `avy ui <pane>` re-renders a `capture-pane -e`
//! snapshot, reads raw keys (via `stty`, no termios dependency), and finally
//! drives the target pane's copy-mode cursor.
//!
//! Cursor positioning, all measured on tmux 3.5a (2026-08-21, throwaway
//! server; see records/…-tmux-avy/assets/scripts/measure-*.sh):
//!   - `cursor-right` steps per CHARACTER (a double-width CJK char is one
//!     step), and walks across wrapped-row boundaries within a logical line.
//!   - Crossing each wrap boundary costs exactly ONE extra step: the cursor
//!     visits a phantom end-of-row position (x = that row's own cell end —
//!     40 on a full 40-col row, 39 when a wide char wrapped early).
//!   - `start-of-line` moves to the start of the LOGICAL line, scrolling the
//!     view up when the line begins above the viewport.
//!   - `top-line` moves to the viewport's top row, preserving the column.
//!   - `goto-line N` counts lines bottom-up and also preserves the column
//!     (measured, unused here).
//! Hence a jump is: top-line → N×cursor-down (screen rows) → start-of-line →
//! (char offset + wrap rows crossed)×cursor-right.
//!
//! Like seek, this tool never writes pane options or titles and reports only
//! via display-message -l, so raw pane text never enters a format context and
//! sanitize_format() is deliberately not involved.

use std::io::{Read, Write};
use std::time::Instant;

use tmuxlib as t;
use unicode_width::UnicodeWidthChar;

const DEFAULT_KEYS: &str = "asdfghjkl";
const DEFAULT_TIMEOUT_MS: u64 = 500;
const SGR_MATCH: &str = "\x1b[7m"; // reverse video
const SGR_LABEL: &str = "\x1b[1;97;41m"; // bold bright-white on red

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(msg) = run(&args) {
        t::message_literal(&format!("avy: {msg}"));
    }
    // Deliberately no non-zero exit on any path: a failing binding surfaces as
    // a tmux error popup (tools/ARCHITECTURE.org, "Conventions").
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("launch") => launch(args.get(1).ok_or("launch: missing pane id")?),
        Some("ui") => ui(args.get(1).ok_or("ui: missing pane id")?),
        _ => Err("usage: avy launch <pane-id> | avy ui <pane-id>".into()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The pure half — no tmux, no processes, all unit-tested below.
// ─────────────────────────────────────────────────────────────────────────────

/// Cell position of the char at `off` (a char index) in a logical line
/// rendered at `width` cells: (wrapped-row index within the line, cell column
/// in that row). Wrap rule measured on tmux 3.5a: a row breaks when the next
/// char would exceed the width (`col + w > width`), so a double-width char at
/// the last cell wraps early and leaves that row one cell short.
/// `off == char count` yields the position just past the last char.
fn locate(line: &str, width: usize, off: usize) -> (usize, usize) {
    let width = width.max(1);
    let mut row = 0usize;
    let mut col = 0usize;
    for (i, ch) in line.chars().enumerate() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w > 0 && col + w > width {
            row += 1;
            col = 0;
        }
        if i == off {
            return (row, col);
        }
        col += w;
    }
    (row, col)
}

/// Screen rows a logical line occupies at `width` cells (empty line = 1).
fn rows_for(line: &str, width: usize) -> usize {
    locate(line, width, line.chars().count()).0 + 1
}

/// `cursor-right` steps from start-of-line to the char at `off`.
/// Measured on tmux 3.5a: per-char stepping plus exactly one phantom
/// end-of-row step at each wrap boundary crossed — i.e. off + wrapped-row
/// index of the target char.
fn steps_to(line: &str, width: usize, off: usize) -> usize {
    off + locate(line, width, off).0
}

/// A match: `line` indexes the joined viewport lines, `off`/`len` are char
/// units within that (possibly top-truncated) line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Hit {
    line: usize,
    off: usize,
    len: usize,
}

fn ci_eq(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// All occurrences of `query` in `lines`. Smart-case: a query containing any
/// uppercase char matches exactly; otherwise matching is case-insensitive.
fn find_hits(lines: &[String], query: &str) -> Vec<Hit> {
    let q: Vec<char> = query.chars().collect();
    if q.is_empty() {
        return Vec::new();
    }
    let sensitive = q.iter().any(|c| c.is_uppercase());
    let mut hits = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        if chars.len() < q.len() {
            continue;
        }
        for start in 0..=(chars.len() - q.len()) {
            let ok = (0..q.len()).all(|k| {
                if sensitive {
                    chars[start + k] == q[k]
                } else {
                    ci_eq(chars[start + k], q[k])
                }
            });
            if ok {
                hits.push(Hit { line: li, off: start, len: q.len() });
            }
        }
    }
    hits
}

/// Label strings for `n` hits: single keys while they last, uniform two-key
/// sequences beyond that (capacity keys²; callers truncate to that first).
fn assign_labels(n: usize, keys: &[char]) -> Vec<String> {
    if n <= keys.len() {
        return keys.iter().take(n).map(|c| c.to_string()).collect();
    }
    let mut v = Vec::with_capacity(n);
    'outer: for a in keys {
        for b in keys {
            v.push(format!("{a}{b}"));
            if v.len() == n {
                break 'outer;
            }
        }
    }
    v
}

/// The viewport capture may start mid-line (a wrapped line straddling the top
/// edge). Given the same line joined from the start of history (`full`) and
/// the visible suffix, return how many chars are hidden above the viewport.
/// None = the two captures disagree — treat the top line as unjumpable.
fn hidden_prefix_chars(full: &str, visible: &str) -> Option<usize> {
    if full == visible {
        Some(0)
    } else if full.ends_with(visible) {
        Some(full.chars().count() - visible.chars().count())
    } else {
        None
    }
}

/// `@avy-timeout` (milliseconds) → stty VTIME deciseconds, clamped to 1..=255.
fn timeout_deciseconds(opt: &str) -> u8 {
    let ms = opt.trim().parse::<u64>().unwrap_or(DEFAULT_TIMEOUT_MS);
    ((ms + 50) / 100).clamp(1, 255) as u8
}

/// `@avy-keys` → label key set (deduplicated, order kept); default home row.
fn label_keys(opt: &str) -> Vec<char> {
    let src = if opt.trim().is_empty() { DEFAULT_KEYS } else { opt.trim() };
    let mut keys = Vec::new();
    for c in src.chars() {
        if !c.is_whitespace() && !keys.contains(&c) {
            keys.push(c);
        }
    }
    if keys.is_empty() {
        keys = DEFAULT_KEYS.chars().collect();
    }
    keys
}

/// The command batch that moves the target pane's copy-mode cursor to a
/// position `vrow` viewport rows below the top, `steps` cursor-rights after
/// the containing logical line's start (see module doc for the measured
/// semantics of each primitive).
fn plan_jump(pane: &str, in_mode: bool, vrow: usize, steps: usize) -> Vec<Vec<String>> {
    let mk = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    let mut batch = Vec::new();
    if !in_mode {
        batch.push(mk(&["copy-mode", "-t", pane]));
    }
    batch.push(mk(&["send-keys", "-t", pane, "-X", "top-line"]));
    if vrow > 0 {
        batch.push(mk(&["send-keys", "-t", pane, "-X", "-N", &vrow.to_string(), "cursor-down"]));
    }
    batch.push(mk(&["send-keys", "-t", pane, "-X", "start-of-line"]));
    if steps > 0 {
        batch.push(mk(&["send-keys", "-t", pane, "-X", "-N", &steps.to_string(), "cursor-right"]));
    }
    batch
}

/// The visible screen as joined logical lines plus what jump math needs.
struct Viewport {
    /// Joined logical lines of the viewport; lines[0] may be only the visible
    /// suffix of a line that starts above the top edge.
    lines: Vec<String>,
    /// Chars of lines[0] hidden above the viewport; None = recovery failed,
    /// hits on lines[0] are excluded rather than jumped wrong (fail closed).
    hidden: Option<usize>,
    /// The full logical first line (== lines[0] when hidden == Some(0)).
    full_first: String,
    width: usize,
    height: usize,
}

impl Viewport {
    fn build(lines: Vec<String>, full: &[String], width: usize, height: usize) -> Viewport {
        let (hidden, full_first) = match lines.first() {
            None => (Some(0), String::new()),
            Some(first) if full.len() >= lines.len() => {
                let cand = &full[full.len() - lines.len()];
                match hidden_prefix_chars(cand, first) {
                    Some(h) => (Some(h), cand.clone()),
                    None => (None, first.clone()),
                }
            }
            Some(first) => (None, first.clone()),
        };
        Viewport { lines, hidden, full_first, width, height }
    }

    /// First viewport row of lines[li] (of its visible part, for li == 0).
    fn row_offset(&self, li: usize) -> usize {
        self.lines[..li].iter().map(|l| rows_for(l, self.width)).sum()
    }

    /// Viewport (row, cell-column) of the char at `off` in lines[li].
    fn pos(&self, hit_line: usize, off: usize) -> (usize, usize) {
        let (r, c) = locate(&self.lines[hit_line], self.width, off);
        (self.row_offset(hit_line) + r, c)
    }

    /// (vrow, steps) for plan_jump, or None when the hit is not safely
    /// jumpable (top-line recovery failed, or it fell below the viewport).
    fn jump_params(&self, hit: &Hit) -> Option<(usize, usize)> {
        if self.pos(hit.line, hit.off).0 >= self.height {
            return None;
        }
        if hit.line == 0 {
            let hidden = self.hidden?;
            Some((0, steps_to(&self.full_first, self.width, hidden + hit.off)))
        } else {
            Some((self.row_offset(hit.line), steps_to(&self.lines[hit.line], self.width, hit.off)))
        }
    }

    /// Hits that are visible and jumpable.
    fn usable(&self, hits: Vec<Hit>) -> Vec<Hit> {
        hits.into_iter().filter(|h| self.jump_params(h).is_some()).collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The tmux half.
// ─────────────────────────────────────────────────────────────────────────────

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Open a borderless popup exactly covering the pane, running `avy ui` in it.
fn launch(pane: &str) -> Result<(), String> {
    let out = t::display(Some(pane), "#{pane_width}\t#{pane_height}")?;
    let mut it = out.split('\t');
    let w = it.next().unwrap_or("");
    let h = it.next().ok_or("cannot read pane geometry")?;
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let cmd = format!("exec {} ui {}", sh_quote(&exe.to_string_lossy()), sh_quote(pane));
    t::tmux([
        "display-popup",
        "-B",
        "-t",
        pane,
        "-x",
        "#{popup_pane_left}",
        "-y",
        "#{popup_pane_top}",
        "-w",
        w,
        "-h",
        h,
        "-E",
        &cmd,
    ])?;
    Ok(())
}

struct PaneState {
    in_mode: bool,
    scroll: i64,
    width: usize,
    height: usize,
}

fn read_state(pane: &str) -> Result<PaneState, String> {
    let out = t::display(
        Some(pane),
        "#{pane_in_mode}\t#{scroll_position}\t#{pane_width}\t#{pane_height}",
    )?;
    let f: Vec<&str> = out.split('\t').collect();
    if f.len() < 4 {
        return Err("cannot read pane state".into());
    }
    Ok(PaneState {
        in_mode: f[0] == "1",
        scroll: f[1].parse().unwrap_or(0),
        width: f[2].parse().map_err(|_| "bad pane_width")?,
        height: f[3].parse().map_err(|_| "bad pane_height")?,
    })
}

/// capture-pane over the copy-mode viewport (range shifted by the scroll
/// position, same arithmetic as seek). `joined` adds -J (logical lines,
/// trailing spaces preserved), `escapes` adds -e (SGR sequences for faithful
/// re-rendering), `from_top` captures from the start of history instead of
/// the viewport top (straddler recovery).
fn capture(pane: &str, st: &PaneState, joined: bool, escapes: bool, from_top: bool) -> Result<Vec<String>, String> {
    let top = (-st.scroll).to_string();
    let bottom = (st.height as i64 - 1 - st.scroll).to_string();
    let mut args: Vec<&str> = vec!["capture-pane", "-p", "-t", pane];
    if joined {
        args.push("-J");
    }
    if escapes {
        args.push("-e");
    }
    let top_arg: &str = if from_top { "-" } else { top.as_str() };
    args.push("-S");
    args.push(top_arg);
    args.push("-E");
    args.push(bottom.as_str());
    Ok(t::tmux(args)?.split('\n').map(str::to_string).collect())
}

fn global_option(name: &str) -> String {
    t::tmux(["show-options", "-gqv", name]).unwrap_or_default()
}

fn stty(args: &[&str]) {
    let _ = std::process::Command::new("stty")
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[derive(Debug, PartialEq, Eq)]
enum Key {
    Ch(char),
    Enter,
    Backspace,
    Escape,
    Cancel,
    Timeout,
    Ignore,
}

struct Keyboard {
    buf: Vec<u8>,
}

impl Keyboard {
    fn new() -> Keyboard {
        Keyboard { buf: Vec::new() }
    }

    fn fill(&mut self) -> usize {
        let mut tmp = [0u8; 64];
        match std::io::stdin().read(&mut tmp) {
            Ok(n) => {
                self.buf.extend_from_slice(&tmp[..n]);
                n
            }
            Err(_) => 0,
        }
    }

    /// One key. `timeout_ds` = Some(VTIME) arms the avy timer (a read that
    /// returns nothing within that many deciseconds yields Timeout); None
    /// blocks until a byte arrives (EOF also yields Timeout — callers treat
    /// a Timeout in a blocking phase as cancel).
    fn next(&mut self, timeout_ds: Option<u8>) -> Key {
        if self.buf.is_empty() {
            match timeout_ds {
                Some(ds) => stty(&["raw", "-echo", "min", "0", "time", &ds.to_string()]),
                None => stty(&["raw", "-echo", "min", "1", "time", "0"]),
            }
            if self.fill() == 0 {
                return Key::Timeout;
            }
        }
        let b = self.buf.remove(0);
        match b {
            0x0d | 0x0a => Key::Enter,
            0x7f | 0x08 => Key::Backspace,
            0x03 | 0x07 => Key::Cancel, // C-c, C-g
            0x1b => {
                if self.buf.is_empty() {
                    // Escape sequences arrive as a burst; a lone ESC stays lone.
                    stty(&["raw", "-echo", "min", "0", "time", "1"]);
                    self.fill();
                }
                if self.buf.is_empty() {
                    Key::Escape
                } else {
                    self.buf.clear(); // arrow/function key etc — ignore whole burst
                    Key::Ignore
                }
            }
            b if b < 0x20 => Key::Ignore,
            b if b < 0x80 => Key::Ch(b as char),
            b => {
                // UTF-8 lead byte: gather the continuation bytes.
                let need = if b >= 0xf0 {
                    3
                } else if b >= 0xe0 {
                    2
                } else {
                    1
                };
                while self.buf.len() < need {
                    stty(&["raw", "-echo", "min", "0", "time", "1"]);
                    if self.fill() == 0 {
                        break;
                    }
                }
                if self.buf.len() < need {
                    return Key::Ignore;
                }
                let mut bytes = vec![b];
                bytes.extend(self.buf.drain(..need));
                match std::str::from_utf8(&bytes) {
                    Ok(s) => s.chars().next().map_or(Key::Ignore, Key::Ch),
                    Err(_) => Key::Ignore,
                }
            }
        }
    }
}

/// Repaint: the captured screen verbatim, then styled overlays at absolute
/// cell positions (rows/cols 0-based here, 1-based in the CUP sequence).
fn render(base: &[String], height: usize, overlays: &[(usize, usize, String, &str)]) {
    let mut out = String::from("\x1b[?25l\x1b[0m\x1b[2J\x1b[H");
    for (i, line) in base.iter().take(height).enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        out.push_str(line);
        out.push_str("\x1b[0m");
    }
    for (row, col, text, sgr) in overlays {
        out.push_str(&format!("\x1b[{};{}H{}{}\x1b[0m", row + 1, col + 1, sgr, text));
    }
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(out.as_bytes());
    let _ = stdout.flush();
}

/// Per-char reverse-video overlay for every hit (walking char by char keeps
/// wrap-spanning matches correct).
fn match_overlays(vp: &Viewport, hits: &[Hit]) -> Vec<(usize, usize, String, &'static str)> {
    let mut ov = Vec::new();
    for h in hits {
        let chars: Vec<char> = vp.lines[h.line].chars().collect();
        for k in 0..h.len {
            let (row, col) = vp.pos(h.line, h.off + k);
            if row < vp.height {
                ov.push((row, col, chars[h.off + k].to_string(), SGR_MATCH));
            }
        }
    }
    ov
}

enum Phase {
    Filter,
    Label { labels: Vec<String>, typed: String },
}

fn ui(pane: &str) -> Result<(), String> {
    let st = read_state(pane)?;
    let plain = capture(pane, &st, true, false, false)?;
    let base = capture(pane, &st, false, true, false)?;
    let full = capture(pane, &st, true, false, true)?;
    let vp = Viewport::build(plain, &full, st.width, st.height);

    let keys = label_keys(&global_option("@avy-keys"));
    let vtime = timeout_deciseconds(&global_option("@avy-timeout"));
    let cap = keys.len() * keys.len();

    let mut kb = Keyboard::new();
    let mut query = String::new();
    let mut phase = Phase::Filter;
    let mut hits: Vec<Hit> = Vec::new();
    let mut truncated = 0usize;
    // EOF on a timed read is indistinguishable from a timeout; a burst of
    // instant "timeouts" means stdin is gone — bail instead of spinning.
    let mut spins = 0u32;
    let mut spin_started = Instant::now();

    loop {
        phase = match phase {
            Phase::Filter => {
                hits = vp.usable(find_hits(&vp.lines, &query));
                truncated = hits.len().saturating_sub(cap);
                hits.truncate(cap);
                render(&base, vp.height, &match_overlays(&vp, &hits));
                let timer = if query.is_empty() { None } else { Some(vtime) };
                let key = kb.next(timer);
                if key != Key::Timeout {
                    spins = 0;
                }
                match key {
                    Key::Ch(c) => {
                        query.push(c);
                        Phase::Filter
                    }
                    Key::Backspace => {
                        query.pop();
                        Phase::Filter
                    }
                    Key::Escape | Key::Cancel => return Ok(()),
                    Key::Enter | Key::Timeout if !query.is_empty() && !hits.is_empty() => {
                        if hits.len() == 1 {
                            return jump(pane, &st, &vp, &hits[0], truncated);
                        }
                        Phase::Label { labels: assign_labels(hits.len(), &keys), typed: String::new() }
                    }
                    Key::Timeout => {
                        if spins == 0 {
                            spin_started = Instant::now();
                        }
                        spins += 1;
                        if spins >= 100 && spin_started.elapsed().as_millis() < 1000 {
                            return Ok(()); // stdin is at EOF, not idle
                        }
                        if spin_started.elapsed().as_millis() >= 1000 {
                            spins = 0;
                        }
                        Phase::Filter
                    }
                    Key::Enter | Key::Ignore => Phase::Filter,
                }
            }
            Phase::Label { labels, typed } => {
                let mut ov = match_overlays(&vp, &hits);
                for (h, label) in hits.iter().zip(labels.iter()) {
                    if !label.starts_with(typed.as_str()) {
                        continue;
                    }
                    let rest: String = label.chars().skip(typed.chars().count()).collect();
                    for (k, c) in rest.chars().enumerate() {
                        let (row, col) = vp.pos(h.line, h.off + k);
                        if row < vp.height {
                            ov.push((row, col, c.to_string(), SGR_LABEL));
                        }
                    }
                }
                render(&base, vp.height, &ov);
                match kb.next(None) {
                    Key::Ch(c) if keys.contains(&c) => {
                        let mut next_typed = typed;
                        next_typed.push(c);
                        let matching: Vec<usize> = labels
                            .iter()
                            .enumerate()
                            .filter(|(_, l)| l.starts_with(next_typed.as_str()))
                            .map(|(i, _)| i)
                            .collect();
                        match matching.as_slice() {
                            [] => return Ok(()), // no such label — cancel, like avy
                            [i] if labels[*i] == next_typed => {
                                return jump(pane, &st, &vp, &hits[*i], truncated)
                            }
                            _ => Phase::Label { labels, typed: next_typed },
                        }
                    }
                    Key::Backspace => {
                        if typed.is_empty() {
                            Phase::Filter
                        } else {
                            let mut t2 = typed;
                            t2.pop();
                            Phase::Label { labels, typed: t2 }
                        }
                    }
                    _ => return Ok(()), // Escape, Cancel, EOF, any non-label key
                }
            }
        };
    }
}

fn jump(pane: &str, st: &PaneState, vp: &Viewport, hit: &Hit, truncated: usize) -> Result<(), String> {
    let (vrow, steps) = vp.jump_params(hit).ok_or("hit is not jumpable")?;
    let batch = plan_jump(pane, st.in_mode, vrow, steps);
    if !t::tmux_batch(&batch) {
        return Err("copy-mode positioning failed".into());
    }
    if truncated > 0 {
        t::message_literal(&format!("avy: {truncated} further matches had no label"));
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — the wrap/step cases encode measurements from a real tmux 3.5a
// (2026-08-21, 40-column pane; see the measure-*.sh scripts in records/).
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn line80() -> String {
        // Wraps into rows of chars 0..39 and 40..79 at width 40.
        let mut s = "0123456789".repeat(4);
        s.push_str(&"ABCDEFGHIJ".repeat(4));
        s
    }

    // 'x' + 25 CJK chars: row 0 = x + 19 CJK (39 cells, early wrap), row 1 = 6 CJK.
    fn cjk_line() -> String {
        let mut s = String::from("x");
        s.push_str(&"中".repeat(25));
        s
    }

    #[test]
    fn locate_plain_wrap() {
        let l = line80();
        assert_eq!(locate(&l, 40, 0), (0, 0));
        assert_eq!(locate(&l, 40, 39), (0, 39));
        assert_eq!(locate(&l, 40, 40), (1, 0));
        assert_eq!(locate(&l, 40, 79), (1, 39));
        // off == len: just past the last char
        assert_eq!(locate(&l, 40, 80), (1, 40));
    }

    #[test]
    fn locate_cjk_early_wrap() {
        let l = cjk_line();
        // Measured: char 19 sits at cell 37 of row 0; char 20 early-wraps.
        assert_eq!(locate(&l, 40, 19), (0, 37));
        assert_eq!(locate(&l, 40, 20), (1, 0));
        assert_eq!(locate(&l, 40, 21), (1, 2));
    }

    #[test]
    fn rows() {
        assert_eq!(rows_for("", 40), 1);
        assert_eq!(rows_for("short", 40), 1);
        assert_eq!(rows_for(&line80(), 40), 2);
        assert_eq!(rows_for(&cjk_line(), 40), 2);
        // Exactly filling the width stays one row (phantom x=40 is row 0).
        assert_eq!(rows_for(&"a".repeat(40), 40), 1);
    }

    #[test]
    fn steps_phantom_rule() {
        let l = line80();
        // Measured: N=40 reaches the phantom x=40 on row 0; char 40 needs N=41.
        assert_eq!(steps_to(&l, 40, 39), 39);
        assert_eq!(steps_to(&l, 40, 40), 41);
        assert_eq!(steps_to(&l, 40, 44), 45);
        // Measured on the early-wrap CJK line: char 20 needs N=21.
        let c = cjk_line();
        assert_eq!(steps_to(&c, 40, 19), 19);
        assert_eq!(steps_to(&c, 40, 20), 21);
        assert_eq!(steps_to(&c, 40, 21), 22);
    }

    #[test]
    fn smart_case_matching() {
        let lines = vec!["Error: file".to_string(), "error again".to_string()];
        let all = find_hits(&lines, "er");
        assert_eq!(all.len(), 2); // "Er" in line 0, "er" in line 1
        assert_eq!(all[0], Hit { line: 0, off: 0, len: 2 });
        assert_eq!(all[1], Hit { line: 1, off: 0, len: 2 });
        let exact = find_hits(&lines, "Er");
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].line, 0);
    }

    #[test]
    fn overlapping_hits_all_reported() {
        let lines = vec!["aaaa".to_string()];
        assert_eq!(find_hits(&lines, "aa").len(), 3);
    }

    #[test]
    fn labels_single_then_pairs() {
        let keys: Vec<char> = "asd".chars().collect();
        assert_eq!(assign_labels(2, &keys), vec!["a", "s"]);
        assert_eq!(assign_labels(3, &keys), vec!["a", "s", "d"]);
        let nine = assign_labels(4, &keys);
        assert_eq!(nine, vec!["aa", "as", "ad", "sa"]);
    }

    #[test]
    fn hidden_prefix() {
        assert_eq!(hidden_prefix_chars("abcdef", "def"), Some(3));
        assert_eq!(hidden_prefix_chars("abcdef", "abcdef"), Some(0));
        assert_eq!(hidden_prefix_chars("abcdef", "xyz"), None);
        assert_eq!(hidden_prefix_chars("中文字", "字"), Some(2));
    }

    #[test]
    fn options_parsing() {
        assert_eq!(timeout_deciseconds(""), 5);
        assert_eq!(timeout_deciseconds("300"), 3);
        assert_eq!(timeout_deciseconds("50"), 1);
        assert_eq!(timeout_deciseconds("junk"), 5);
        assert_eq!(timeout_deciseconds("99999"), 255);
        assert_eq!(label_keys(""), "asdfghjkl".chars().collect::<Vec<_>>());
        assert_eq!(label_keys("qwq"), vec!['q', 'w']);
    }

    #[test]
    fn jump_batch_shapes() {
        let b = plan_jump("%5", false, 0, 0);
        assert_eq!(b.len(), 3); // copy-mode, top-line, start-of-line
        assert_eq!(b[0][0], "copy-mode");
        let b = plan_jump("%5", true, 3, 13);
        assert_eq!(
            b,
            vec![
                vec!["send-keys", "-t", "%5", "-X", "top-line"],
                vec!["send-keys", "-t", "%5", "-X", "-N", "3", "cursor-down"],
                vec!["send-keys", "-t", "%5", "-X", "start-of-line"],
                vec!["send-keys", "-t", "%5", "-X", "-N", "13", "cursor-right"],
            ]
            .into_iter()
            .map(|v| v.into_iter().map(String::from).collect::<Vec<String>>())
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn viewport_straddler_math() {
        // Width 10; the true first line is 25 chars (3 rows); the viewport
        // starts at its second row, so 10 chars are hidden and the visible
        // suffix is 15 chars (2 rows). A second line follows.
        let full_first = "abcdefghijklmnopqrstuvwxy".to_string();
        let visible_first = full_first.chars().skip(10).collect::<String>();
        let lines = vec![visible_first, "target here".to_string()];
        let full = vec![full_first.clone(), "target here".to_string()];
        let vp = Viewport::build(lines, &full, 10, 5);
        assert_eq!(vp.hidden, Some(10));
        assert_eq!(vp.full_first, full_first);

        // Hit at visible offset 2 of line 0 → true char 12, which sits on the
        // full line's row 1 → steps 13; anchored at the viewport top (vrow 0).
        let hit = Hit { line: 0, off: 2, len: 1 };
        assert_eq!(vp.pos(0, 2), (0, 2));
        assert_eq!(vp.jump_params(&hit), Some((0, 13)));

        // Line 1 starts after the 2 visible rows of line 0.
        let hit2 = Hit { line: 1, off: 7, len: 4 };
        assert_eq!(vp.pos(1, 7), (2, 7));
        assert_eq!(vp.jump_params(&hit2), Some((2, 7)));
    }

    #[test]
    fn viewport_failed_recovery_excludes_top_line() {
        let lines = vec!["visible".to_string(), "next".to_string()];
        let full = vec!["mismatch".to_string(), "next".to_string()];
        let vp = Viewport::build(lines, &full, 40, 5);
        assert_eq!(vp.hidden, None);
        let hits = vp.usable(vec![
            Hit { line: 0, off: 0, len: 3 },
            Hit { line: 1, off: 0, len: 3 },
        ]);
        assert_eq!(hits, vec![Hit { line: 1, off: 0, len: 3 }]);
    }

    #[test]
    fn hits_below_viewport_excluded() {
        let lines: Vec<String> = (0..8).map(|i| format!("row {i}")).collect();
        let full = lines.clone();
        let vp = Viewport::build(lines, &full, 40, 5);
        let hits = vp.usable(vec![
            Hit { line: 2, off: 0, len: 3 },
            Hit { line: 7, off: 0, len: 3 },
        ]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2);
    }
}
