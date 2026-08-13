//! `cc-tape` — flight recorder for a tmux pane, past the alternate screen.
//!
//! Three subcommands:
//!   * `toggle <pane>` — `prefix T`; start/stop `pipe-pane` into
//!     `~/.cache/tmux-tape/<pane>.log` and flip the window's border on/off;
//!   * `strip` — stdin→stdout ANSI/OSC stripper; this is what `pipe-pane` runs;
//!   * `status <pane>` — `on`/`off` plus the log path, for scripts and debugging.
//!
//! `pipe-pane` taps the pty byte stream rather than the screen, so a tape keeps
//! rolling while the pane's app lives on the alternate screen — exactly where
//! `capture-pane` and `talk read` go blind.
//!
//! Two constraints worth stating up front:
//!   * **the window's `pane-border-format` is never written here.** Six features
//!     share one format string (see [ARCHITECTURE.org](../ARCHITECTURE.org)); the
//!     global one already renders `#{?pane_pipe,●REC,}`. A window-local format
//!     would clobber the beacon/role/marked badges. Only `pane-border-status`
//!     is touched, and only at the window level.
//!   * **`strip` never fails loudly.** It is the write end of the user's pane
//!     pipe; a dead recorder must not take the pane's output with it.

use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use tmuxlib as t;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const CR: u8 = b'\r';
const LF: u8 = b'\n';

fn main() {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    let pane = args.next().unwrap_or_default();

    match cmd.as_str() {
        "strip" => strip_stdin(),
        "toggle" if !pane.is_empty() => toggle(&pane),
        "status" if !pane.is_empty() => status(&pane),
        _ => {
            eprintln!("usage: cc-tape toggle <pane_id> | strip | status <pane_id>");
            std::process::exit(2);
        }
    }
}

// ---------------------------------------------------------------------------
// The stripper
// ---------------------------------------------------------------------------

/// Where the escape scanner is between bytes.
///
/// `pipe-pane` hands us an arbitrarily-chopped byte stream, so every sequence
/// must be resumable: a CSI can (and will) straddle two `read()` calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Scan {
    /// Ordinary text.
    #[default]
    None,
    /// Saw ESC; the next byte picks the sequence family.
    Start,
    /// Inside `ESC [ … <final>` — SGR, cursor moves, `?1049h`, …
    Csi,
    /// Inside `ESC <intermediate>… <final>` — charset selects (`ESC ( B`),
    /// `ESC # 8`, `ESC % G`.
    Inter,
    /// Inside a string sequence (OSC/DCS/SOS/PM/APC), ended by BEL or `ESC \`.
    Str,
    /// Inside a string sequence, having just seen ESC (candidate ST).
    StrEsc,
}

/// Pure ANSI/OSC stripper: bytes in, cleaned bytes out, no I/O.
///
/// Carriage returns are resolved *after* escapes are dropped, so a TUI's
/// `\r <erase-line> \n` redraw collapses to one newline instead of leaving a
/// blank line behind. That is also why the pending-CR flag lives on the struct:
/// the decision needs one byte of lookahead, which may be in the next chunk.
#[derive(Debug, Default)]
struct Stripper {
    scan: Scan,
    pending_cr: bool,
}

impl Stripper {
    fn feed(&mut self, chunk: &[u8], out: &mut Vec<u8>) {
        for &b in chunk {
            self.byte(b, out);
        }
    }

    /// End of stream: a trailing CR still owes a newline.
    fn finish(&mut self, out: &mut Vec<u8>) {
        if self.pending_cr {
            self.pending_cr = false;
            out.push(LF);
        }
    }

    fn byte(&mut self, b: u8, out: &mut Vec<u8>) {
        match self.scan {
            Scan::None => {
                if b == ESC {
                    self.scan = Scan::Start;
                } else {
                    self.text(b, out);
                }
            }
            Scan::Start => match b {
                b'[' => self.scan = Scan::Csi,
                // OSC, DCS, SOS, PM, APC — all run until BEL or ST.
                b']' | b'P' | b'X' | b'^' | b'_' => self.scan = Scan::Str,
                ESC => {}
                0x20..=0x2f => self.scan = Scan::Inter,
                0x30..=0x7e => self.scan = Scan::None, // complete two-byte sequence
                // Not an escape at all (control byte or 8-bit data after a
                // stray ESC): keep the byte rather than eat real output.
                _ => {
                    self.scan = Scan::None;
                    self.text(b, out);
                }
            },
            Scan::Csi => match b {
                0x40..=0x7e => self.scan = Scan::None, // final byte
                ESC => self.scan = Scan::Start,        // aborted; a new one begins
                _ => {}                                // params, intermediates, junk
            },
            Scan::Inter => match b {
                0x20..=0x2f => {}
                ESC => self.scan = Scan::Start,
                _ => self.scan = Scan::None,
            },
            Scan::Str => match b {
                BEL => self.scan = Scan::None,
                ESC => self.scan = Scan::StrEsc,
                _ => {}
            },
            Scan::StrEsc => match b {
                b'\\' => self.scan = Scan::None, // ST
                ESC => {}
                // The ESC ended the string and starts something else.
                _ => {
                    self.scan = Scan::Start;
                    self.byte(b, out);
                }
            },
        }
    }

    /// The text layer: everything the escape scanner let through.
    fn text(&mut self, b: u8, out: &mut Vec<u8>) {
        if b == CR {
            // A second CR means the first one really did end a line.
            if self.pending_cr {
                out.push(LF);
            }
            self.pending_cr = true;
            return;
        }
        if self.pending_cr {
            self.pending_cr = false;
            if b != LF {
                out.push(LF);
            }
        }
        out.push(b);
    }
}

/// stdin → stdout, flushed per chunk so a live `tail -f` of the tape keeps up.
fn strip_stdin() {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();

    let mut sm = Stripper::default();
    let mut buf = [0u8; 8192];
    let mut out: Vec<u8> = Vec::with_capacity(8192);

    loop {
        let n = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        out.clear();
        sm.feed(&buf[..n], &mut out);
        if out.is_empty() {
            continue;
        }
        // Log deleted, disk full, reader gone: stop, quietly and successfully.
        if output.write_all(&out).is_err() || output.flush().is_err() {
            return;
        }
    }

    out.clear();
    sm.finish(&mut out);
    let _ = output.write_all(&out);
    let _ = output.flush();
}

// ---------------------------------------------------------------------------
// toggle / status
// ---------------------------------------------------------------------------

fn toggle(pane: &str) {
    if !t::pane_alive(pane) {
        t::message(&format!("cc-tape: no such pane {pane}"));
        return;
    }
    let Some(log) = log_path(pane) else {
        t::message(&format!("cc-tape: refusing odd pane id {pane}"));
        return;
    };
    if std::fs::create_dir_all(t::tape_dir()).is_err() {
        t::message(&format!(
            "cc-tape: cannot create {}",
            t::tape_dir().display()
        ));
        return;
    }
    let shown = log.display().to_string();

    if is_recording(pane) {
        // `pipe-pane` with no command closes the pipe.
        t::tmux_ok(["pipe-pane", "-t", pane]);
        divider(&log, "tape off");
        t::message(&esc_hash(&format!("cc-tape: ●REC off — {shown}")));
    } else {
        // Divider first: once the pipe is open the recorder owns the file, and
        // a header appended afterwards would land behind whatever the pane
        // printed in the meantime.
        divider(&log, "tape on");
        let cmd = format!(
            "exec {} strip >> {}",
            sh_quote(&self_exe()),
            sh_quote(&shown)
        );
        // tmux runs the pipe command through `sh` *after* expanding it as a
        // format string, so `#` has to survive the expansion pass first.
        if t::tmux_ok(["pipe-pane", "-t", pane, "-o", &esc_hash(&cmd)]) {
            t::message(&esc_hash(&format!("cc-tape: ●REC on — {shown}")));
        } else {
            t::message(&format!("cc-tape: pipe-pane failed for {pane}"));
        }
    }

    rec_light(pane);
}

fn status(pane: &str) {
    let path = log_path(pane)
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    println!("{} {}", if is_recording(pane) { "on" } else { "off" }, path);
}

fn is_recording(pane: &str) -> bool {
    t::display(Some(pane), "#{pane_pipe}").unwrap_or_default() == "1"
}

/// Border status is window-scoped and shared with the beacon: turn it on while
/// any pane in this window records, and *unset* it — never `off` — afterwards,
/// so the global setting takes back over. The format string is deliberately
/// left alone; see the module docs.
fn rec_light(pane: &str) {
    let piped = t::tmux(["list-panes", "-t", pane, "-F", "#{pane_pipe}"]).unwrap_or_default();
    if piped.lines().any(|l| l.trim() == "1") {
        t::set_window_opt(pane, "pane-border-status", "top");
    } else {
        t::unset_window_opt(pane, "pane-border-status");
    }
    t::tmux_ok(["refresh-client", "-S"]);
}

// ---------------------------------------------------------------------------
// Paths, quoting, time
// ---------------------------------------------------------------------------

/// `~/.cache/tmux-tape/%12.log`. A pane id is `%N`; anything that could walk
/// out of the tape directory is refused rather than sanitized.
fn log_path(pane: &str) -> Option<PathBuf> {
    if pane.is_empty() || pane.contains('/') || pane.contains("..") {
        return None;
    }
    Some(t::tape_dir().join(format!("{pane}.log")))
}

/// The path of this very binary, for the `pipe-pane` command line. Resolved at
/// runtime because the workspace is relocatable; the fallback is where the
/// release build normally lands.
fn self_exe() -> String {
    std::env::current_exe()
        .ok()
        .filter(|p| p.is_absolute())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| {
            t::tmux_dir()
                .join("tools/target/release/cc-tape")
                .display()
                .to_string()
        })
}

/// Single-quote for `sh`.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `##` is a literal `#` to tmux's format expander, which runs over both
/// `pipe-pane` commands and `display-message` text.
fn esc_hash(s: &str) -> String {
    s.replace('#', "##")
}

fn divider(log: &Path, label: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        // A pane's last output rarely ends in a newline. Without this the
        // divider lands mid-line and stops working as a slice anchor for
        // `rg '── tape on ──'`, which is how a tape is meant to be read.
        if !ends_with_newline(log) {
            let _ = f.write_all(b"\n");
        }
        let _ = writeln!(f, "{} ── {label} ──", stamp());
    }
}

/// True for a missing or empty log too — neither needs a separator line.
fn ends_with_newline(log: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(log) else {
        return true;
    };
    let Ok(len) = f.seek(SeekFrom::End(0)) else {
        return true;
    };
    if len == 0 || f.seek(SeekFrom::Start(len - 1)).is_err() {
        return true;
    }
    let mut last = [0u8; 1];
    f.read_exact(&mut last).is_ok() && last[0] == LF
}

/// Local `YYYY-mm-dd HH:MM:SS`. Shelling out to `date` keeps the timezone right
/// without pulling in a time crate; it costs one fork per toggle.
fn stamp() -> String {
    Command::new("date")
        .arg("+%F %T")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed the whole input as one chunk, then finish.
    fn strip(input: &[u8]) -> Vec<u8> {
        let mut sm = Stripper::default();
        let mut out = Vec::new();
        sm.feed(input, &mut out);
        sm.finish(&mut out);
        out
    }

    fn strip_str(input: &[u8]) -> String {
        String::from_utf8(strip(input)).expect("this fixture is valid utf-8")
    }

    /// Feed one byte per call — the worst chopping a byte stream can inflict.
    fn strip_bytewise(input: &[u8]) -> Vec<u8> {
        let mut sm = Stripper::default();
        let mut out = Vec::new();
        for b in input {
            sm.feed(&[*b], &mut out);
        }
        sm.finish(&mut out);
        out
    }

    #[test]
    fn plain_text_passes_through_unchanged() {
        let s = b"hello world\nsecond line\n";
        assert_eq!(strip(s), s.to_vec());
        assert_eq!(strip_bytewise(s), s.to_vec());
    }

    #[test]
    fn sgr_color_is_removed() {
        assert_eq!(strip_str(b"\x1b[31mred\x1b[0m\n"), "red\n");
        assert_eq!(strip_str(b"\x1b[1;38;5;196mbright\x1b[m"), "bright");
    }

    #[test]
    fn cursor_position_csi_is_removed() {
        assert_eq!(strip_str(b"\x1b[2;5Hat-home\x1b[K\n"), "at-home\n");
        // Private-mode CSI: the alt-screen switch itself.
        assert_eq!(strip_str(b"\x1b[?1049hTUI\x1b[?1049l\n"), "TUI\n");
    }

    #[test]
    fn osc_terminated_by_bel_is_removed() {
        assert_eq!(strip_str(b"\x1b]0;my title\x07after\n"), "after\n");
    }

    #[test]
    fn osc_terminated_by_st_is_removed() {
        assert_eq!(strip_str(b"\x1b]2;title\x1b\\after\n"), "after\n");
        // OSC 8 hyperlink: two OSCs wrapped around the visible label.
        assert_eq!(
            strip_str(b"\x1b]8;;http://x\x07label\x1b]8;;\x07\n"),
            "label\n"
        );
    }

    #[test]
    fn csi_split_across_two_feeds_is_still_removed() {
        let mut sm = Stripper::default();
        let mut out = Vec::new();
        sm.feed(b"\x1b[3", &mut out);
        sm.feed(b"1mred", &mut out);
        sm.finish(&mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "red");
    }

    #[test]
    fn osc_split_across_feeds_is_still_removed() {
        let mut sm = Stripper::default();
        let mut out = Vec::new();
        sm.feed(b"a\x1b]0;ti", &mut out);
        sm.feed(b"tle\x1b", &mut out);
        sm.feed(b"\\b", &mut out);
        sm.finish(&mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "ab");
    }

    #[test]
    fn charset_selects_are_removed() {
        assert_eq!(strip_str(b"\x1b(Bplain\x1b)0\n"), "plain\n");
        assert_eq!(strip_str(b"\x1b#8fill\n"), "fill\n");
    }

    #[test]
    fn two_byte_escapes_are_removed() {
        // ESC 7 / ESC 8 (cursor save/restore), ESC M (reverse index), ESC =.
        assert_eq!(
            strip_str(b"\x1b7save\x1bMup\x1b8\x1b=done\n"),
            "saveupdone\n"
        );
    }

    #[test]
    fn crlf_becomes_lf() {
        assert_eq!(strip_str(b"one\r\ntwo\r\n"), "one\ntwo\n");
    }

    #[test]
    fn bare_cr_becomes_lf() {
        assert_eq!(
            strip_str(b"progress 1\rprogress 2\n"),
            "progress 1\nprogress 2\n"
        );
        // A trailing CR still owes its newline at end of stream.
        assert_eq!(strip_str(b"tail\r"), "tail\n");
    }

    #[test]
    fn cr_is_resolved_after_escapes_not_before() {
        // The classic TUI line redraw: CR, erase-line, text, newline. Dropping
        // the escape first is what keeps this from leaving a blank line.
        assert_eq!(strip_str(b"old\r\x1b[K\n"), "old\n");
        assert_eq!(strip_str(b"old\r\x1b[Kfresh\n"), "old\nfresh\n");
        // A CR with no text before it still opens a line — same as the sed
        // reference in the plan, and it keeps redraw frames from merging.
        assert_eq!(strip_str(b"\r\x1b[Kfresh\n"), "\nfresh\n");
    }

    #[test]
    fn cr_split_across_feeds_keeps_its_lookahead() {
        let mut sm = Stripper::default();
        let mut out = Vec::new();
        sm.feed(b"line\r", &mut out);
        sm.feed(b"\nnext", &mut out);
        sm.finish(&mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "line\nnext");
    }

    #[test]
    fn invalid_utf8_passes_through_without_panicking() {
        let raw = b"good\xff\xfe\x80bytes\n";
        assert_eq!(strip(raw), raw.to_vec());
        // ...and colour around invalid bytes is still stripped.
        assert_eq!(strip(b"\x1b[31m\xc3\x28\x1b[0m"), b"\xc3\x28".to_vec());
    }

    #[test]
    fn file_line_references_survive_for_thumbs() {
        let input = b"\x1b[1;31merror\x1b[0m: \x1b[4msrc/file.rs:42\x1b[0m: boom\n";
        assert_eq!(strip_str(input), "error: src/file.rs:42: boom\n");
    }

    #[test]
    fn ls_color_output_shape_is_cleaned() {
        let input = b"\x1b[0m\x1b[01;34mplugins\x1b[0m\n\x1b[01;32mtape.sh\x1b[0m\n";
        assert_eq!(strip_str(input), "plugins\ntape.sh\n");
    }

    #[test]
    fn malformed_escapes_never_panic_and_keep_text() {
        // Truncated CSI at EOF, stray ESC before a newline, ESC inside a CSI.
        assert_eq!(strip_str(b"text\x1b[38;5;"), "text");
        assert_eq!(strip_str(b"a\x1b\nb"), "a\nb");
        assert_eq!(strip_str(b"a\x1b[1\x1b[2mb"), "ab");
        // Every byte value, both chunk shapes: must not panic.
        let all: Vec<u8> = (0u8..=255).collect();
        assert_eq!(strip(&all), strip_bytewise(&all));
    }

    #[test]
    fn chunking_never_changes_the_result() {
        let input = b"\x1b[31mred\x1b[0m\r\n\x1b]0;t\x07plain \xff\x1b(Btail\r";
        assert_eq!(strip_bytewise(input), strip(input));
        for split in 1..input.len() {
            let mut sm = Stripper::default();
            let mut out = Vec::new();
            sm.feed(&input[..split], &mut out);
            sm.feed(&input[split..], &mut out);
            sm.finish(&mut out);
            assert_eq!(out, strip(input), "split at {split}");
        }
    }

    #[test]
    fn log_path_refuses_traversal() {
        assert!(log_path("%12").unwrap().ends_with("%12.log"));
        assert!(log_path("").is_none());
        assert!(log_path("../../etc/passwd").is_none());
        assert!(log_path("a/b").is_none());
    }

    #[test]
    fn sh_quote_survives_spaces_and_quotes() {
        assert_eq!(sh_quote("/home/a b/x.log"), "'/home/a b/x.log'");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn dividers_get_their_own_line() {
        let p = std::env::temp_dir().join(format!("cc-tape-test-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&p);

        assert!(ends_with_newline(&p), "a missing log needs no separator");
        std::fs::write(&p, b"").unwrap();
        assert!(ends_with_newline(&p), "an empty log needs no separator");
        std::fs::write(&p, b"finished\n").unwrap();
        assert!(ends_with_newline(&p));
        // The realistic case: a shell prompt left dangling with no newline.
        std::fs::write(&p, b"user@host:~$ ").unwrap();
        assert!(!ends_with_newline(&p));

        divider(&p, "tape off");
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(
            body.lines().any(|l| l.ends_with("── tape off ──")),
            "{body:?}"
        );
        assert!(body.starts_with("user@host:~$ \n"), "{body:?}");

        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn esc_hash_doubles_for_the_format_expander() {
        assert_eq!(esc_hash("/tmp/#{x}.log"), "/tmp/##{x}.log");
        assert_eq!(esc_hash("plain"), "plain");
    }
}
