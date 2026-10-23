//! `win-order` — popup that reorders the windows (tabs) of the current session.
//!
//! Bound to `prefix A` inside `display-popup -E`; see
//! [ARCHITECTURE.org](../ARCHITECTURE.org). Exists so reordering never needs
//! `swap-window -s 0 -t 1` typed from memory: the popup lists the tabs, the
//! footer names every key, and nothing reaches tmux until Enter.
//!
//!   * `j` / `k`  (↓ / ↑)          move the cursor
//!   * `J` / `K`  (Shift+↓ / ↑)    carry the highlighted tab down / up
//!   * Enter                        apply the new order
//!   * Esc / `q`                    close, order untouched
//!
//! The new order is applied as a run of `swap-window` calls over the session's
//! existing index set, so gaps and `base-index` survive; the window that was
//! current stays current. Every path exits 0 — this runs from a key binding,
//! where a non-zero exit surfaces as a tmux error popup.

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use tmuxlib as t;

const FOOTER: &str = "j/k 選  J/K 搬  Enter 套用  Esc 取消";
const W_NAME: usize = 30;

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct Win {
    id: String,
    index: u32,
    name: String,
    active: bool,
}

/// One row per `list-windows` line: `id \t index \t name \t active`. The name
/// is taken from both ends, so a tab inside it cannot shift `active`.
fn parse_windows(out: &str) -> Vec<Win> {
    out.lines()
        .filter_map(|line| {
            let (id, rest) = line.split_once('\t')?;
            let (index, rest) = rest.split_once('\t')?;
            let (name, active) = rest.rsplit_once('\t')?;
            Some(Win {
                id: id.to_string(),
                index: index.parse().ok()?,
                name: name.to_string(),
                active: active == "1",
            })
        })
        .collect()
}

/// The `swap-window` commands that turn `current` into `desired`.
///
/// `slots` is the session's index set in ascending order; `current[i]` and
/// `desired[i]` are the window ids that occupy / should occupy `slots[i]`.
/// Selection-sort shape: fill slot 0, then slot 1, … — at most `n - 1` swaps,
/// none when nothing moved. Targets are `session:index`, so the batch never
/// depends on which session the popup's own tmux client resolves to.
fn plan_swaps(session: &str, slots: &[u32], current: &[String], desired: &[String]) -> Vec<Vec<String>> {
    let mut cur = current.to_vec();
    let mut cmds = Vec::new();
    for i in 0..cur.len() {
        if cur[i] == desired[i] {
            continue;
        }
        let Some(j) = cur.iter().position(|id| *id == desired[i]) else { continue };
        cmds.push(vec![
            "swap-window".to_string(),
            "-d".to_string(),
            "-s".to_string(),
            format!("{session}:{}", slots[j]),
            "-t".to_string(),
            format!("{session}:{}", slots[i]),
        ]);
        cur.swap(i, j);
    }
    cmds
}

// ---------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------

fn main() {
    run();
    std::process::exit(0);
}

fn run() {
    // The popup's shell command gets no format expansion (see the sift block in
    // claude.conf), so ask tmux which session the popup belongs to.
    let session = match t::tmux(["display-message", "-p", "#{session_id}"]) {
        Ok(s) if !s.is_empty() => s,
        _ => return t::message("win-order: not inside tmux"),
    };
    let out = match t::tmux([
        "list-windows",
        "-t",
        &session,
        "-F",
        "#{window_id}\t#{window_index}\t#{window_name}\t#{window_active}",
    ]) {
        Ok(out) => out,
        Err(e) => return t::message_literal(&format!("win-order: {e}")),
    };
    let wins = parse_windows(&out);
    if wins.len() < 2 {
        return t::message("win-order: only one window, nothing to reorder");
    }

    let mut terminal = ratatui::init();
    let picked = pick(&mut terminal, wins.clone());
    ratatui::restore();

    let Some(order) = picked else { return };
    let slots: Vec<u32> = wins.iter().map(|w| w.index).collect();
    let current: Vec<String> = wins.iter().map(|w| w.id.clone()).collect();
    let desired: Vec<String> = order.iter().map(|w| w.id.clone()).collect();
    let mut cmds = plan_swaps(&session, &slots, &current, &desired);
    if cmds.is_empty() {
        return;
    }
    if let Some(active) = wins.iter().find(|w| w.active) {
        cmds.push(vec!["select-window".to_string(), "-t".to_string(), active.id.clone()]);
    }
    if !t::tmux_batch(&cmds) {
        t::message("win-order: swap-window failed, order may be partly applied");
    }
}

// ---------------------------------------------------------------------------
// TUI
// ---------------------------------------------------------------------------

/// Run the key loop. `Some(order)` on Enter, `None` on Esc / q / read error.
fn pick(terminal: &mut DefaultTerminal, mut wins: Vec<Win>) -> Option<Vec<Win>> {
    let slots: Vec<u32> = wins.iter().map(|w| w.index).collect();
    let mut state = ListState::default();
    state.select(Some(wins.iter().position(|w| w.active).unwrap_or(0)));

    loop {
        terminal.draw(|f| draw(f, &wins, &slots, &mut state)).ok()?;
        let Event::Key(key) = event::read().ok()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let i = state.selected().unwrap_or(0);
        let last = wins.len() - 1;
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return None,
            KeyCode::Enter => return Some(wins),
            KeyCode::Char('J') | KeyCode::Down if shift || key.code == KeyCode::Char('J') => {
                if i < last {
                    wins.swap(i, i + 1);
                    state.select(Some(i + 1));
                }
            }
            KeyCode::Char('K') | KeyCode::Up if shift || key.code == KeyCode::Char('K') => {
                if i > 0 {
                    wins.swap(i, i - 1);
                    state.select(Some(i - 1));
                }
            }
            KeyCode::Char('j') | KeyCode::Down => state.select(Some((i + 1).min(last))),
            KeyCode::Char('k') | KeyCode::Up => state.select(Some(i.saturating_sub(1))),
            _ => {}
        }
    }
}

fn draw(f: &mut Frame, wins: &[Win], slots: &[u32], state: &mut ListState) {
    let [list_area, footer_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());

    let items: Vec<ListItem> = wins
        .iter()
        .zip(slots)
        .map(|(w, slot)| {
            let name = t::truncate(&t::sanitize_format(&w.name), W_NAME);
            let mut row = format!("{slot:>3}  {name:<W_NAME$}");
            if w.index != *slot {
                row.push_str(&format!("  (原 {})", w.index));
            }
            if w.active {
                row.push_str("  ●");
            }
            ListItem::new(Line::from(row))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, list_area, state);
    f.render_widget(
        Paragraph::new(FOOTER).style(Style::default().add_modifier(Modifier::DIM)),
        footer_area,
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Replay the swaps on a slot→id model, as tmux would.
    fn apply(slots: &[u32], current: &[String], cmds: &[Vec<String>]) -> Vec<String> {
        let mut cur = current.to_vec();
        let pos = |target: &str| {
            let idx: u32 = target.rsplit(':').next().unwrap().parse().unwrap();
            slots.iter().position(|s| *s == idx).unwrap()
        };
        for c in cmds {
            assert_eq!(c[0], "swap-window");
            let (a, b) = (pos(&c[3]), pos(&c[5]));
            cur.swap(a, b);
        }
        cur
    }

    #[test]
    fn swaps_first_two() {
        let slots = [0, 1, 2, 3, 4];
        let cur = ids(&["@0", "@1", "@2", "@3", "@4"]);
        let want = ids(&["@1", "@0", "@2", "@3", "@4"]);
        let cmds = plan_swaps("$0", &slots, &cur, &want);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0], ids(&["swap-window", "-d", "-s", "$0:1", "-t", "$0:0"]));
        assert_eq!(apply(&slots, &cur, &cmds), want);
    }

    #[test]
    fn moves_last_to_first() {
        let slots = [0, 1, 2, 3];
        let cur = ids(&["@a", "@b", "@c", "@d"]);
        let want = ids(&["@d", "@a", "@b", "@c"]);
        let cmds = plan_swaps("$1", &slots, &cur, &want);
        assert_eq!(apply(&slots, &cur, &cmds), want);
    }

    #[test]
    fn unchanged_order_emits_nothing() {
        let cur = ids(&["@0", "@1", "@2"]);
        assert!(plan_swaps("$0", &[0, 1, 2], &cur, &cur).is_empty());
    }

    #[test]
    fn keeps_gapped_indices() {
        let slots = [1, 3, 5];
        let cur = ids(&["@x", "@y", "@z"]);
        let want = ids(&["@z", "@x", "@y"]);
        let cmds = plan_swaps("$2", &slots, &cur, &want);
        for c in &cmds {
            for target in [&c[3], &c[5]] {
                assert!(["$2:1", "$2:3", "$2:5"].contains(&target.as_str()), "{target}");
            }
        }
        assert_eq!(apply(&slots, &cur, &cmds), want);
    }

    #[test]
    fn parses_list_windows_rows() {
        let out = "@3\t0\tzsh\t0\n@7\t1\tmy\tvim\t1\n";
        let w = parse_windows(out);
        assert_eq!(w.len(), 2);
        assert_eq!(w[1], Win { id: "@7".into(), index: 1, name: "my\tvim".into(), active: true });
    }
}
