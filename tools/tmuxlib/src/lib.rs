//! Shared tmux plumbing for the Claude-in-tmux toolset.
//!
//! Every binary in this workspace talks to tmux through here so the per-pane
//! option contract stays in one place. The options below are the whole
//! cross-tool API surface; see `docs/architecture.md` for who writes what.

use std::ffi::OsStr;
use std::fmt;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};

// ---------------------------------------------------------------------------
// Option names — the cross-binary contract.
// ---------------------------------------------------------------------------

/// Per-pane: `busy` | `idle` | `needs-input`.
pub const OPT_STATE: &str = "@claude_state";
/// Per-pane: epoch seconds of the last state transition.
pub const OPT_SINCE: &str = "@claude_since";
/// Per-pane: short description of the tool call in flight.
pub const OPT_ACTIVITY: &str = "@claude_activity";
/// Per-pane: kebab-case slug of the session's first prompt.
pub const OPT_TASK: &str = "@claude_task";
/// Per-pane: Claude Code session UUID, for `claude --resume`.
pub const OPT_SESSION_ID: &str = "@claude_session_id";
/// Per-pane: human-assigned role name (`talk-fleet role set`).
pub const OPT_ROLE: &str = "@claude_role";

// There is deliberately no per-window rollup option. tmux can compute one live
// inside a format — `#{P:}` loops a window's panes, and `#{W:}` loops a
// session's windows — so the status bar derives it from `@claude_state`
// directly. A stored rollup would cost two tmux round-trips on every tool call
// and could go stale whenever a pane moved between windows.

// ---------------------------------------------------------------------------
// Lifecycle state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Blocked on a permission prompt or otherwise waiting for a human.
    NeedsInput,
    /// Turn finished; the agent is waiting for its next prompt.
    Idle,
    /// Working.
    Busy,
    /// Not a Claude pane, or no beacon has been stamped yet.
    Unknown,
}

impl State {
    pub fn parse(s: &str) -> State {
        match s.trim() {
            "needs-input" | "blocked" | "attn" => State::NeedsInput,
            "idle" => State::Idle,
            "busy" => State::Busy,
            _ => State::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            State::NeedsInput => "needs-input",
            State::Idle => "idle",
            State::Busy => "busy",
            State::Unknown => "",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            State::NeedsInput => "✋",
            State::Idle => "●",
            State::Busy => "◐",
            State::Unknown => "○",
        }
    }

    /// Attention ranking: lower sorts first in every picker in this workspace.
    pub fn priority(self) -> u8 {
        match self {
            State::NeedsInput => 0,
            State::Idle => 1,
            State::Busy => 2,
            State::Unknown => 3,
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Running tmux
// ---------------------------------------------------------------------------

/// Run `tmux <args>` and return trimmed stdout. `Err` carries stderr.
///
/// Setting `CC_TMUX_SOCKET` redirects every call to `tmux -L <name>`. That is
/// the seam that makes these tools testable: a test server can be driven
/// end-to-end without a stray command reaching the session the user is working
/// in. Unset in normal use, so the inherited `$TMUX` picks the server as usual.
pub fn tmux<I, S>(args: I) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = Command::new("tmux");
    if let Some(socket) = std::env::var_os("CC_TMUX_SOCKET") {
        cmd.arg("-L").arg(socket);
    }
    let out = cmd
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("failed to spawn tmux: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Run `tmux <args>`, discarding output. Returns false on failure.
pub fn tmux_ok<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    tmux(args).is_ok()
}

/// Run several tmux commands in one invocation, saving a process spawn each.
///
/// tmux treats a bare `;` argument as a command separator. An argument that is
/// *exactly* `;` would therefore be read as one — which is why
/// [`sanitize_format`] strips semicolons from everything user data can reach.
pub fn tmux_batch(commands: &[Vec<String>]) -> bool {
    let mut args: Vec<String> = Vec::new();
    for (i, cmd) in commands.iter().enumerate() {
        if i > 0 {
            args.push(";".to_string());
        }
        args.extend(cmd.iter().cloned());
    }
    if args.is_empty() {
        return true;
    }
    tmux(args).is_ok()
}

/// True when a tmux server is reachable.
pub fn in_tmux() -> bool {
    std::env::var_os("TMUX").is_some() && tmux(["display-message", "-p", "ok"]).is_ok()
}

/// The pane this process is running in, from `$TMUX_PANE`.
pub fn current_pane() -> Option<String> {
    std::env::var("TMUX_PANE").ok().filter(|s| !s.is_empty())
}

/// Expand a tmux format string in the context of `pane` (None = current client).
pub fn display(pane: Option<&str>, format: &str) -> Result<String, String> {
    let mut args = vec!["display-message".to_string(), "-p".to_string()];
    if let Some(p) = pane {
        args.push("-t".to_string());
        args.push(p.to_string());
    }
    args.push(format.to_string());
    tmux(args)
}

/// True when the pane still exists on the server.
pub fn pane_alive(pane: &str) -> bool {
    match tmux(["list-panes", "-a", "-F", "#{pane_id}"]) {
        Ok(out) => out.lines().any(|l| l.trim() == pane),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Pane / window options
// ---------------------------------------------------------------------------

pub fn set_pane_opt(pane: &str, key: &str, value: &str) -> bool {
    tmux_ok(["set-option", "-p", "-t", pane, key, value])
}

pub fn unset_pane_opt(pane: &str, key: &str) -> bool {
    tmux_ok(["set-option", "-pu", "-t", pane, key])
}

/// Empty string when unset — `-q` keeps tmux quiet about unknown user options.
pub fn get_pane_opt(pane: &str, key: &str) -> String {
    tmux(["show-options", "-pqv", "-t", pane, key]).unwrap_or_default()
}

pub fn set_window_opt(target: &str, key: &str, value: &str) -> bool {
    tmux_ok(["set-option", "-w", "-t", target, key, value])
}

pub fn unset_window_opt(target: &str, key: &str) -> bool {
    tmux_ok(["set-option", "-wu", "-t", target, key])
}

pub fn get_window_opt(target: &str, key: &str) -> String {
    tmux(["show-options", "-wqv", "-t", target, key]).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Pane inventory
// ---------------------------------------------------------------------------

/// Field order of [`PANE_FORMAT`]; keep the two in sync.
pub const PANE_FORMAT: &str = "#{pane_id}\t#{session_name}\t#{window_index}\t#{pane_index}\t#{window_id}\t#{pane_current_command}\t#{pane_current_path}\t#{pane_title}\t#{@claude_state}\t#{@claude_since}\t#{@claude_activity}\t#{@claude_task}\t#{@claude_session_id}\t#{@claude_role}\t#{pane_marked}\t#{pane_pipe}\t#{window_active}\t#{pane_active}";

#[derive(Debug, Clone, Default)]
pub struct Pane {
    pub id: String,
    pub session: String,
    pub window_index: String,
    pub pane_index: String,
    pub window_id: String,
    pub command: String,
    pub path: String,
    pub title: String,
    pub state_raw: String,
    pub since: Option<u64>,
    pub activity: String,
    pub task: String,
    pub session_id: String,
    pub role: String,
    pub marked: bool,
    pub piped: bool,
    pub window_active: bool,
    pub pane_active: bool,
}

impl Pane {
    fn from_line(line: &str) -> Option<Pane> {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 18 || f[0].is_empty() {
            return None;
        }
        Some(Pane {
            id: f[0].to_string(),
            session: f[1].to_string(),
            window_index: f[2].to_string(),
            pane_index: f[3].to_string(),
            window_id: f[4].to_string(),
            command: f[5].to_string(),
            path: f[6].to_string(),
            title: f[7].to_string(),
            state_raw: f[8].to_string(),
            since: f[9].parse().ok(),
            activity: f[10].to_string(),
            task: f[11].to_string(),
            session_id: f[12].to_string(),
            role: f[13].to_string(),
            marked: f[14] == "1",
            piped: f[15] == "1",
            window_active: f[16] == "1",
            pane_active: f[17] == "1",
        })
    }

    /// `session:window.pane`, the human-facing target string.
    pub fn target(&self) -> String {
        format!("{}:{}.{}", self.session, self.window_index, self.pane_index)
    }

    /// Beacon state if a hook stamped one, else inferred from the pane title.
    ///
    /// The title heuristic is the fallback for panes whose Claude started
    /// before the beacon hooks were wired; it mirrors `talk ping`.
    pub fn state(&self) -> State {
        let stamped = State::parse(&self.state_raw);
        if stamped != State::Unknown {
            return stamped;
        }
        title_state(&self.title)
    }

    /// Whether this looks like a Claude Code pane at all.
    pub fn is_claude(&self) -> bool {
        if !self.state_raw.is_empty() || !self.session_id.is_empty() {
            return true;
        }
        title_state(&self.title) != State::Unknown
    }

    pub fn age_secs(&self, now: u64) -> Option<u64> {
        self.since.map(|s| now.saturating_sub(s))
    }
}

/// Claude Code writes its pane title as `✳ <task>` when idle and
/// `<braille spinner> <task>` while working. Verified on this machine
/// 2026-08-06; `talk ping` keys off the same two shapes.
pub fn title_state(title: &str) -> State {
    let t = title.trim_start();
    if t.starts_with('✳') || t == "Claude Code" {
        return State::Idle;
    }
    match t.chars().next() {
        // U+2800..U+28FF braille block = the spinner frames.
        Some(c) if ('\u{2800}'..='\u{28FF}').contains(&c) => State::Busy,
        _ => State::Unknown,
    }
}

/// Every pane on the server.
pub fn list_panes() -> Vec<Pane> {
    match tmux(["list-panes", "-a", "-F", PANE_FORMAT]) {
        Ok(out) => out.lines().filter_map(Pane::from_line).collect(),
        Err(_) => Vec::new(),
    }
}

/// Every pane that looks like a Claude Code session.
pub fn list_claude_panes() -> Vec<Pane> {
    list_panes().into_iter().filter(|p| p.is_claude()).collect()
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

/// Move `client` (or the current one) so that `pane` is on screen and focused.
///
/// Ordering matters: switch the session first, then the window, then the pane —
/// otherwise a cross-session jump lands on the right session's *old* window.
pub fn focus_pane(client: Option<&str>, pane: &str) -> Result<(), String> {
    let session = display(Some(pane), "#{session_name}")?;
    let window = display(Some(pane), "#{window_id}")?;
    match client {
        Some(c) if !c.is_empty() => {
            tmux(["switch-client", "-c", c, "-t", &session])?;
        }
        _ => {
            tmux(["switch-client", "-t", &session])?;
        }
    }
    tmux(["select-window", "-t", &window])?;
    tmux(["select-pane", "-t", pane])?;
    Ok(())
}

/// Flash a message on the status line. Never fails the caller.
pub fn message(text: &str) {
    let _ = tmux(["display-message", text]);
}

// ---------------------------------------------------------------------------
// Text hygiene
// ---------------------------------------------------------------------------

/// Strip the characters that would be re-interpreted if the text is later
/// substituted into a tmux format string, and flatten it to one line.
///
/// Anything stamped into `@claude_activity` / `@claude_task` passes through
/// here: a tool argument like `echo #{pane_pid}` must render literally.
/// Semicolons go too: [`tmux_batch`] separates commands with a bare `;`
/// argument, so a value that is exactly `;` would silently become one.
pub fn sanitize_format(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '#' | '{' | '}' | '%' | ';' | '\n' | '\r' | '\t'))
        .collect::<String>()
        .trim()
        .to_string()
}

/// Truncate on a char boundary, appending `…` when anything was cut.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Lowercase kebab slug of the first `words` words — used for pane task labels.
pub fn slugify(text: &str, words: usize, max_len: usize) -> String {
    let cleaned: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(words)
        .map(|w| w.to_lowercase())
        .collect();
    truncate_hard(&cleaned.join("-"), max_len)
}

fn truncate_hard(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

// ---------------------------------------------------------------------------
// Time & paths
// ---------------------------------------------------------------------------

pub fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `3m` / `2h14m` / `—` — compact age for picker columns.
pub fn human_age(secs: Option<u64>) -> String {
    match secs {
        None => "—".to_string(),
        Some(s) if s < 60 => format!("{s}s"),
        Some(s) if s < 3600 => format!("{}m", s / 60),
        Some(s) => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
    }
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// `~/.tmux` — this repo.
pub fn tmux_dir() -> PathBuf {
    home().join(".tmux")
}

/// Raw pipe-pane recordings (persistent).
pub fn tape_dir() -> PathBuf {
    home().join(".cache/tmux-tape")
}

/// Per-tool-call tickers (ephemeral, cleared at reboot).
pub fn progress_dir() -> PathBuf {
    PathBuf::from("/tmp/claude-progress")
}

/// `talk-fleet` role registry.
pub fn roles_dir() -> PathBuf {
    PathBuf::from("/tmp/claude-talk/roles")
}

/// `talk-fleet` broadcast rounds.
pub fn rounds_dir() -> PathBuf {
    PathBuf::from("/tmp/claude-talk/rounds")
}

/// Layout snapshots.
pub fn state_dir() -> PathBuf {
    tmux_dir().join("state")
}

/// `prefix + e` scrollback captures.
pub fn capture_dir() -> PathBuf {
    PathBuf::from("/tmp/claude-capture")
}

// ---------------------------------------------------------------------------
// Roles
// ---------------------------------------------------------------------------

/// Resolve a `talk-fleet` target: `@role` hits the registry, everything else
/// (`%12`, `main:1.0`) passes through untouched.
///
/// A role bound to a dead pane is unbound and reported as an error rather than
/// silently mis-delivering to a recycled pane id.
pub fn resolve_target(target: &str) -> Result<String, String> {
    let Some(name) = target.strip_prefix('@') else {
        return Ok(target.to_string());
    };
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!("invalid role name: {target}"));
    }
    let path = roles_dir().join(name);
    let id = std::fs::read_to_string(&path)
        .map_err(|_| format!("no such role: {target} (talk-fleet role list)"))?
        .trim()
        .to_string();
    if !pane_alive(&id) {
        let _ = std::fs::remove_file(&path);
        return Err(format!(
            "role {target} was bound to dead pane {id}; unbound"
        ));
    }
    Ok(id)
}

// ---------------------------------------------------------------------------
// Hook payloads
// ---------------------------------------------------------------------------

/// Read a Claude Code hook's JSON payload from stdin.
///
/// Hooks must never block the agent, so a malformed or absent payload yields
/// `Value::Null` instead of an error.
pub fn read_hook_json() -> serde_json::Value {
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return serde_json::Value::Null;
    }
    serde_json::from_str(&buf).unwrap_or(serde_json::Value::Null)
}

/// First non-empty string among `paths` (dotted lookups), else `""`.
pub fn json_first(v: &serde_json::Value, paths: &[&str]) -> String {
    for p in paths {
        let mut cur = v;
        for seg in p.split('.') {
            cur = &cur[seg];
        }
        if let Some(s) = cur.as_str() {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_heuristic_matches_claude_shapes() {
        assert_eq!(title_state("✳ Claude Code"), State::Idle);
        assert_eq!(title_state("✳ fix the auth bug"), State::Idle);
        assert_eq!(title_state("Claude Code"), State::Idle);
        assert_eq!(title_state("⠐ 設計 tmux 上的功能"), State::Busy);
        assert_eq!(title_state("⣿ working"), State::Busy);
        assert_eq!(title_state("bash"), State::Unknown);
        assert_eq!(title_state(""), State::Unknown);
    }

    #[test]
    fn stamped_state_beats_title() {
        let p = Pane {
            state_raw: "needs-input".into(),
            title: "✳ done".into(),
            ..Default::default()
        };
        assert_eq!(p.state(), State::NeedsInput);
    }

    #[test]
    fn sanitize_strips_format_metachars() {
        assert_eq!(sanitize_format("echo #{pane_pid}"), "echo pane_pid");
        assert_eq!(sanitize_format("a\nb\tc"), "abc");
        assert_eq!(sanitize_format("100%"), "100");
    }

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
        assert_eq!(truncate("設計功能測試", 3), "設計…");
    }

    #[test]
    fn slugify_takes_leading_words() {
        assert_eq!(
            slugify("Fix the keyd IME toggle please", 4, 32),
            "fix-the-keyd-ime"
        );
        assert_eq!(slugify("  ", 4, 32), "");
    }

    #[test]
    fn state_priority_orders_attention_first() {
        let mut v = vec![State::Busy, State::Idle, State::NeedsInput, State::Unknown];
        v.sort_by_key(|s| s.priority());
        assert_eq!(
            v,
            vec![State::NeedsInput, State::Idle, State::Busy, State::Unknown]
        );
    }

    #[test]
    fn resolve_target_passes_through_non_roles() {
        assert_eq!(resolve_target("%12").unwrap(), "%12");
        assert_eq!(resolve_target("main:1.0").unwrap(), "main:1.0");
        assert!(resolve_target("@../etc/passwd").is_err());
    }

    #[test]
    fn human_age_formats() {
        assert_eq!(human_age(None), "—");
        assert_eq!(human_age(Some(30)), "30s");
        assert_eq!(human_age(Some(120)), "2m");
        assert_eq!(human_age(Some(7500)), "2h05m");
    }

    #[test]
    fn json_first_walks_dotted_paths() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"tool_input":{"command":"go test"},"tool_name":"Bash"}"#)
                .unwrap();
        assert_eq!(
            json_first(&v, &["tool_input.file_path", "tool_input.command"]),
            "go test"
        );
        assert_eq!(json_first(&v, &["nope.nope"]), "");
    }
}
