//! `cc-fleet` — fzf picker over every Claude Code pane on the server.
//!
//! Bound to `prefix S` inside `display-popup -E`; see
//! [ARCHITECTURE.org](../ARCHITECTURE.org). Two modes:
//!
//!   * `cc-fleet`      — build rows, run fzf, jump to the pick;
//!   * `cc-fleet rows` — print the rows only, for debugging and for an fzf
//!     `reload(…)` binding.
//!
//! Esc is half the feature: the fzf preview *is* a read-only peek at another
//! agent's transcript, so aborting must leave the client exactly where it was.
//! Every path exits 0 — this runs from a key binding, where a non-zero exit
//! surfaces as a tmux error popup.

use std::io::Write;
use std::process::{Command, Stdio};

use tmuxlib as t;

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

const W_STATE: usize = 11; // widest state name is `needs-input`
const W_AGE: usize = 5;
const W_TARGET: usize = 16;
const W_DIR: usize = 16;
const W_TASK: usize = 28;
const W_TAIL: usize = 60;

const HEADER: &str = "Enter: jump   Esc: close (peek only)   ctrl-r: refresh preview";

/// `{1}` is the hidden pane-id field — `--with-nth=2..` keeps it out of the
/// list but leaves it addressable here.
const PREVIEW: &str = "tmux capture-pane -e -p -t {1} -S -200";

// ---------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------

fn main() {
    match std::env::args().nth(1).unwrap_or_default().as_str() {
        "" => pick(),
        "rows" => {
            for row in current_rows() {
                println!("{row}");
            }
        }
        other => eprintln!("cc-fleet: unknown command `{other}` (usage: cc-fleet [rows])"),
    }
    std::process::exit(0);
}

fn pick() {
    let rows = current_rows();
    if rows.is_empty() {
        // The popup closes the instant this returns, so the status line is the
        // only place the human will actually read it.
        println!("cc-fleet: no Claude Code panes on this server.");
        t::message("cc-fleet: no Claude Code panes");
        return;
    }
    // Esc / ctrl-c / no match all land here as `None`: the peek case, no side effects.
    let Some(sel) = run_fzf(&rows) else { return };
    let Some(id) = sel.split('\t').next().filter(|s| !s.is_empty()) else {
        return;
    };
    if let Err(e) = t::focus_pane(None, id) {
        t::message(&format!("cc-fleet: {e}"));
    }
}

fn current_rows() -> Vec<String> {
    build_rows(&t::list_claude_panes(), t::now_epoch(), &last_output_line)
}

// ---------------------------------------------------------------------------
// Rows — pure over `&[Pane]`, so the whole table is testable without a server
// ---------------------------------------------------------------------------

/// One row per pane, attention-first. `tail` supplies the pane's last output
/// line and is only called for panes with no `@claude_activity` — one
/// `capture-pane` per row at worst.
fn build_rows(panes: &[t::Pane], now: u64, tail: &dyn Fn(&t::Pane) -> String) -> Vec<String> {
    sorted(panes, now)
        .into_iter()
        .map(|p| format_row(p, now, tail))
        .collect()
}

/// Neediest first, then longest time-in-state. Panes with no `@claude_since`
/// have no measurable wait, so they sort to the back of their state.
fn sorted(panes: &[t::Pane], now: u64) -> Vec<&t::Pane> {
    let mut v: Vec<&t::Pane> = panes.iter().collect();
    v.sort_by(|a, b| {
        a.state()
            .priority()
            .cmp(&b.state().priority())
            .then_with(|| {
                b.age_secs(now)
                    .unwrap_or(0)
                    .cmp(&a.age_secs(now).unwrap_or(0))
            })
            .then_with(|| a.id.cmp(&b.id))
    });
    v
}

/// `<pane id>\t<display columns>` — exactly one tab, because the id is field 1
/// and everything after it is what `--with-nth=2..` shows.
fn format_row(p: &t::Pane, now: u64, tail: &dyn Fn(&t::Pane) -> String) -> String {
    let state = p.state();
    let label = if p.task.is_empty() { &p.title } else { &p.task };
    let detail = if p.activity.is_empty() {
        tail(p)
    } else {
        p.activity.clone()
    };

    format!(
        "{}\t{} {} {:>aw$}  {} {} {} │ {}",
        p.id,
        state.glyph(),
        fit(state_name(state), W_STATE),
        t::human_age(p.age_secs(now)),
        fit(&cell(&p.target()), W_TARGET),
        fit(&cell(dir_base(&p.path)), W_DIR),
        fit(&cell(label), W_TASK),
        clip(&cell(&detail), W_TAIL),
        aw = W_AGE,
    )
}

/// `State::Unknown` renders as the empty string on the border; a picker column
/// needs a word.
fn state_name(state: t::State) -> &'static str {
    match state {
        t::State::Unknown => "unknown",
        s => s.as_str(),
    }
}

fn dir_base(path: &str) -> &str {
    path.rsplit('/').find(|s| !s.is_empty()).unwrap_or("/")
}

/// Clip to `cols` terminal columns, then pad to exactly that width. Padding by
/// `char` count would shear every column right of a Chinese task slug, which is
/// most of them on this machine.
fn fit(s: &str, cols: usize) -> String {
    let mut out = clip(s, cols);
    for _ in width(&out)..cols {
        out.push(' ');
    }
    out
}

/// Clip to `cols` terminal columns, marking the cut with `…`.
fn clip(s: &str, cols: usize) -> String {
    if width(s) <= cols {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = char_width(c);
        if used + w > cols.saturating_sub(1) {
            break;
        }
        used += w;
        out.push(c);
    }
    out.push('…');
    out
}

fn width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// Only the East Asian Wide/Fullwidth blocks count double. The state glyphs
/// (`● ◐ ✋ ○`), `✳` and the braille spinners are East-Asian-*Ambiguous* and
/// render single-width in this terminal.
fn char_width(c: char) -> usize {
    match c as u32 {
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE10..=0xFE19
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF => 2,
        _ => 1,
    }
}

/// One display cell: no tabs (the field delimiter) and no escape sequences —
/// fzf runs with `--ansi`, so a stray CSI would colour the rest of the list.
fn cell(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            // Consume the whole CSI rather than leaving its `[0m` tail behind.
            if it.peek() == Some(&'[') {
                it.next();
                for n in it.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
            }
            continue;
        }
        if !c.is_control() {
            out.push(c);
        }
    }
    out.trim().to_string()
}

/// Last non-blank line the pane printed. Five lines of scrollback is enough to
/// clear a trailing prompt and cheap enough to run once per pane.
fn last_output_line(p: &t::Pane) -> String {
    let out = t::tmux(["capture-pane", "-p", "-t", &p.id, "-S", "-5"]).unwrap_or_default();
    out.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(cell)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// fzf
// ---------------------------------------------------------------------------

/// Rows in on stdin, selection out on stdout; fzf draws its TUI on `/dev/tty`,
/// so the popup renders even with both pipes redirected.
fn run_fzf(rows: &[String]) -> Option<String> {
    let mut child = match Command::new("fzf")
        .arg("--ansi")
        .arg("--delimiter=\t")
        .arg("--with-nth=2..")
        .arg("--no-sort")
        .arg(format!("--header={HEADER}"))
        .arg(format!("--preview={PREVIEW}"))
        .arg("--preview-window=right,60%")
        .arg("--bind=ctrl-r:refresh-preview")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            t::message(&format!("cc-fleet: cannot run fzf: {e}"));
            return None;
        }
    };

    // Dropping the pipe closes fzf's input; the row set is far smaller than the
    // pipe buffer, so a writer thread would buy nothing.
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(rows.join("\n").as_bytes());
        let _ = stdin.write_all(b"\n");
    }

    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    let sel = String::from_utf8_lossy(&out.stdout);
    let sel = sel.trim_end_matches('\n');
    (!sel.is_empty()).then(|| sel.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 10_000;

    fn pane(id: &str, state: &str, since: Option<u64>) -> t::Pane {
        t::Pane {
            id: id.into(),
            session: "main".into(),
            window_index: "1".into(),
            pane_index: "0".into(),
            path: "/home/fenrir/code/backend".into(),
            state_raw: state.into(),
            since,
            ..Default::default()
        }
    }

    fn no_tail(_: &t::Pane) -> String {
        String::new()
    }

    fn ids(rows: &[String]) -> Vec<String> {
        rows.iter()
            .map(|r| r.split('\t').next().unwrap().to_string())
            .collect()
    }

    #[test]
    fn attention_sorts_before_idle_before_busy_before_unknown() {
        let panes = vec![
            pane("%busy", "busy", Some(NOW - 10)),
            pane("%unknown", "", Some(NOW - 10)),
            pane("%needs", "needs-input", Some(NOW - 10)),
            pane("%idle", "idle", Some(NOW - 10)),
        ];
        let rows = build_rows(&panes, NOW, &no_tail);
        assert_eq!(ids(&rows), ["%needs", "%idle", "%busy", "%unknown"]);
    }

    #[test]
    fn within_a_state_the_longest_wait_is_on_top() {
        let panes = vec![
            pane("%new", "idle", Some(NOW - 60)),
            pane("%oldest", "idle", Some(NOW - 7200)),
            pane("%mid", "idle", Some(NOW - 600)),
            pane("%unstamped", "idle", None),
        ];
        let rows = build_rows(&panes, NOW, &no_tail);
        assert_eq!(ids(&rows), ["%oldest", "%mid", "%new", "%unstamped"]);
    }

    #[test]
    fn pane_id_is_field_one_and_nothing_else_carries_a_tab() {
        let mut p = pane("%42", "idle", Some(NOW - 120));
        p.title = "✳ fix\tauth".into();
        p.activity = "Bash\tgo test".into();
        let rows = build_rows(&[p], NOW, &no_tail);
        let fields: Vec<&str> = rows[0].split('\t').collect();
        assert_eq!(fields.len(), 2, "row must be id + one display field");
        assert_eq!(fields[0], "%42");
        assert!(fields[1].contains("main:1.0"));
        assert!(fields[1].contains("backend"));
    }

    #[test]
    fn a_pane_without_claude_since_renders_an_em_dash_age() {
        let rows = build_rows(&[pane("%7", "busy", None)], NOW, &no_tail);
        assert!(
            rows[0].contains(" — "),
            "expected em-dash age in {:?}",
            rows[0]
        );
    }

    #[test]
    fn long_activity_is_truncated_instead_of_breaking_the_columns() {
        let mut p = pane("%9", "busy", Some(NOW - 30));
        p.activity = "x".repeat(400);
        let rows = build_rows(&[p], NOW, &no_tail);
        let detail = rows[0].rsplit('│').next().unwrap().trim();
        assert_eq!(detail.chars().count(), W_TAIL);
        assert!(detail.ends_with('…'));
    }

    #[test]
    fn activity_wins_over_the_tail_and_the_tail_is_read_only_when_needed() {
        let mut busy = pane("%busy", "busy", Some(NOW - 5));
        busy.activity = "Bash go test ./...".into();
        let idle = pane("%idle", "idle", Some(NOW - 5));
        let tail = |p: &t::Pane| format!("tail-of-{}", p.id);
        let rows = build_rows(&[busy, idle], NOW, &tail);
        assert!(rows.iter().any(|r| r.contains("Bash go test ./...")));
        assert!(rows.iter().any(|r| r.ends_with("tail-of-%idle")));
        assert!(!rows.iter().any(|r| r.contains("tail-of-%busy")));
    }

    #[test]
    fn task_slug_beats_the_pane_title() {
        let mut p = pane("%3", "idle", Some(NOW - 5));
        p.title = "✳ Claude Code".into();
        p.task = "fix-auth-middleware".into();
        let rows = build_rows(&[p], NOW, &no_tail);
        assert!(rows[0].contains("fix-auth-middleware"));
        assert!(!rows[0].contains("Claude Code"));
    }

    #[test]
    fn no_panes_yields_no_rows() {
        assert!(build_rows(&[], NOW, &no_tail).is_empty());
    }

    #[test]
    fn cells_drop_ansi_and_control_characters_whole() {
        assert_eq!(cell("\u{1b}[31mred\u{1b}[0m"), "red");
        assert_eq!(cell("a\u{7}b\tc"), "abc");
        assert_eq!(cell("  padded  "), "padded");
    }

    #[test]
    fn columns_are_padded_by_terminal_width_not_char_count() {
        assert_eq!(width(&fit("設計 tmux 功能", 20)), 20);
        assert_eq!(width(&fit("ascii", 20)), 20);
        // A cut landing on a wide char can leave one column short; `fit` pads
        // it back, which is why the mixed-script rows below still line up.
        assert!(width(&clip("設計 tmux 上的 Claude 功能", 12)) <= 12);
        assert_eq!(width(&fit("設計 tmux 上的 Claude 功能", 12)), 12);
        assert!(clip("設計功能", 5).ends_with('…'));
        assert_eq!(clip("設計功能", 99), "設計功能");
    }

    #[test]
    fn rows_of_mixed_scripts_keep_the_same_column_width() {
        let mut cjk = pane("%1", "idle", Some(NOW - 60));
        cjk.task = "設計 tmux 上的功能".into();
        let mut ascii = pane("%2", "idle", Some(NOW - 60));
        ascii.task = "fix-auth".into();
        let rows = build_rows(&[cjk, ascii], NOW, &no_tail);
        let bar_col = |r: &String| width(r.split('\t').nth(1).unwrap().split('│').next().unwrap());
        assert_eq!(bar_col(&rows[0]), bar_col(&rows[1]));
    }

    #[test]
    fn dir_base_takes_the_last_path_segment() {
        assert_eq!(dir_base("/home/fenrir/code/backend"), "backend");
        assert_eq!(dir_base("/home/fenrir/"), "fenrir");
        assert_eq!(dir_base("/"), "/");
    }

    #[test]
    fn unknown_state_still_names_a_column() {
        let rows = build_rows(&[pane("%1", "", None)], NOW, &no_tail);
        assert!(rows[0].contains("○ unknown"));
    }
}
