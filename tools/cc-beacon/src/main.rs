//! `cc-beacon <event>` / `cc-beacon agy <event>` — mirrors a Claude Code or
//! Antigravity CLI (`agy`) session's lifecycle into tmux pane options so every
//! other tool in this workspace can see it.
//!
//! Wired into seven Claude Code hook events in `~/.claude/settings.json` and
//! five `agy` hook events in `~/.gemini/config/hooks.json`; see
//! [ARCHITECTURE.org](../ARCHITECTURE.org) for the event table. This is the only
//! writer of `@claude_state` and friends.
//!
//! Two invariants, both load-bearing:
//!   * every path exits 0 (and `agy` mode always prints valid JSON on stdout) —
//!     a hook that fails must never block the agent;
//!   * pane *titles* are never touched — `talk ping` owns them.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::Command;

use tmuxlib as t;

fn main() {
    // Anything that goes wrong here is not the agent's problem.
    run();
    std::process::exit(0);
}

fn run() {
    let event = match std::env::args().nth(1) {
        Some(e) => e,
        None => return,
    };

    if event == "agy" {
        let sub = std::env::args().nth(2).unwrap_or_default();
        run_agy(&sub);
        return;
    }

    let Some(pane) = t::current_pane() else {
        return;
    };

    // No liveness pre-check: this runs on every tool call, and a round-trip to
    // ask tmux whether the pane still exists costs more than letting the writes
    // below fail. Commands against a dead pane are rejected harmlessly and
    // every result here is discarded anyway.
    let payload = t::read_hook_json();

    match event.as_str() {
        "session-start" => session_start(&pane, &payload),
        "prompt" => prompt(&pane, &payload),
        "activity" => activity(&pane, &payload),
        "tool-done" => tool_done(&pane, &payload),
        "attn" => attn(&pane),
        "idle" => idle(&pane),
        "clear" => clear(&pane),
        _ => {}
    }
}

/// Handle Antigravity CLI (`agy`) lifecycle hooks.
///
/// `agy` requires every hook command to print a JSON object on stdout, even
/// when running outside tmux. We therefore always emit the contract response
/// before returning.
fn run_agy(event: &str) {
    let stdout_json = agy_stdout_response(event);
    let payload = t::read_hook_json();

    if let Some(pane) = t::current_pane() {
        match event {
            "session-start" => agy_session_start(&pane, &payload),
            "pre-invocation" => agy_pre_invocation(&pane, &payload),
            "ask-question" => agy_ask_question(&pane),
            "post-tool" => agy_post_tool(&pane, &payload),
            "stop" => agy_stop(&pane, &payload),
            _ => {}
        }
    }

    println!("{stdout_json}");
}

fn agy_stdout_response(event: &str) -> &'static str {
    match event {
        "ask-question" | "stop" => r#"{"decision":"allow"}"#,
        _ => "{}",
    }
}

// ---------------------------------------------------------------------------
// Event handlers
// ---------------------------------------------------------------------------

fn session_start(pane: &str, payload: &serde_json::Value) {
    t::set_pane_opt(pane, t::OPT_AGENT, "claude");
    let sid = t::json_first(payload, &["session_id"]);
    if !sid.is_empty() {
        t::set_pane_opt(pane, t::OPT_SESSION_ID, &sid);
    }

    // Name the window after the project so the status bar reads as a roster
    // instead of five windows all called `claude`.
    let cwd = {
        let from_payload = t::json_first(payload, &["cwd"]);
        if from_payload.is_empty() {
            std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        } else {
            from_payload
        }
    };
    if let Some(base) = cwd.rsplit('/').find(|s| !s.is_empty()) {
        let name = t::sanitize_format(&format!("cc:{base}"));
        if !name.is_empty() {
            t::tmux_ok(["rename-window", "-t", pane, &name]);
            t::set_window_opt(pane, "automatic-rename", "off");
        }
    }

    stamp(pane, t::State::Idle);
}

fn prompt(pane: &str, payload: &serde_json::Value) {
    // Label the pane with the session's first prompt, once. Relayed peer
    // traffic is not what this session is *about*, so it never becomes a label.
    if t::get_pane_opt(pane, t::OPT_TASK).is_empty() {
        let text = t::json_first(payload, &["prompt"]);
        if !is_peer_traffic(&text) {
            let slug = t::slugify(&text, 4, 32);
            if !slug.is_empty() {
                t::set_pane_opt(pane, t::OPT_TASK, &t::sanitize_format(&slug));
            }
        }
    }
    stamp(pane, t::State::Busy);
}

fn activity(pane: &str, payload: &serde_json::Value) {
    // The hot path — this fires before every single tool call, so the activity
    // string and the state stamp go out in one tmux invocation.
    let summary = tool_summary(payload, 48);
    stamp_with(pane, t::State::Busy, Some(&summary));
}

fn tool_done(pane: &str, payload: &serde_json::Value) {
    // The semantic tape: one greppable line per tool call, immune to the
    // alt-screen redraws that destroy tmux scrollback.
    let line = tool_summary(payload, 90);
    if !line.is_empty() {
        append_ticker(pane, &format!("{} {}", clock(), line));
    }
}

fn attn(pane: &str) {
    stamp(pane, t::State::NeedsInput);
    if !is_on_screen(pane) {
        ring_bell(pane);
    }
}

fn idle(pane: &str) {
    stamp_with(pane, t::State::Idle, Some(""));
    append_ticker(pane, &format!("{} ── idle ──", clock()));
}

fn clear(pane: &str) {
    let mut commands: Vec<Vec<String>> = [
        t::OPT_STATE,
        t::OPT_SINCE,
        t::OPT_ACTIVITY,
        t::OPT_TASK,
        t::OPT_SESSION_ID,
        t::OPT_AGENT,
    ]
    .iter()
    .map(|opt| {
        vec![
            "set-option".to_string(),
            "-pu".to_string(),
            "-t".to_string(),
            pane.to_string(),
            (*opt).to_string(),
        ]
    })
    .collect();
    commands.push(vec!["refresh-client".to_string(), "-S".to_string()]);
    t::tmux_batch(&commands);
}

// ---------------------------------------------------------------------------
// Antigravity CLI (`agy`) event handlers
// ---------------------------------------------------------------------------

fn agy_session_start(pane: &str, payload: &serde_json::Value) {
    let mut commands = vec![
        set_pane(pane, t::OPT_AGENT, "agy"),
        set_pane(pane, t::OPT_STATE, t::State::Idle.as_str()),
        set_pane(pane, t::OPT_SINCE, &t::now_epoch().to_string()),
        set_pane(pane, t::OPT_ACTIVITY, ""),
    ];
    let sid = t::json_first(payload, &["conversationId"]);
    if !sid.is_empty() {
        commands.push(set_pane(pane, t::OPT_SESSION_ID, &t::sanitize_format(&sid)));
    }
    commands.push(vec!["refresh-client".to_string(), "-S".to_string()]);
    t::tmux_batch(&commands);
}

fn agy_pre_invocation(pane: &str, payload: &serde_json::Value) {
    let mut commands = vec![
        set_pane(pane, t::OPT_AGENT, "agy"),
        set_pane(pane, t::OPT_STATE, t::State::Busy.as_str()),
        set_pane(pane, t::OPT_SINCE, &t::now_epoch().to_string()),
    ];
    if t::get_pane_opt(pane, t::OPT_TASK).is_empty() {
        let text = t::json_first(payload, &["lastUserInput"]);
        if !is_peer_traffic(&text) {
            let slug = t::slugify(&text, 4, 32);
            if !slug.is_empty() {
                commands.push(set_pane(pane, t::OPT_TASK, &t::sanitize_format(&slug)));
            }
        }
    }
    commands.push(vec!["refresh-client".to_string(), "-S".to_string()]);
    t::tmux_batch(&commands);
}

fn agy_ask_question(pane: &str) {
    let commands = vec![
        set_pane(pane, t::OPT_AGENT, "agy"),
        set_pane(pane, t::OPT_STATE, t::State::NeedsInput.as_str()),
        set_pane(pane, t::OPT_SINCE, &t::now_epoch().to_string()),
        set_pane(pane, t::OPT_ACTIVITY, "ask_question"),
        vec!["refresh-client".to_string(), "-S".to_string()],
    ];
    t::tmux_batch(&commands);
    if !is_on_screen(pane) {
        ring_bell(pane);
    }
}

fn agy_post_tool(pane: &str, payload: &serde_json::Value) {
    let short = agy_tool_summary(payload, 48);
    let commands = vec![
        set_pane(pane, t::OPT_AGENT, "agy"),
        set_pane(pane, t::OPT_STATE, t::State::Busy.as_str()),
        set_pane(pane, t::OPT_SINCE, &t::now_epoch().to_string()),
        set_pane(pane, t::OPT_ACTIVITY, &short),
        vec!["refresh-client".to_string(), "-S".to_string()],
    ];
    t::tmux_batch(&commands);

    let line = agy_tool_summary(payload, 90);
    if !line.is_empty() {
        append_ticker(pane, &format!("{} {}", clock(), line));
    }
}

fn agy_stop(pane: &str, payload: &serde_json::Value) {
    if agy_stop_is_error(payload) {
        let commands = vec![
            set_pane(pane, t::OPT_AGENT, "agy"),
            set_pane(pane, t::OPT_STATE, t::State::NeedsInput.as_str()),
            set_pane(pane, t::OPT_SINCE, &t::now_epoch().to_string()),
            set_pane(pane, t::OPT_ACTIVITY, "error"),
            vec!["refresh-client".to_string(), "-S".to_string()],
        ];
        t::tmux_batch(&commands);
        append_ticker(pane, &format!("{} ── error ──", clock()));
        if !is_on_screen(pane) {
            ring_bell(pane);
        }
        return;
    }

    let commands = vec![
        set_pane(pane, t::OPT_AGENT, "agy"),
        set_pane(pane, t::OPT_STATE, t::State::Idle.as_str()),
        set_pane(pane, t::OPT_SINCE, &t::now_epoch().to_string()),
        set_pane(pane, t::OPT_ACTIVITY, ""),
        vec!["refresh-client".to_string(), "-S".to_string()],
    ];
    t::tmux_batch(&commands);
    append_ticker(pane, &format!("{} ── idle ──", clock()));
}

fn agy_stop_is_error(payload: &serde_json::Value) -> bool {
    let reason = t::json_first(payload, &["terminationReason"]);
    if reason == "ERROR" {
        return true;
    }
    !t::json_first(payload, &["error"]).is_empty()
}

/// `run_command npm test` — `agy` camelCase toolCall name plus its key arg.
fn agy_tool_summary(payload: &serde_json::Value, max: usize) -> String {
    let tool = t::json_first(payload, &["toolCall.name"]);
    if tool.is_empty() {
        return String::new();
    }
    let arg = t::json_first(
        payload,
        &[
            "toolCall.args.CommandLine",
            "toolCall.args.Path",
            "toolCall.args.Query",
            "toolCall.args.query",
            "toolCall.args.Description",
            "toolCall.args.Prompt",
            "toolCall.args.toolSummary",
        ],
    );
    let joined = if arg.is_empty() {
        tool
    } else {
        format!("{tool} {arg}")
    };
    t::truncate(&t::sanitize_format(&joined), max)
}

// ---------------------------------------------------------------------------
// State plumbing
// ---------------------------------------------------------------------------

fn stamp(pane: &str, state: t::State) {
    stamp_with(pane, state, None);
}

/// Write the state transition, optionally the activity string, and repaint —
/// all in a single tmux invocation.
///
/// `refresh-client -S` is what makes the bar update now instead of at the next
/// `status-interval` tick. There is no window-level rollup to maintain: the
/// status formats derive it from these per-pane options with `#{P:}`.
fn stamp_with(pane: &str, state: t::State, set_activity: Option<&str>) {
    let mut commands = vec![
        set_pane(pane, t::OPT_STATE, state.as_str()),
        set_pane(pane, t::OPT_SINCE, &t::now_epoch().to_string()),
    ];
    if let Some(activity) = set_activity {
        commands.push(set_pane(pane, t::OPT_ACTIVITY, activity));
    }
    commands.push(vec!["refresh-client".to_string(), "-S".to_string()]);
    t::tmux_batch(&commands);
}

fn set_pane(pane: &str, key: &str, value: &str) -> Vec<String> {
    vec![
        "set-option".into(),
        "-p".into(),
        "-t".into(),
        pane.into(),
        key.into(),
        value.into(),
    ]
}

// ---------------------------------------------------------------------------
// Bell
// ---------------------------------------------------------------------------

/// True when the human is currently looking at this pane.
fn is_on_screen(pane: &str) -> bool {
    let visible =
        t::display(Some(pane), "#{&&:#{window_active},#{pane_active}}").unwrap_or_default();
    let attached = t::display(Some(pane), "#{session_attached}").unwrap_or_default();
    visible == "1" && attached != "0"
}

/// Write BEL to the pane's tty. tmux reads it off the pty master and raises the
/// window's `!` flag — the native "this window wants you" signal.
fn ring_bell(pane: &str) {
    let Ok(tty) = t::display(Some(pane), "#{pane_tty}") else {
        return;
    };
    if tty.is_empty() {
        return;
    }
    if let Ok(mut f) = OpenOptions::new().write(true).open(&tty) {
        let _ = f.write_all(b"\x07");
    }
}

// ---------------------------------------------------------------------------
// Payload helpers
// ---------------------------------------------------------------------------

/// `Bash go test ./...` — tool name plus its most identifying argument.
fn tool_summary(payload: &serde_json::Value, max: usize) -> String {
    let tool = t::json_first(payload, &["tool_name"]);
    if tool.is_empty() {
        return String::new();
    }
    let arg = t::json_first(
        payload,
        &[
            "tool_input.command",
            "tool_input.file_path",
            "tool_input.pattern",
            "tool_input.url",
            "tool_input.skill",
            "tool_input.description",
            "tool_input.prompt",
        ],
    );
    let joined = if arg.is_empty() {
        tool
    } else {
        format!("{tool} {arg}")
    };
    t::truncate(&t::sanitize_format(&joined), max)
}

/// Messages relayed from a sibling pane arrive with these banners; they are
/// this session's *input*, not its subject.
fn is_peer_traffic(text: &str) -> bool {
    text.lines()
        .take(3)
        .any(|l| l.starts_with("### [talk]") || l.starts_with("-----mq"))
}

// ---------------------------------------------------------------------------
// Ticker
// ---------------------------------------------------------------------------

fn append_ticker(pane: &str, line: &str) {
    let dir = t::progress_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join(format!("{pane}.log"));
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

/// Local `HH:MM`. Shelling out to `date` keeps the local timezone correct
/// without pulling in a time crate; it costs one fork per completed tool call,
/// which is noise next to the tool round-trip itself.
fn clock() -> String {
    Command::new("date")
        .arg("+%H:%M")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "--:--".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn tool_summary_prefers_command() {
        let v =
            json(r#"{"tool_name":"Bash","tool_input":{"command":"go test ./...","timeout":5}}"#);
        assert_eq!(tool_summary(&v, 90), "Bash go test ./...");
    }

    #[test]
    fn tool_summary_falls_back_through_arg_shapes() {
        let v = json(r#"{"tool_name":"Read","tool_input":{"file_path":"/etc/hosts"}}"#);
        assert_eq!(tool_summary(&v, 90), "Read /etc/hosts");
        let v = json(r#"{"tool_name":"Task","tool_input":{}}"#);
        assert_eq!(tool_summary(&v, 90), "Task");
    }

    #[test]
    fn tool_summary_neutralizes_format_metachars() {
        let v = json(r#"{"tool_name":"Bash","tool_input":{"command":"echo #{pane_pid}"}}"#);
        assert_eq!(tool_summary(&v, 90), "Bash echo pane_pid");
    }

    #[test]
    fn tool_summary_is_empty_without_a_tool() {
        assert_eq!(tool_summary(&serde_json::Value::Null, 90), "");
    }

    #[test]
    fn peer_banners_are_not_task_labels() {
        assert!(is_peer_traffic("### [talk] PEER-AGENT msg from %3\nhello"));
        assert!(is_peer_traffic("-----mq  topic=review  file=/tmp/x"));
        assert!(!is_peer_traffic("fix the keyd IME toggle"));
        // A banner further down the prompt is quoted text, not a relay.
        assert!(!is_peer_traffic("look at this:\n\n\n\n### [talk] quoted"));
    }

    #[test]
    fn agy_tool_summary_parses_camelcase_payload() {
        let v = json(
            r#"{"toolCall":{"id":"t1","name":"run_command","args":{"CommandLine":"cargo test"}}}"#,
        );
        assert_eq!(agy_tool_summary(&v, 90), "run_command cargo test");

        let v = json(r#"{"toolCall":{"name":"read_file","args":{"Path":"/tmp/foo.rs"}}}"#);
        assert_eq!(agy_tool_summary(&v, 90), "read_file /tmp/foo.rs");

        let v = json(r#"{"toolCall":{"name":"ask_question","args":{}}}"#);
        assert_eq!(agy_tool_summary(&v, 90), "ask_question");
    }

    #[test]
    fn agy_stop_error_detection_and_stdout_contract() {
        assert!(agy_stop_is_error(&json(r#"{"terminationReason":"ERROR"}"#)));
        assert!(agy_stop_is_error(&json(r#"{"error":"boom"}"#)));
        assert!(!agy_stop_is_error(&json(
            r#"{"terminationReason":"NO_TOOL_CALL","error":""}"#
        )));

        assert_eq!(agy_stdout_response("session-start"), "{}");
        assert_eq!(agy_stdout_response("pre-invocation"), "{}");
        assert_eq!(agy_stdout_response("post-tool"), "{}");
        assert_eq!(
            agy_stdout_response("ask-question"),
            r#"{"decision":"allow"}"#
        );
        assert_eq!(agy_stdout_response("stop"), r#"{"decision":"allow"}"#);
    }
}
