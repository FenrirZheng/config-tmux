//! `cc-attend jump|back <client_name> <pane_id>` — the two attention keys.
//!
//! Bound to `prefix N` / `prefix B`; tmux expands `#{client_name}` and
//! `#{pane_id}` at bind time, so both arguments arrive already resolved (and may
//! arrive empty, which every path below tolerates).
//!
//! `jump` reads the beacon options that `cc-beacon` stamps (`@claude_state`,
//! `@claude_since`) and goes to the pane that has been waiting on a human the
//! longest, marking the pane you left so `back` can return. The mark is stock
//! tmux state (`select-pane -m`), which is why `back` needs no bookkeeping of
//! its own and why pressing `B` repeatedly toggles between the two panes.
//!
//! Every path exits 0 — these run from key bindings, where a non-zero exit
//! surfaces as a tmux error popup.

use tmuxlib as t;
use tmuxlib::{Pane, State};

/// An `idle` pane older than this is assumed abandoned — a Claude that exited
/// leaves its last `idle` stamp behind forever, and that ghost must not win the
/// fallback over a live pane. `needs-input` is never aged out: a human was
/// asked for something and never answered, however long ago.
const STALE_IDLE_SECS: u64 = 2 * 60 * 60;

fn main() {
    run();
    std::process::exit(0);
}

fn run() {
    let mut args = std::env::args().skip(1);
    let sub = args.next().unwrap_or_default();
    let client = args.next().unwrap_or_default();
    let pane = args.next().unwrap_or_default();

    match sub.as_str() {
        "jump" => jump(&client, &pane),
        "back" => back(&client, &pane),
        _ => t::message("cc-attend: usage: cc-attend jump|back <client> <pane_id>"),
    }
}

// ---------------------------------------------------------------------------
// prefix N — jump to whoever has been waiting longest
// ---------------------------------------------------------------------------

fn jump(client: &str, origin: &str) {
    let panes = t::list_claude_panes();
    let now = t::now_epoch();

    let Some(target) = pick_target(&panes, now, origin) else {
        t::message("attend: no Claude pane needs attention");
        return;
    };

    // Mark before jumping: a switch that fails part-way still leaves a usable
    // return path. Never re-mark the pane we are about to land on — `-m` on the
    // already-marked pane clears the mark (verified, tmux 3.5a).
    if !origin.is_empty() && origin != target.id {
        t::tmux_ok(["select-pane", "-m", "-t", origin]);
    }

    match t::focus_pane(Some(client), &target.id) {
        Ok(()) => t::message(&describe(target, now)),
        Err(e) => t::message(&t::sanitize_format(&format!("attend: {e}"))),
    }
}

/// The pane that most needs a human, or `None`.
///
/// Pure over its inputs so the ranking is testable without a tmux server.
/// Order: oldest `needs-input` first (longest blocked), else newest `idle`
/// (most recently finished turn, i.e. the one you were most likely waiting on).
/// A pane with no `@claude_since` cannot be ranked by age, so it sorts last
/// within its state group rather than being dropped.
fn pick_target<'a>(panes: &'a [Pane], now: u64, origin: &str) -> Option<&'a Pane> {
    let live = || panes.iter().filter(|p| p.id != origin || origin.is_empty());

    let blocked = live()
        .filter(|p| p.state() == State::NeedsInput)
        .min_by_key(|p| (p.since.is_none(), p.since.unwrap_or(0)));
    if blocked.is_some() {
        return blocked;
    }

    live()
        .filter(|p| p.state() == State::Idle && !is_stale_idle(p, now))
        .max_by_key(|p| (p.since.is_some(), p.since.unwrap_or(0)))
}

/// Unknown age is not evidence of staleness — only a stamp we can read and that
/// reads old disqualifies a pane.
fn is_stale_idle(p: &Pane, now: u64) -> bool {
    p.age_secs(now).is_some_and(|age| age > STALE_IDLE_SECS)
}

/// `→ ✋ needs-input  work2:1.0  fix-auth-middleware (blocked 4m)`
///
/// The task slug and the session name reach here from a user prompt and a
/// user-chosen session name, and `display-message` runs both `#{…}` format
/// expansion and strftime over its argument — so those two go through
/// `sanitize_format`. The target alone identifies the pane, so an unlabelled
/// pane simply drops the slug column rather than printing a raw `%id` (whose
/// `%` is exactly what the sanitizer strips).
fn describe(p: &Pane, now: u64) -> String {
    let state = p.state();
    let mut line = format!(
        "→ {} {}  {}",
        state.glyph(),
        state.as_str(),
        t::sanitize_format(&p.target())
    );
    if !p.task.is_empty() {
        line.push_str("  ");
        line.push_str(&t::sanitize_format(&p.task));
    }
    if let Some(secs) = p.age_secs(now) {
        let verb = if state == State::NeedsInput {
            "blocked"
        } else {
            "idle"
        };
        line.push_str(&format!(" ({verb} {})", t::human_age(Some(secs))));
    }
    line
}

// ---------------------------------------------------------------------------
// prefix B — bounce back to the marked origin
// ---------------------------------------------------------------------------

fn back(client: &str, here: &str) {
    // `{marked}` is tmux's own target for the marked pane. With nothing marked
    // it still exits 0 and prints an empty line (verified, tmux 3.5a), so the
    // emptiness — not the exit status — is what says "nowhere to go back to".
    let origin = t::display(Some("{marked}"), "#{pane_id}").unwrap_or_default();
    if origin.is_empty() {
        t::message("return: no origin marked");
        return;
    }

    // Swap the mark to where we are standing *first*, so a second `B` comes
    // back here. Re-marking the origin itself would clear the mark instead.
    if !here.is_empty() && here != origin {
        t::tmux_ok(["select-pane", "-m", "-t", here]);
    }

    if let Err(e) = t::focus_pane(Some(client), &origin) {
        t::message(&t::sanitize_format(&format!("return: {e}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000;

    fn pane(id: &str, state: &str, since: Option<u64>) -> Pane {
        Pane {
            id: id.to_string(),
            session: "s".to_string(),
            window_index: "0".to_string(),
            pane_index: "0".to_string(),
            state_raw: state.to_string(),
            since,
            ..Default::default()
        }
    }

    /// Minutes before `NOW`, as an epoch stamp.
    fn ago(mins: u64) -> Option<u64> {
        Some(NOW - mins * 60)
    }

    fn pick(panes: &[Pane]) -> Option<&Pane> {
        pick_target(panes, NOW, "")
    }

    #[test]
    fn oldest_blocked_wins_over_newer_blocked() {
        let panes = [
            pane("%1", "needs-input", ago(4)),
            pane("%2", "needs-input", ago(40)),
            pane("%3", "needs-input", ago(12)),
        ];
        assert_eq!(pick(&panes).unwrap().id, "%2");
    }

    #[test]
    fn idle_fallback_picks_the_newest() {
        let panes = [
            pane("%1", "idle", ago(30)),
            pane("%2", "idle", ago(2)),
            pane("%3", "idle", ago(11)),
        ];
        assert_eq!(pick(&panes).unwrap().id, "%2");
    }

    #[test]
    fn needs_input_beats_idle_regardless_of_age() {
        // The blocked pane is far older than the idle one and still wins.
        let panes = [
            pane("%1", "idle", ago(1)),
            pane("%2", "needs-input", ago(90)),
        ];
        assert_eq!(pick(&panes).unwrap().id, "%2");
    }

    #[test]
    fn blocked_is_never_aged_out() {
        // Five hours blocked is exactly the case `prefix N` exists for.
        let panes = [pane("%1", "needs-input", ago(300))];
        assert_eq!(pick(&panes).unwrap().id, "%1");
    }

    #[test]
    fn stale_idle_is_not_a_fallback_candidate() {
        let panes = [pane("%1", "idle", ago(150))];
        assert!(pick(&panes).is_none());
    }

    #[test]
    fn a_live_idle_pane_still_wins_past_stale_ones() {
        let panes = [
            pane("%1", "idle", ago(400)),
            pane("%2", "idle", ago(45)),
            pane("%3", "idle", ago(200)),
        ];
        assert_eq!(pick(&panes).unwrap().id, "%2");
    }

    #[test]
    fn origin_pane_is_excluded() {
        let panes = [
            pane("%1", "needs-input", ago(60)),
            pane("%2", "needs-input", ago(5)),
        ];
        assert_eq!(pick_target(&panes, NOW, "%1").unwrap().id, "%2");
        assert!(pick_target(&panes[..1], NOW, "%1").is_none());
    }

    #[test]
    fn empty_input_returns_none() {
        assert!(pick(&[]).is_none());
    }

    #[test]
    fn busy_and_unstamped_panes_are_never_targets() {
        let panes = [
            pane("%1", "busy", ago(90)),
            pane("%2", "", None),
            pane("%3", "nonsense", ago(1)),
        ];
        assert!(pick(&panes).is_none());
    }

    #[test]
    fn panes_without_since_are_selectable_when_alone() {
        let blocked = [pane("%1", "needs-input", None)];
        assert_eq!(pick(&blocked).unwrap().id, "%1");
        // An idle pane with no stamp has an unknown age, not a stale one.
        let idle = [pane("%2", "idle", None)];
        assert_eq!(pick(&idle).unwrap().id, "%2");
    }

    #[test]
    fn panes_without_since_sort_last_within_their_group() {
        let blocked = [
            pane("%1", "needs-input", None),
            pane("%2", "needs-input", ago(3)),
        ];
        assert_eq!(pick(&blocked).unwrap().id, "%2");
        let idle = [pane("%3", "idle", None), pane("%4", "idle", ago(50))];
        assert_eq!(pick(&idle).unwrap().id, "%4");
    }

    #[test]
    fn an_unstamped_blocked_pane_still_outranks_a_fresh_idle_one() {
        let panes = [pane("%1", "idle", ago(1)), pane("%2", "needs-input", None)];
        assert_eq!(pick(&panes).unwrap().id, "%2");
    }

    #[test]
    fn title_heuristic_panes_participate() {
        // A Claude started before the hooks were wired stamps nothing; tmuxlib
        // infers `idle` from the pane title, and it must remain jumpable.
        let p = Pane {
            id: "%9".to_string(),
            title: "✳ fix auth".to_string(),
            ..Default::default()
        };
        assert_eq!(pick(std::slice::from_ref(&p)).unwrap().id, "%9");
    }

    #[test]
    fn describe_names_state_target_task_and_age() {
        let mut p = pane("%1", "needs-input", ago(4));
        p.session = "work2".to_string();
        p.window_index = "1".to_string();
        p.pane_index = "0".to_string();
        p.task = "fix-auth-middleware".to_string();
        assert_eq!(
            describe(&p, NOW),
            "→ ✋ needs-input  work2:1.0  fix-auth-middleware (blocked 4m)"
        );
    }

    #[test]
    fn describe_drops_the_slug_column_and_an_unknown_age() {
        let p = pane("%7", "idle", None);
        assert_eq!(describe(&p, NOW), "→ ● idle  s:0.0");
    }

    #[test]
    fn describe_neutralizes_format_metachars_from_the_task_slug() {
        let mut p = pane("%1", "idle", ago(1));
        p.task = "echo #{pane_pid}".to_string();
        assert!(describe(&p, NOW).contains("echo pane_pid"));
        assert!(!describe(&p, NOW).contains('#'));
    }
}
