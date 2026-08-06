//! `cc-launch <resume|project|pair>` — the three scripted entries of the
//! `prefix C` Claude launch menu. The menu's other entries (split, new-window,
//! popup, continue) are tmux one-liners and live in `tmux.conf`.
//!
//!   * `resume <pane_id> <cwd>`  — fzf over this project's past transcripts,
//!     then `claude --resume <uuid>` into the calling pane. Runs in a popup.
//!   * `project`                 — zoxide + fzf; jump to an existing Claude
//!     window for that directory, else open one. Runs in a popup.
//!   * `pair <pane_id> <cwd>`    — split a second Claude and run the mirrored
//!     `/pair` handshake into both. Runs detached (`run-shell -b`).
//!
//! Every path exits 0: these run from a key binding, where a non-zero exit
//! surfaces as a tmux error popup.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, UNIX_EPOCH};

use tmuxlib as t;

/// Newest transcripts offered by the resume picker.
const MAX_TRANSCRIPTS: usize = 30;
/// Excerpt width in the picker; the terminal is narrower than most prompts.
const EXCERPT_CHARS: usize = 80;
/// A transcript's first prompt is within its first few lines; a file where it
/// is not is malformed, and scanning megabytes to prove that is not worth it.
const SCAN_LINES: usize = 400;

const READY_TIMEOUT: Duration = Duration::from_secs(30);
const READY_POLL: Duration = Duration::from_millis(500);
/// Gap between literal text and its Enter — see [`send_line`].
const ENTER_DELAY: Duration = Duration::from_millis(300);
/// Let the newcomer's mq reader come up before mirroring into the caller.
const HANDSHAKE_GAP: Duration = Duration::from_secs(2);
/// A `display-popup -E` closes the instant the command exits.
const NOTICE_PAUSE: Duration = Duration::from_millis(1500);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a = |n: usize| args.get(n).map(String::as_str).unwrap_or_default();

    match a(0) {
        "resume" if args.len() >= 3 => resume(a(1), a(2)),
        "project" => project(),
        "pair" if args.len() >= 3 => pair(a(1), a(2)),
        _ => usage(),
    }
    std::process::exit(0);
}

fn usage() {
    eprintln!(
        "usage: cc-launch resume <pane_id> <cwd>\n\
         \x20      cc-launch project\n\
         \x20      cc-launch pair <caller_pane_id> <cwd>"
    );
}

// ---------------------------------------------------------------------------
// resume — per-project transcript picker
// ---------------------------------------------------------------------------

fn resume(pane: &str, cwd: &str) {
    let files = transcripts(&t::home(), cwd, MAX_TRANSCRIPTS);
    if files.is_empty() {
        notice(&format!("no past Claude sessions for {cwd}"));
        return;
    }

    let tz = tz_offset_secs();
    let mut menu = String::new();
    for f in &files {
        let uuid = f
            .path
            .file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        let excerpt = first_prompt_excerpt(&f.path).unwrap_or_else(|| "<no prompt>".to_string());
        menu.push_str(&format!(
            "{uuid}\t{}  {excerpt}\n",
            format_stamp(f.mtime, tz)
        ));
    }

    // The uuid rides as a hidden first field: `--with-nth=2..` shows the rest.
    let Some(pick) = fzf(
        &menu,
        &["--delimiter=\t", "--with-nth=2..", "--prompt=resume> "],
    ) else {
        return;
    };
    let Some(uuid) = parse_pick(&pick) else {
        return;
    };

    // Claude Code rejects abbreviated ids, so the full uuid goes across.
    send_line(pane, &format!("claude --resume {uuid}"));
    t::message(&format!("resuming {uuid} in {pane}"));
}

struct Transcript {
    path: PathBuf,
    mtime: i64,
}

/// The `<max>` newest `*.jsonl` for `cwd`, newest first.
///
/// Only the project dir's immediate children count — `<uuid>/subagents/*.jsonl`
/// are sub-agent side transcripts and are not resumable sessions.
fn transcripts(home: &Path, cwd: &str, max: usize) -> Vec<Transcript> {
    let root = home.join(".claude/projects");
    let mut dirs = vec![root.join(project_slug(cwd))];
    let legacy = root.join(legacy_slug(cwd));
    if legacy != dirs[0] {
        dirs.push(legacy);
    }

    let mut out: Vec<Transcript> = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(md) = e.metadata() else { continue };
            if !md.is_file() {
                continue;
            }
            out.push(Transcript {
                path,
                mtime: mtime_secs(&md),
            });
        }
    }
    out.sort_by(|a, b| b.mtime.cmp(&a.mtime));
    out.truncate(max);
    out
}

/// Claude Code names a project dir after its cwd with `/` **and `.`** both
/// flattened to `-`; case and `_` are preserved.
///
/// Verified against `~/.claude/projects` on this machine 2026-08-06:
/// `/home/fenrir/.tmux` → `-home-fenrir--tmux`, `/home/fenrir/code/pictur_with_day`
/// → `-home-fenrir-code-pictur_with_day`.
fn project_slug(cwd: &str) -> String {
    normalize_dir(cwd)
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect()
}

/// The pre-April-2026 spelling, which flattened only `/` and left the dot
/// alone: `-home-fenrir-.claude` (last written 2026-04-01) sits next to today's
/// `-home-fenrir--claude`. Old sessions are still resumable, so both are read.
fn legacy_slug(cwd: &str) -> String {
    normalize_dir(cwd).replace('/', "-")
}

/// Drop a trailing slash so `/home/fenrir/` and `/home/fenrir` slug alike.
fn normalize_dir(dir: &str) -> &str {
    let trimmed = dir.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        "/"
    } else {
        trimmed
    }
}

/// The session's opening prompt, read incrementally — transcripts run to
/// megabytes and only the head is ever interesting.
fn first_prompt_excerpt(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    for line in BufReader::new(file).lines().take(SCAN_LINES) {
        // A non-UTF-8 read means the rest of this file is unusable, not that
        // the next line might parse.
        let Ok(line) = line else { break };
        if let Some(text) = user_text(&line) {
            return Some(text);
        }
    }
    None
}

/// One transcript line → the human's typed text, if that is what it is.
///
/// `message.content` comes in two shapes: a bare string, or an array of blocks
/// where the prompt is the first `{"type":"text"}` one. Tool results, meta
/// lines and sidechain (sub-agent) turns are not prompts.
fn user_text(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(|x| x.as_str()) != Some("user") {
        return None;
    }
    for flag in ["isMeta", "isSidechain"] {
        if v.get(flag).and_then(|x| x.as_bool()).unwrap_or(false) {
            return None;
        }
    }

    let content = v.get("message")?.get("content")?;
    let text = match content {
        serde_json::Value::String(s) => s.as_str(),
        serde_json::Value::Array(blocks) => blocks
            .iter()
            .find(|b| b.get("type").and_then(|x| x.as_str()) == Some("text"))
            .and_then(|b| b.get("text"))
            .and_then(|x| x.as_str())?,
        _ => return None,
    };

    let text = unwrap_command(text);
    // Tabs would split the picker line's hidden-uuid field.
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        None
    } else {
        Some(t::truncate(&flat, EXCERPT_CHARS))
    }
}

/// A typed slash command is stored as an XML-ish wrapper
/// (`<command-name>/goal</command-name>…<command-args>…</command-args>`).
/// Shown raw it fills the picker line with markup, so it collapses to what the
/// human actually typed.
fn unwrap_command(text: &str) -> String {
    let Some(name) = tag(text, "command-name") else {
        return text.to_string();
    };
    match tag(text, "command-args")
        .map(str::trim)
        .filter(|a| !a.is_empty())
    {
        Some(args) => format!("{} {args}", name.trim()),
        None => name.trim().to_string(),
    }
}

fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = s.find(&open)? + open.len();
    let end = s[start..].find(&format!("</{name}>"))? + start;
    Some(&s[start..end])
}

/// `<uuid>\t<stamp>  <excerpt>` → the uuid. fzf echoes the whole line back,
/// hidden field included.
fn parse_pick(line: &str) -> Option<&str> {
    let id = line.split('\t').next()?.trim();
    (!id.is_empty()).then_some(id)
}

// ---------------------------------------------------------------------------
// project — zoxide + fzf jump-or-create
// ---------------------------------------------------------------------------

fn project() {
    let Some(dirs) = capture("zoxide", &["query", "-l"]) else {
        notice("zoxide is not available");
        return;
    };
    if dirs.trim().is_empty() {
        notice("zoxide knows no directories yet");
        return;
    }

    let Some(pick) = fzf(&dirs, &["--prompt=claude project> "]) else {
        return;
    };
    if pick.trim().is_empty() {
        return;
    }
    let dir = normalize_dir(&pick);

    // The whole point of this entry: never open a second Claude on a project
    // that already has one two windows away. A failed jump reports rather than
    // falling through to new-window, which would create the duplicate anyway.
    if let Some(id) = existing_pane(&t::list_panes(), dir) {
        match t::focus_pane(None, &id) {
            Ok(()) => t::message(&format!("jumped to {id} — {dir}")),
            Err(e) => t::message(&format!("cc-launch: could not jump to {id}: {e}")),
        }
        return;
    }

    let name = t::sanitize_format(&format!("cc:{}", basename(dir)));
    if t::tmux_ok(["new-window", "-c", dir, "-n", &name, "claude"]) {
        t::message(&format!("new claude window {name} — {dir}"));
    } else {
        t::message(&format!("could not open a window in {dir}"));
    }
}

/// A pane already sitting in `dir`, preferring one that is actually running
/// Claude — jumping to a bare shell that happens to be `cd`'d there would
/// defeat the entry.
fn existing_pane(panes: &[t::Pane], dir: &str) -> Option<String> {
    let here = || panes.iter().filter(|p| normalize_dir(&p.path) == dir);
    here()
        .find(|p| p.is_claude())
        .or_else(|| here().next())
        .map(|p| p.id.clone())
}

fn basename(dir: &str) -> &str {
    dir.rsplit('/').find(|s| !s.is_empty()).unwrap_or("claude")
}

// ---------------------------------------------------------------------------
// pair — automated /pair handshake
// ---------------------------------------------------------------------------

fn pair(caller: &str, cwd: &str) {
    if !t::pane_alive(caller) {
        t::message(&format!("cc-launch pair: no such pane {caller}"));
        return;
    }

    let Ok(new) = t::tmux([
        "split-window",
        "-h",
        "-c",
        cwd,
        "-P",
        "-F",
        "#{pane_id}",
        "claude",
    ]) else {
        t::message("cc-launch pair: split-window failed");
        return;
    };
    let new = new.trim().to_string();
    let (mine, theirs) = (topic(&new), topic(caller));

    // A half-finished handshake is worse than none: the caller would sit
    // listening on a channel nobody publishes to.
    if !wait_ready(&new) {
        t::message(&format!(
            "cc-launch pair: {new} not ready after {}s — no /pair sent",
            READY_TIMEOUT.as_secs()
        ));
        return;
    }

    // pair.md's argument order is (other's channel, my channel), so the two
    // panes get mirrored lines.
    send_line(&new, &format!("/pair {theirs} {mine}"));
    std::thread::sleep(HANDSHAKE_GAP);
    send_line(caller, &format!("/pair {mine} {theirs}"));
    t::message(&format!("paired {caller} [{theirs}] ↔ {new} [{mine}]"));
}

/// `%17` → `p17`. Mirrors the pane-id convention the talk banner already uses
/// (`msg from %N`), minus the sigil, which mq topics cannot carry.
fn topic(pane_id: &str) -> String {
    format!("p{}", pane_id.trim().trim_start_matches('%'))
}

/// Block until the pane's Claude is accepting input, or the cap expires.
///
/// The readiness signal is the pane title: Claude Code writes `✳ Claude Code`
/// when idle, which is exactly what [`tmuxlib::title_state`] already decodes.
/// The plan's `? for shortcuts` splash banner was never verified and is not
/// used.
fn wait_ready(pane: &str) -> bool {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if !t::pane_alive(pane) {
            return false;
        }
        let title = t::display(Some(pane), "#{pane_title}").unwrap_or_default();
        if t::title_state(&title) == t::State::Idle {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(READY_POLL);
    }
}

/// Literal text, then Enter as a second call.
///
/// `talk`'s `send_text` needs no gap because `paste-buffer -p` delivers the
/// whole line atomically as a bracketed paste. A slash command must arrive as
/// *typed* text for Claude Code to treat it as a command, and `send-keys -l`
/// is not atomic — the autocomplete popup it opens will eat an Enter that
/// arrives in the same instant.
fn send_line(pane: &str, text: &str) {
    t::tmux_ok(["send-keys", "-t", pane, "-l", text]);
    std::thread::sleep(ENTER_DELAY);
    t::tmux_ok(["send-keys", "-t", pane, "Enter"]);
}

// ---------------------------------------------------------------------------
// Subprocesses
// ---------------------------------------------------------------------------

/// Feed `input` to fzf and return the chosen line. `None` covers every
/// non-choice: Esc (130), no match (1), fzf missing.
fn fzf(input: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new("fzf")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;

    // Write from a thread: a list longer than the pipe buffer would otherwise
    // deadlock against fzf while we wait on its output.
    let mut sink = child.stdin.take()?;
    let payload = input.to_string();
    let writer = std::thread::spawn(move || {
        let _ = sink.write_all(payload.as_bytes());
    });

    let out = child.wait_with_output().ok()?;
    let _ = writer.join();
    if !out.status.success() {
        return None;
    }
    let pick = String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_string();
    (!pick.is_empty()).then_some(pick)
}

fn capture(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// Popups vanish the moment the command exits, so a bare println would flash
/// past unread.
fn notice(msg: &str) {
    println!("{msg}");
    let _ = std::io::stdout().flush();
    std::thread::sleep(NOTICE_PAUSE);
}

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

fn mtime_secs(md: &fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// One `date +%z` for the whole picker, instead of a `date -r` fork per file.
fn tz_offset_secs() -> i64 {
    capture("date", &["+%z"]).map(|z| parse_tz(&z)).unwrap_or(0)
}

fn parse_tz(z: &str) -> i64 {
    let z = z.trim();
    let (sign, digits) = match z.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, z.strip_prefix('+').unwrap_or(z)),
    };
    if digits.len() < 4 || !digits.is_char_boundary(4) {
        return 0;
    }
    let h: i64 = digits[0..2].parse().unwrap_or(0);
    let m: i64 = digits[2..4].parse().unwrap_or(0);
    sign * (h * 3600 + m * 60)
}

/// `mm-dd HH:MM` in local time — the picker's first visible column.
fn format_stamp(epoch: i64, tz_offset: i64) -> String {
    let local = epoch + tz_offset;
    let (_, month, day) = civil_from_days(local.div_euclid(86_400));
    let secs = local.rem_euclid(86_400);
    format!(
        "{month:02}-{day:02} {:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60
    )
}

/// Days since the Unix epoch → (year, month, day), via Howard Hinnant's
/// `civil_from_days`. Cheaper than a `date` fork per transcript and, unlike
/// one, unit-testable.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11], March-based
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- slugs ---------------------------------------------------------------

    #[test]
    fn slug_flattens_slashes_and_dots() {
        // Verified against ~/.claude/projects on 2026-08-06.
        assert_eq!(project_slug("/home/fenrir/.tmux"), "-home-fenrir--tmux");
        assert_eq!(
            project_slug("/home/fenrir/.claude/skills"),
            "-home-fenrir--claude-skills"
        );
        // Case and underscores survive.
        assert_eq!(
            project_slug("/home/fenrir/code/SkillOpt/pictur_with_day"),
            "-home-fenrir-code-SkillOpt-pictur_with_day"
        );
    }

    #[test]
    fn legacy_slug_keeps_the_dot() {
        assert_eq!(legacy_slug("/home/fenrir/.claude"), "-home-fenrir-.claude");
        // Dotless paths slug identically under both rules.
        assert_eq!(
            legacy_slug("/home/fenrir/code"),
            project_slug("/home/fenrir/code")
        );
    }

    #[test]
    fn slug_ignores_a_trailing_slash() {
        assert_eq!(project_slug("/home/fenrir/"), project_slug("/home/fenrir"));
        assert_eq!(project_slug("/"), "-");
    }

    // -- transcript excerpts -------------------------------------------------

    const STRING_CONTENT: &str = r#"{"type":"user","message":{"role":"user","content":"commit it"},"cwd":"/home/fenrir/.tmux"}"#;
    const ARRAY_CONTENT: &str = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"design the launch menu"}]}}"#;
    const TOOL_RESULT: &str = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01","content":"ok"}]}}"#;
    const BROKEN: &str = r#"{"type":"user","message":{"content":"truncated"#;

    #[test]
    fn user_text_reads_plain_string_content() {
        assert_eq!(user_text(STRING_CONTENT).as_deref(), Some("commit it"));
    }

    #[test]
    fn user_text_reads_the_first_text_block() {
        assert_eq!(
            user_text(ARRAY_CONTENT).as_deref(),
            Some("design the launch menu")
        );
        // A tool result ahead of the prompt does not shadow it.
        let mixed = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"x"},{"type":"text","text":"then this"}]}}"#;
        assert_eq!(user_text(mixed).as_deref(), Some("then this"));
    }

    #[test]
    fn user_text_skips_non_prompts() {
        assert_eq!(user_text(TOOL_RESULT), None);
        assert_eq!(user_text(BROKEN), None);
        assert_eq!(
            user_text(r#"{"type":"assistant","message":{"content":"hi"}}"#),
            None
        );
        assert_eq!(
            user_text(r#"{"type":"user","isMeta":true,"message":{"content":"Caveat: ..."}}"#),
            None
        );
        assert_eq!(
            user_text(r#"{"type":"user","isSidechain":true,"message":{"content":"sub-agent"}}"#),
            None
        );
        assert_eq!(user_text(r#"{"type":"user","message":{}}"#), None);
        assert_eq!(
            user_text(r#"{"type":"user","message":{"content":"   "}}"#),
            None
        );
        assert_eq!(user_text(""), None);
    }

    #[test]
    fn user_text_flattens_and_truncates() {
        let line = r#"{"type":"user","message":{"content":"line\tone\nline two"}}"#;
        assert_eq!(user_text(line).as_deref(), Some("line one line two"));

        let long = "a".repeat(200);
        let line = format!(r#"{{"type":"user","message":{{"content":"{long}"}}}}"#);
        let got = user_text(&line).unwrap();
        assert_eq!(got.chars().count(), EXCERPT_CHARS);
        assert!(got.ends_with('…'));
    }

    #[test]
    fn first_prompt_excerpt_walks_past_the_preamble() {
        let path =
            std::env::temp_dir().join(format!("cc-launch-fixture-{}.jsonl", std::process::id()));
        let body = format!(
            "{}\n{}\n{}\n{}\n{}\n",
            r#"{"type":"last-prompt","leafUuid":"1f5d"}"#, // session preamble
            BROKEN,
            TOOL_RESULT,
            ARRAY_CONTENT,
            STRING_CONTENT,
        );
        fs::write(&path, body).unwrap();
        let got = first_prompt_excerpt(&path);
        let _ = fs::remove_file(&path);
        assert_eq!(got.as_deref(), Some("design the launch menu"));

        assert_eq!(
            first_prompt_excerpt(Path::new("/nonexistent/x.jsonl")),
            None
        );
    }

    #[test]
    fn user_text_collapses_slash_command_markup() {
        let line = r#"{"type":"user","message":{"content":"<command-name>/goal</command-name>\n  <command-message>goal</command-message>\n  <command-args>run the plan</command-args>"}}"#;
        assert_eq!(user_text(line).as_deref(), Some("/goal run the plan"));
        // Argument-less commands keep just the name.
        assert_eq!(
            unwrap_command("<command-name>/clear</command-name>"),
            "/clear"
        );
        // An unterminated tag is left alone rather than half-parsed.
        assert_eq!(unwrap_command("<command-name>/goal"), "<command-name>/goal");
        assert_eq!(unwrap_command("plain prompt"), "plain prompt");
    }

    #[test]
    fn parse_pick_takes_the_hidden_uuid_field() {
        assert_eq!(
            parse_pick("4feec49e-3ed1-4937-89c5-8e814cf26e6a\t08-06 21:04  ultracode fanout"),
            Some("4feec49e-3ed1-4937-89c5-8e814cf26e6a")
        );
        assert_eq!(parse_pick(""), None);
        assert_eq!(parse_pick("\t08-06 21:04  orphan"), None);
    }

    // -- project picker ------------------------------------------------------

    #[test]
    fn existing_pane_prefers_a_claude_over_a_shell() {
        let panes = vec![
            t::Pane {
                id: "%1".into(),
                path: "/home/fenrir/code/x".into(),
                ..Default::default()
            },
            t::Pane {
                id: "%2".into(),
                path: "/home/fenrir/code/x/".into(),
                title: "✳ Claude Code".into(),
                ..Default::default()
            },
            t::Pane {
                id: "%3".into(),
                path: "/tmp".into(),
                ..Default::default()
            },
        ];
        assert_eq!(
            existing_pane(&panes, "/home/fenrir/code/x").as_deref(),
            Some("%2")
        );
        // No Claude there — a plain shell in the directory still beats a new window.
        assert_eq!(existing_pane(&panes, "/tmp").as_deref(), Some("%3"));
        assert_eq!(existing_pane(&panes, "/home/fenrir/code/y"), None);
    }

    #[test]
    fn basename_names_the_window() {
        assert_eq!(
            basename("/home/fenrir/code/afb-recogniztion"),
            "afb-recogniztion"
        );
        assert_eq!(basename("/home/fenrir/"), "fenrir");
        assert_eq!(basename("/"), "claude");
    }

    // -- pair ----------------------------------------------------------------

    #[test]
    fn topic_strips_the_pane_sigil() {
        assert_eq!(topic("%17"), "p17");
        assert_eq!(topic(" %3\n"), "p3");
        // Already-bare ids pass through, so a caller id from a format string
        // that lost its `%` still round-trips.
        assert_eq!(topic("5"), "p5");
    }

    // -- time ----------------------------------------------------------------

    #[test]
    fn parse_tz_handles_both_signs() {
        assert_eq!(parse_tz("+0800"), 8 * 3600);
        assert_eq!(parse_tz("-0530\n"), -(5 * 3600 + 30 * 60));
        assert_eq!(parse_tz("+0000"), 0);
        assert_eq!(parse_tz("junk"), 0);
    }

    #[test]
    fn format_stamp_is_local_civil_time() {
        // 1786021485 = 2026-08-06 13:04:45 UTC (verified with `date -d`).
        assert_eq!(format_stamp(1_786_021_485, 0), "08-06 13:04");
        assert_eq!(format_stamp(1_786_021_485, 8 * 3600), "08-06 21:04");
        // 1709251140 = 2024-02-29 23:59 UTC — leap day, and +1h rolls the month.
        assert_eq!(format_stamp(1_709_251_140, 0), "02-29 23:59");
        assert_eq!(format_stamp(1_709_251_140, 3600), "03-01 00:59");
        assert_eq!(format_stamp(0, 0), "01-01 00:00");
        // Negative local time must floor, not truncate toward zero.
        assert_eq!(format_stamp(0, -3600), "12-31 23:00");
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }
}
