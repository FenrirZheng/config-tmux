//! `to-claude` — drop text into a Claude Code composer, **unsubmitted**.
//!
//! Four modes, each spelled both `--flag` and bare word because tmux-thumbs
//! config and copy-mode binds use different idioms:
//!
//! ```text
//!   to-claude --ref | ref            stdin one-liner  -> types `@<text> `
//!   to-claude --type | type          stdin literal    -> typed verbatim
//!   to-claude --paste | paste        stdin block      -> ONE bracketed paste
//!   to-claude capture <src_pane_id>  scrollback tail  -> `@<file> ` reference
//! ```
//!
//! No argument means `--paste`. See [ARCHITECTURE.md](../ARCHITECTURE.md) and
//! [the capture-rail plan](../../plans/to-claude-capture-rail.md).
//!
//! Two invariants, both load-bearing:
//!   * **nothing ever presses Enter** — `talk` is the tool that submits; this one
//!     leaves the cursor after the inserted text for the human's actual question;
//!   * **every delivery reports its target** via `display-message`, so a
//!     mis-send is visible instead of silent.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tmuxlib as t;

/// `--ref` is for a `file:line` hint, not a document.
const REF_MAX: usize = 4096;
/// `--type` still goes through `send-keys -l`, one keystroke batch.
const TYPE_MAX: usize = 16384;
/// Tail depth for a normal `prefix e` capture.
const TAIL_LINES: &str = "-200";
/// Mark-on-source means the failing pane *is* the inbox — take everything.
const FULL_LINES: &str = "-10000";

const USAGE: &str = "usage: to-claude [--ref|--type|--paste|capture <pane>]";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match parse_mode(&args) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("to-claude: {e}\n{USAGE}");
            t::message(&format!("to-claude: {e}"));
            std::process::exit(2);
        }
    };

    let code = match mode {
        Mode::Capture(src) => capture(src),
        m => deliver_stdin(m),
    };
    std::process::exit(code);
}

// ---------------------------------------------------------------------------
// Mode parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Ref,
    Type,
    Paste,
    /// `None` = fall back to `$TMUX_PANE`; the `prefix e` bind passes `#{pane_id}`.
    Capture(Option<String>),
}

/// An unrecognised word is an error, never a silent fall-through to the
/// default — a typo in a key binding would otherwise paste stdin somewhere
/// nobody asked for.
fn parse_mode(args: &[String]) -> Result<Mode, String> {
    match args.first().map(String::as_str) {
        None => Ok(Mode::Paste),
        Some("--ref" | "ref") => Ok(Mode::Ref),
        Some("--type" | "type") => Ok(Mode::Type),
        Some("--paste" | "paste") => Ok(Mode::Paste),
        Some("--capture" | "capture") => Ok(Mode::Capture(
            args.get(1).filter(|s| !s.is_empty()).cloned(),
        )),
        Some(other) => Err(format!("unknown mode: {other}")),
    }
}

// ---------------------------------------------------------------------------
// Text shaping — the pure half
// ---------------------------------------------------------------------------

/// Truncate to at most `max` **bytes**, backing off to a char boundary so a
/// multi-byte selection never panics or produces invalid UTF-8.
fn cap_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// `foo.vue:181` -> `@foo.vue:181 `. Newlines are dropped rather than escaped:
/// a thumbs hint is a single token, and an embedded LF in `send-keys -l` would
/// submit the composer.
fn ref_line(input: &str) -> String {
    let flat: String = cap_bytes(input, REF_MAX)
        .chars()
        .filter(|c| *c != '\n' && *c != '\r')
        .collect();
    let trimmed = flat.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    format!("@{trimmed} ")
}

/// Verbatim, only bounded.
fn type_text(input: &str) -> String {
    cap_bytes(input, TYPE_MAX).to_string()
}

/// `20260806-220217-12.txt` — the `%` of `%12` is not a filename character, and
/// anything else exotic in a target string is flattened for the same reason.
fn capture_file_name(stamp: &str, pane: &str) -> String {
    let id: String = pane
        .trim_start_matches('%')
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("{stamp}-{id}.txt")
}

/// The one line `prefix e` types. The trailing space keeps the cursor clear of
/// the path so the next thing typed is the human's question.
fn capture_line(cwd: &str, path: &str) -> String {
    format!("[capture from {cwd}] @{path} ")
}

/// Unique per invocation without a random source: pid plus nanoseconds.
fn buffer_name(pid: u32, nanos: u128) -> String {
    format!("toclaude-{pid}-{nanos}")
}

// ---------------------------------------------------------------------------
// Target resolution
// ---------------------------------------------------------------------------

/// A Claude pane in the running for "most recently active".
#[derive(Debug, Clone)]
struct Candidate {
    id: String,
    recency: u64,
    active: bool,
}

/// Highest recency wins; the focused pane breaks a tie (a whole window shares
/// one `window_activity` stamp), and the lowest pane id breaks a total tie so
/// the choice is at least deterministic.
fn most_recent(cands: &[Candidate]) -> Option<String> {
    cands
        .iter()
        .max_by(|a, b| {
            a.recency
                .cmp(&b.recency)
                .then(a.active.cmp(&b.active))
                .then_with(|| b.id.cmp(&a.id))
        })
        .map(|c| c.id.clone())
}

/// The user-declared inbox, if one is marked and still alive.
fn marked_pane() -> Option<String> {
    if t::display(None, "#{pane_marked_set}").unwrap_or_default() != "1" {
        return None;
    }
    t::tmux(["display-message", "-p", "-t", "{marked}", "#{pane_id}"])
        .ok()
        .filter(|s| !s.is_empty())
}

/// Fallback inventory: every pane that looks like Claude, minus `exclude`.
///
/// `pane_last_activity` — which the plan's sketch sorted on — does not exist in
/// tmux 3.5a (verified on this machine). `window_activity` is the closest real
/// format; a beacon-stamped `@claude_since` wins when it is newer.
fn claude_candidates(exclude: Option<&str>) -> Vec<Candidate> {
    let activity = window_activity();
    t::list_panes()
        .into_iter()
        // `is_claude()` reads the beacon options and the pane-title heuristic;
        // `pane_current_command` is the MVP signal the plan specified and is
        // `claude` on this machine. Either is enough.
        .filter(|p| p.is_claude() || p.command == "claude")
        .filter(|p| exclude != Some(p.id.as_str()))
        .map(|p| {
            let win = activity
                .iter()
                .find(|(id, _)| *id == p.id)
                .map(|(_, ts)| *ts)
                .unwrap_or(0);
            Candidate {
                recency: win.max(p.since.unwrap_or(0)),
                active: p.pane_active,
                id: p.id,
            }
        })
        .collect()
}

fn window_activity() -> Vec<(String, u64)> {
    let out =
        t::tmux(["list-panes", "-a", "-F", "#{pane_id}\t#{window_activity}"]).unwrap_or_default();
    out.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(id, ts)| (id.to_string(), ts.trim().parse().unwrap_or(0)))
        .collect()
}

/// `skip_mark` is the mark-on-source case: the marked pane is the pane being
/// captured *from*, so the mark says nothing about where to send.
fn resolve_target(skip_mark: bool, exclude: Option<&str>) -> Option<String> {
    if !skip_mark {
        if let Some(m) = marked_pane() {
            if exclude != Some(m.as_str()) {
                return Some(m);
            }
        }
    }
    most_recent(&claude_candidates(exclude))
}

/// The one genuinely unusable state: there is nowhere to deliver.
fn no_target() -> i32 {
    fail("no marked pane and no claude pane")
}

fn fail(msg: &str) -> i32 {
    t::message(&format!("to-claude: {msg}"));
    eprintln!("to-claude: {msg}");
    1
}

// ---------------------------------------------------------------------------
// Delivery
// ---------------------------------------------------------------------------

/// `--` guards text that begins with `-` from being read as a flag.
fn send_literal(target: &str, text: &str) -> Result<(), String> {
    t::tmux(["send-keys", "-t", target, "-l", "--", text]).map(|_| ())
}

/// The delivery pattern proven by `talk`'s `send_text`: a uniquely named buffer,
/// then `-d` (drop it afterwards), `-r` (keep the LFs as LFs, no CR
/// translation) and `-p` — the bracketed-paste wrapper that makes a 40-line
/// selection arrive as one `[Pasted text]` unit instead of 40 submitted lines.
fn send_paste(target: &str, bytes: &[u8]) -> Result<(), String> {
    let buf = buffer_name(std::process::id(), nanos());
    let mut child = Command::new("tmux")
        .args(["load-buffer", "-b", &buf, "-"])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn tmux: {e}"))?;
    {
        let mut stdin = child.stdin.take().ok_or("tmux load-buffer took no stdin")?;
        stdin
            .write_all(bytes)
            .map_err(|e| format!("failed to write buffer: {e}"))?;
    }
    if !child
        .wait()
        .map_err(|e| format!("tmux load-buffer: {e}"))?
        .success()
    {
        return Err("tmux load-buffer failed".to_string());
    }
    t::tmux(["paste-buffer", "-d", "-r", "-p", "-b", &buf, "-t", target]).map_err(|e| {
        // `-d` never ran, so the buffer would otherwise leak into the stack.
        let _ = t::tmux(["delete-buffer", "-b", &buf]);
        e
    })?;
    Ok(())
}

fn announce(target: &str) {
    t::message(&format!("→ CLAUDE {target}"));
}

// ---------------------------------------------------------------------------
// Modes
// ---------------------------------------------------------------------------

fn read_stdin() -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buf);
    buf
}

fn deliver_stdin(mode: Mode) -> i32 {
    let raw = read_stdin();
    if raw.is_empty() {
        return 0; // nothing to do is not a failure
    }

    let Some(target) = resolve_target(false, None) else {
        return no_target();
    };

    let result = match mode {
        Mode::Ref => {
            let line = ref_line(&String::from_utf8_lossy(&raw));
            if line.is_empty() {
                return 0;
            }
            send_literal(&target, &line)
        }
        Mode::Type => {
            let text = type_text(&String::from_utf8_lossy(&raw));
            if text.is_empty() {
                return 0;
            }
            send_literal(&target, &text)
        }
        // Raw bytes, not the lossy string: the buffer round-trips whatever the
        // selection actually was.
        Mode::Paste => send_paste(&target, &raw),
        Mode::Capture(_) => return fail("internal: capture routed to deliver_stdin"),
    };

    match result {
        Ok(()) => {
            announce(&target);
            0
        }
        Err(e) => fail(&e),
    }
}

fn capture(src: Option<String>) -> i32 {
    let Some(src) = src.or_else(t::current_pane) else {
        eprintln!("to-claude: capture needs a source pane id\n{USAGE}");
        t::message("to-claude: capture needs a source pane id");
        return 2;
    };

    // Mark-on-source: the failing pane is itself the declared inbox, so the
    // mark cannot also name the destination. Take the whole scrollback (this
    // pane is the one being debugged) and resolve the target by fallback.
    let mark_on_source = t::display(Some(&src), "#{pane_marked}").unwrap_or_default() == "1";
    let depth = if mark_on_source {
        FULL_LINES
    } else {
        TAIL_LINES
    };

    let body = match capture_pane(&src, depth) {
        Ok(b) => b,
        Err(e) => return fail(&e),
    };

    let dir = t::capture_dir();
    let path = dir.join(capture_file_name(&stamp(), &src));
    if let Err(e) = write_capture(&dir, &path, &body) {
        return fail(&e);
    }

    let Some(target) = resolve_target(mark_on_source, Some(&src)) else {
        return no_target();
    };

    let cwd = t::display(Some(&src), "#{pane_current_path}").unwrap_or_default();
    let line = capture_line(&cwd, &path.display().to_string());
    match send_literal(&target, &line) {
        Ok(()) => {
            announce(&target);
            0
        }
        Err(e) => fail(&e),
    }
}

/// `-J` joins wrapped lines so a long command or stack frame survives as one
/// line; raw bytes are kept so the file is byte-identical to what tmux emitted.
fn capture_pane(src: &str, depth: &str) -> Result<Vec<u8>, String> {
    let out = Command::new("tmux")
        .args(["capture-pane", "-p", "-J", "-S", depth, "-t", src])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("failed to spawn tmux: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "capture-pane {src}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

fn write_capture(dir: &Path, path: &PathBuf, body: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(path, body).map_err(|e| format!("{}: {e}", path.display()))
}

/// Local `YYYYmmdd-HHMMSS`. Shelling out to `date` keeps the local timezone
/// right without a time crate — the same trade `cc-beacon` makes.
fn stamp() -> String {
    Command::new("date")
        .arg("+%Y%m%d-%H%M%S")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| t::now_epoch().to_string())
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // -- mode parsing -------------------------------------------------------

    #[test]
    fn both_spellings_of_every_mode_parse() {
        assert_eq!(parse_mode(&args(&["--ref"])).unwrap(), Mode::Ref);
        assert_eq!(parse_mode(&args(&["ref"])).unwrap(), Mode::Ref);
        assert_eq!(parse_mode(&args(&["--type"])).unwrap(), Mode::Type);
        assert_eq!(parse_mode(&args(&["type"])).unwrap(), Mode::Type);
        assert_eq!(parse_mode(&args(&["--paste"])).unwrap(), Mode::Paste);
        assert_eq!(parse_mode(&args(&["paste"])).unwrap(), Mode::Paste);
    }

    #[test]
    fn no_argument_means_paste() {
        assert_eq!(parse_mode(&[]).unwrap(), Mode::Paste);
    }

    #[test]
    fn capture_carries_its_source_pane() {
        assert_eq!(
            parse_mode(&args(&["capture", "%7"])).unwrap(),
            Mode::Capture(Some("%7".into()))
        );
        assert_eq!(
            parse_mode(&args(&["capture"])).unwrap(),
            Mode::Capture(None)
        );
        // An unexpanded `#{pane_id}` that arrived empty must not look like a pane.
        assert_eq!(
            parse_mode(&args(&["capture", ""])).unwrap(),
            Mode::Capture(None)
        );
    }

    #[test]
    fn unknown_mode_is_an_error_not_a_default() {
        assert!(parse_mode(&args(&["--past"])).is_err());
        assert!(parse_mode(&args(&["-r"])).is_err());
        assert!(parse_mode(&args(&[""])).is_err());
    }

    // -- @-ref construction -------------------------------------------------

    #[test]
    fn ref_wraps_and_leaves_a_trailing_space() {
        assert_eq!(ref_line("foo.vue:181"), "@foo.vue:181 ");
        assert_eq!(ref_line("  src/main.rs:12\n"), "@src/main.rs:12 ");
    }

    #[test]
    fn ref_strips_embedded_newlines() {
        assert_eq!(ref_line("a\nb\r\nc"), "@abc ");
        assert_eq!(ref_line("\n\n"), "");
        assert_eq!(ref_line(""), "");
    }

    #[test]
    fn ref_caps_at_4096_bytes_on_a_char_boundary() {
        let long = "設".repeat(3000); // 9000 bytes
        let out = ref_line(&long);
        let body = out.trim_start_matches('@').trim_end();
        assert!(body.len() <= REF_MAX);
        // 4096 is not a multiple of 3, so this only holds if we backed off.
        assert_eq!(body.chars().count(), REF_MAX / 3);
        assert!(body.chars().all(|c| c == '設'));
    }

    // -- literal text -------------------------------------------------------

    #[test]
    fn type_is_verbatim_including_newlines_and_leading_dash() {
        assert_eq!(
            type_text("--not-a-flag\nline two\n"),
            "--not-a-flag\nline two\n"
        );
        assert_eq!(type_text(""), "");
    }

    #[test]
    fn type_caps_at_16384_bytes_without_splitting_a_char() {
        let long = "漢".repeat(10_000); // 30000 bytes
        let out = type_text(&long);
        assert!(out.len() <= TYPE_MAX);
        assert_eq!(out.chars().count(), TYPE_MAX / 3);
    }

    #[test]
    fn cap_bytes_never_splits_a_multi_byte_char() {
        assert_eq!(cap_bytes("héllo", 2), "h"); // é occupies bytes 1..3
        assert_eq!(cap_bytes("héllo", 3), "hé");
        assert_eq!(cap_bytes("abc", 10), "abc");
        assert_eq!(cap_bytes("設", 1), "");
    }

    // -- capture naming -----------------------------------------------------

    #[test]
    fn capture_file_name_drops_the_pane_sigil() {
        assert_eq!(
            capture_file_name("20260806-220217", "%12"),
            "20260806-220217-12.txt"
        );
        assert_eq!(
            capture_file_name("20260806-220217", "%3"),
            "20260806-220217-3.txt"
        );
    }

    #[test]
    fn capture_file_name_flattens_a_non_pane_id_target() {
        assert_eq!(capture_file_name("ts", "main:1.0"), "ts-main-1-0.txt");
        assert_eq!(
            capture_file_name("ts", "../etc/passwd"),
            "ts----etc-passwd.txt"
        );
    }

    #[test]
    fn capture_line_is_prefix_path_space() {
        assert_eq!(
            capture_line("/home/fenrir/code/app", "/tmp/claude-capture/ts-3.txt"),
            "[capture from /home/fenrir/code/app] @/tmp/claude-capture/ts-3.txt "
        );
        assert!(capture_line("/x", "/y").ends_with(' '));
    }

    #[test]
    fn buffer_names_differ_per_invocation() {
        assert_eq!(buffer_name(42, 7), "toclaude-42-7");
        assert_ne!(buffer_name(42, 7), buffer_name(42, 8));
    }

    // -- fallback ranking ---------------------------------------------------

    fn cand(id: &str, recency: u64, active: bool) -> Candidate {
        Candidate {
            id: id.into(),
            recency,
            active,
        }
    }

    #[test]
    fn most_recent_prefers_the_newest_activity() {
        let c = [
            cand("%1", 100, false),
            cand("%2", 300, false),
            cand("%3", 200, true),
        ];
        assert_eq!(most_recent(&c).unwrap(), "%2");
    }

    #[test]
    fn equal_activity_falls_to_the_focused_pane_then_the_lowest_id() {
        let c = [cand("%1", 300, false), cand("%2", 300, true)];
        assert_eq!(most_recent(&c).unwrap(), "%2");
        let c = [cand("%5", 300, false), cand("%1", 300, false)];
        assert_eq!(most_recent(&c).unwrap(), "%1");
    }

    #[test]
    fn no_candidates_means_no_target() {
        assert_eq!(most_recent(&[]), None);
    }
}
