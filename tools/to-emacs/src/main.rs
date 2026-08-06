//! `to-emacs` — open a pane's FULL scrollback in the running Emacs daemon.
//!
//! One mode:
//!
//! ```text
//!   to-emacs [<src_pane_id>]    entire scrollback -> file -> emacsclient
//! ```
//!
//! No argument (or an unexpanded-empty `#{pane_id}`) falls back to `$TMUX_PANE`.
//! See [ARCHITECTURE.md](../ARCHITECTURE.md) and
//! [the scrollback-to-emacs plan](../../plans/to-emacs-scrollback.md).
//!
//! Two invariants:
//!   * **a dead daemon must answer "no", not get autostarted** — the frame probe
//!     runs without `-a ""`, because an autostart behind a key binding is a
//!     multi-second hang loading init.el, outside `emacs.service` supervision;
//!   * **every outcome reports via `display-message`** — file + line count +
//!     which frame path was taken, so a silent no-op is impossible.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tmuxlib as t;

const USAGE: &str = "usage: to-emacs [<src_pane_id>]";

/// A frame is usable if it has a tty (an `emacsclient -t` frame) or is
/// graphical (x/w32/ns/pgtk). The daemon's invisible F1 frame has neither —
/// opening a file "on" F1 paints nothing anywhere, which is exactly the trap
/// this probe exists to avoid (see ~/.claude/commands/emacs-open.md).
const FRAME_PROBE: &str = "(if (delq nil (mapcar (lambda (f) (or (frame-parameter f 'tty) (memq (framep f) '(x w32 ns pgtk)))) (frame-list))) t nil)";

fn main() {
    let src = std::env::args()
        .nth(1)
        .filter(|s| !s.is_empty())
        .or_else(t::current_pane);
    let Some(src) = src else {
        eprintln!("to-emacs: no source pane id and no $TMUX_PANE\n{USAGE}");
        t::message("to-emacs: no source pane id and no $TMUX_PANE");
        std::process::exit(2);
    };
    std::process::exit(run(&src));
}

fn run(src: &str) -> i32 {
    let raw = match capture_scrollback(src) {
        Ok(b) => b,
        Err(e) => return fail(&e),
    };
    let body = trim_trailing_blank_lines(&raw);
    if body.is_empty() {
        // Nothing to show is not a failure — exit 0, no file, no frame churn.
        t::message(&format!("to-emacs: {src} scrollback is empty"));
        return 0;
    }

    let dir = t::capture_dir();
    let path = dir.join(capture_file_name(&stamp(), src));
    if let Err(e) = write_capture(&dir, &path, body) {
        return fail(&e);
    }

    let lines = line_count(body);
    let via = match frame_usable() {
        Err(e) => return fail(&e),
        Ok(true) => match open_in_frame(&path, lines) {
            Ok(()) => "existing frame",
            Err(e) => return fail(&e),
        },
        Ok(false) => match open_in_new_window(&path, lines) {
            Ok(()) => "new window",
            Err(e) => return fail(&e),
        },
    };

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    t::message(&format!("→ EMACS {name} ({lines} lines, {via})"));
    0
}

// ---------------------------------------------------------------------------
// Capture — the pure half is trimming and counting
// ---------------------------------------------------------------------------

/// The whole history: `-S -` starts at the top of the scrollback (history-limit
/// is 100000 here). `-J` joins wrapped lines so a long command or stack frame
/// survives as one line; no `-e`, so ANSI is stripped — same trade as
/// to-claude's capture.
fn capture_scrollback(src: &str) -> Result<Vec<u8>, String> {
    let out = Command::new("tmux")
        .args(["capture-pane", "-p", "-J", "-S", "-", "-t", src])
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

/// Drop trailing all-blank lines: `capture-pane -S -` pads the final screen to
/// full pane height below the cursor, and those rows would put the `+N` jump
/// target on an empty line far past the real content. Blank = empty or
/// whitespace-only. The last kept line keeps its `\n` if it had one.
fn trim_trailing_blank_lines(body: &[u8]) -> &[u8] {
    let mut end = body.len();
    loop {
        // The last line inside body[..end], ignoring its terminating newline.
        let scan_end = if end > 0 && body[end - 1] == b'\n' {
            end - 1
        } else {
            end
        };
        if scan_end == 0 && end <= 1 {
            return &body[..0];
        }
        let line_start = body[..scan_end]
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        if body[line_start..scan_end]
            .iter()
            .all(|b| b.is_ascii_whitespace())
        {
            end = line_start;
        } else {
            let keep_end = if end > scan_end { scan_end + 1 } else { scan_end };
            return &body[..keep_end];
        }
    }
}

/// Newline count, counting a final unterminated line as a line — this is the
/// `+N` jump target, so it must name the LAST line, not one past it.
fn line_count(body: &[u8]) -> usize {
    let full = body.iter().filter(|&&b| b == b'\n').count();
    if body.is_empty() || body.ends_with(b"\n") {
        full
    } else {
        full + 1
    }
}

/// `scrollback-20260806-220217-12.txt` — the `scrollback-` prefix makes the
/// Emacs buffer name say what it is; pane-id sanitization is byte-for-byte
/// to-claude's (`%` stripped, anything exotic flattened to `-`).
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
    format!("scrollback-{stamp}-{id}.txt")
}

fn write_capture(dir: &Path, path: &PathBuf, body: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(path, body).map_err(|e| format!("{}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Emacs delivery
// ---------------------------------------------------------------------------

/// `Ok(true)` = a usable frame exists; `Ok(false)` = only the invisible F1;
/// `Err` = daemon unreachable. Deliberately no `-a ""` (see module doc).
fn frame_usable() -> Result<bool, String> {
    let out = Command::new("emacsclient")
        .args(["--eval", FRAME_PROBE])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("emacsclient: {e}"))?;
    if !out.status.success() {
        return Err("emacs daemon not reachable — systemctl --user start emacs".to_string());
    }
    Ok(parse_probe(&String::from_utf8_lossy(&out.stdout)))
}

/// Elisp `t` printed by --eval means "yes"; `nil` or anything else means "no".
fn parse_probe(out: &str) -> bool {
    out.trim() == "t"
}

/// `-n` opens in the most-recently-used frame and returns immediately; a
/// tmux-hosted TTY frame redraws by itself. `+N` jumps to the last line — the
/// interesting part of a scrollback is its tail.
fn open_in_frame(path: &Path, line: usize) -> Result<(), String> {
    let plus = format!("+{line}");
    let out = Command::new("emacsclient")
        .args(["-n", &plus])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("emacsclient: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "emacsclient -n: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// No frame anywhere: give the daemon one, in a tmux window this tool owns.
/// Never `emacsclient -c` from a script — under Wayland it dies with
/// `could not get terminal name` (per emacs-open.md).
fn open_in_new_window(path: &Path, line: usize) -> Result<(), String> {
    let cmd = new_window_cmd(&path.display().to_string(), line);
    t::tmux(["new-window", "-n", "emacs", &cmd]).map(|_| ())
}

/// Shell string for the new window. Filename characters are `[A-Za-z0-9._-/]`
/// only (stamp + sanitized pane id under /tmp/claude-capture), so the single
/// quoting is trivially safe — but it is still there, and still tested.
fn new_window_cmd(path: &str, line: usize) -> String {
    format!("emacsclient -t +{line} '{path}'")
}

// ---------------------------------------------------------------------------
// Plumbing
// ---------------------------------------------------------------------------

/// Local `YYYYmmdd-HHMMSS`. Shelling out to `date` keeps the local timezone
/// right without a time crate — the same trade to-claude and cc-beacon make.
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

fn fail(msg: &str) -> i32 {
    t::message(&format!("to-emacs: {msg}"));
    eprintln!("to-emacs: {msg}");
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- trailing-blank trim ------------------------------------------------

    #[test]
    fn trim_drops_the_pane_height_padding() {
        assert_eq!(trim_trailing_blank_lines(b"a\nb\n\n\n\n"), b"a\nb\n");
    }

    #[test]
    fn trim_treats_whitespace_only_lines_as_blank() {
        assert_eq!(trim_trailing_blank_lines(b"a\n   \n\t\n  \n"), b"a\n");
    }

    #[test]
    fn trim_keeps_a_body_with_no_padding() {
        assert_eq!(trim_trailing_blank_lines(b"a\nb\n"), b"a\nb\n");
        assert_eq!(trim_trailing_blank_lines(b"a\nb"), b"a\nb");
    }

    #[test]
    fn trim_of_all_blank_is_empty() {
        assert_eq!(trim_trailing_blank_lines(b"\n\n\n"), b"");
        assert_eq!(trim_trailing_blank_lines(b"   \n\t\n"), b"");
        assert_eq!(trim_trailing_blank_lines(b""), b"");
    }

    #[test]
    fn trim_preserves_embedded_blank_lines() {
        assert_eq!(trim_trailing_blank_lines(b"a\n\nb\n\n\n"), b"a\n\nb\n");
    }

    #[test]
    fn trim_keeps_the_last_line_unterminated_if_it_was() {
        assert_eq!(trim_trailing_blank_lines(b"a\nb   "), b"a\nb   ");
    }

    // -- line counting ------------------------------------------------------

    #[test]
    fn line_count_matches_the_jump_target() {
        assert_eq!(line_count(b""), 0);
        assert_eq!(line_count(b"a\n"), 1);
        assert_eq!(line_count(b"a\nb\n"), 2);
        assert_eq!(line_count(b"a\nb"), 2);
    }

    // -- file naming --------------------------------------------------------

    #[test]
    fn capture_file_name_drops_the_pane_sigil() {
        assert_eq!(
            capture_file_name("20260806-220217", "%12"),
            "scrollback-20260806-220217-12.txt"
        );
    }

    #[test]
    fn capture_file_name_flattens_a_non_pane_id_target() {
        assert_eq!(
            capture_file_name("ts", "main:1.0"),
            "scrollback-ts-main-1-0.txt"
        );
        assert_eq!(
            capture_file_name("ts", "../etc/passwd"),
            "scrollback-ts----etc-passwd.txt"
        );
    }

    // -- frame probe --------------------------------------------------------

    #[test]
    fn probe_t_is_the_only_yes() {
        assert!(parse_probe("t"));
        assert!(parse_probe("t\n"));
        assert!(!parse_probe("nil"));
        assert!(!parse_probe(""));
        assert!(!parse_probe("*ERROR*: something"));
    }

    // -- new-window command -------------------------------------------------

    #[test]
    fn new_window_cmd_quotes_the_path_and_jumps() {
        assert_eq!(
            new_window_cmd("/tmp/claude-capture/scrollback-ts-3.txt", 4821),
            "emacsclient -t +4821 '/tmp/claude-capture/scrollback-ts-3.txt'"
        );
    }
}
