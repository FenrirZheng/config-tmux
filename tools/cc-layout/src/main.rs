//! `cc-layout snapshot|restore|list` — save a whole agent team to disk and
//! rebuild it after a reboot.
//!
//! `snapshot` (bound to `prefix M-s`) serializes every session/window/pane to a
//! TSV under [`state_dir`](tmuxlib::state_dir); `restore` replays that file into
//! a live server. The two primitives it stands on are both already proven here:
//!
//!   * `#{window_layout}` round-trips exactly through `select-layout`;
//!   * `claude --resume <full-uuid>` restores a conversation with full memory.
//!
//! What makes geometry come back *exactly* is applying each window's layout
//! **once**, after every pane of that window exists — a `select-layout` after
//! every split is undone by the split that follows it.
//!
//! Restore never touches an existing session: a name collision restores into
//! `<name>-restored` alongside it. Pane options are `cc-beacon`'s to write, so
//! only `@claude_task` is re-applied here, and pane *titles* are never written
//! at all — `talk ping` owns them (see [ARCHITECTURE.org](../ARCHITECTURE.org)).

use std::fmt;
use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use tmuxlib as t;

/// Bumped only on an incompatible field change; an unknown version is a hard
/// parse error rather than a best-effort read of fields that have moved.
const FORMAT_VERSION: &str = "1";

/// The snapshot every keystroke overwrites; `restore` reads it by default.
const LAST: &str = "last-layout.tsv";

/// Used when no client is attached — a detached `new-session` otherwise
/// defaults to 80x24 and squashes every restored layout into it.
const DEFAULT_SIZE: (u32, u32) = (200, 50);

/// `#{window_id}` is captured for targeting only; it is never serialized,
/// because ids are not stable across a server restart.
const WINDOW_FORMAT: &str = "#{window_id}\t#{window_index}\t#{window_name}\t#{window_layout}";
const PANE_FORMAT: &str =
    "#{pane_index}\t#{pane_current_path}\t#{@claude_session_id}\t#{@claude_task}";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("snapshot") => cmd_snapshot(&args[1..]),
        Some("restore") => cmd_restore(&args[1..]),
        Some("list") => cmd_list(),
        Some("-h") | Some("--help") | Some("help") => {
            usage();
            0
        }
        Some(other) => {
            eprintln!("cc-layout: unknown subcommand {other:?}");
            usage();
            2
        }
        None => {
            usage();
            2
        }
    };
    std::process::exit(code);
}

fn usage() {
    eprintln!(
        "cc-layout — snapshot and resurrect a tmux agent team

  cc-layout snapshot [--out <path>]      write the live server to TSV
  cc-layout restore [<path>] [--dry-run] rebuild sessions/windows/panes from TSV
  cc-layout list                         show saved snapshots

Default snapshot: {}",
        t::state_dir().join(LAST).display()
    );
}

// ---------------------------------------------------------------------------
// Model — the whole file format, as data
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Snapshot {
    sessions: Vec<Session>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Session {
    name: String,
    windows: Vec<Window>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Window {
    index: String,
    name: String,
    /// The verbatim `#{window_layout}` string, checksum included.
    layout: String,
    panes: Vec<PaneRec>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PaneRec {
    index: String,
    cwd: String,
    session_id: String,
    task: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Counts {
    sessions: usize,
    windows: usize,
    panes: usize,
    claude: usize,
}

impl Snapshot {
    fn counts(&self) -> Counts {
        let mut c = Counts {
            sessions: self.sessions.len(),
            ..Default::default()
        };
        for s in &self.sessions {
            c.windows += s.windows.len();
            for w in &s.windows {
                c.panes += w.panes.len();
                c.claude += w.panes.iter().filter(|p| !p.session_id.is_empty()).count();
            }
        }
        c
    }
}

impl fmt::Display for Counts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} sessions, {} windows, {} panes, {} claude",
            self.sessions, self.windows, self.panes, self.claude
        )
    }
}

// ---------------------------------------------------------------------------
// Serialization — pure, no tmux
// ---------------------------------------------------------------------------
//
// One tagged record per line, fields separated by a literal tab:
//
//   V <version>
//   S <session-name>
//   W <index> <name> <layout>
//   P <index> <cwd> <session-id> <task>
//
// Every field is backslash-escaped (`\\ \t \n \r`) on the way out and unescaped
// on the way in, so no field content can produce a stray separator. tmux never
// puts a tab inside a `#{window_layout}` string — the escaping is defensive,
// and it is what lets window names, cwds and task slugs (all ultimately
// human-supplied) be stored verbatim instead of mangled.

fn serialize(snap: &Snapshot) -> String {
    let mut out = row(&["V", FORMAT_VERSION]);
    for s in &snap.sessions {
        out.push_str(&row(&["S", &s.name]));
        for w in &s.windows {
            out.push_str(&row(&["W", &w.index, &w.name, &w.layout]));
            for p in &w.panes {
                out.push_str(&row(&["P", &p.index, &p.cwd, &p.session_id, &p.task]));
            }
        }
    }
    out
}

fn row(fields: &[&str]) -> String {
    let mut line = String::new();
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            line.push('\t');
        }
        line.push_str(&escape(f));
    }
    line.push('\n');
    line
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Parsing — pure, no tmux
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParseError {
    line: usize,
    msg: String,
}

impl ParseError {
    fn at(line: usize, msg: impl Into<String>) -> ParseError {
        ParseError {
            line,
            msg: msg.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}

fn parse(text: &str) -> Result<Snapshot, ParseError> {
    let mut snap = Snapshot::default();

    for (i, raw) in text.lines().enumerate() {
        let no = i + 1;
        if raw.is_empty() {
            continue;
        }
        let f: Vec<&str> = raw.split('\t').collect();
        match f[0] {
            "V" => {
                let v = field(&f, 2, 1, no)?;
                if v != FORMAT_VERSION {
                    return Err(ParseError::at(
                        no,
                        format!(
                            "unsupported format version {v:?} (this build reads {FORMAT_VERSION})"
                        ),
                    ));
                }
            }
            "S" => snap.sessions.push(Session {
                name: field(&f, 2, 1, no)?,
                windows: Vec::new(),
            }),
            "W" => {
                let w = Window {
                    index: field(&f, 4, 1, no)?,
                    name: field(&f, 4, 2, no)?,
                    layout: field(&f, 4, 3, no)?,
                    panes: Vec::new(),
                };
                snap.sessions
                    .last_mut()
                    .ok_or_else(|| ParseError::at(no, "W record before any S record"))?
                    .windows
                    .push(w);
            }
            "P" => {
                let p = PaneRec {
                    index: field(&f, 5, 1, no)?,
                    cwd: field(&f, 5, 2, no)?,
                    session_id: field(&f, 5, 3, no)?,
                    task: field(&f, 5, 4, no)?,
                };
                snap.sessions
                    .last_mut()
                    .and_then(|s| s.windows.last_mut())
                    .ok_or_else(|| ParseError::at(no, "P record before any W record"))?
                    .panes
                    .push(p);
            }
            other => return Err(ParseError::at(no, format!("unknown record tag {other:?}"))),
        }
    }

    Ok(snap)
}

/// Field `idx` of a record that must have exactly `want` fields.
///
/// Exact, not minimum: a short line is a truncated write and a long one is a
/// field that grew, and silently mis-parsing either would restore a pane into
/// the wrong directory or type the wrong UUID.
fn field(f: &[&str], want: usize, idx: usize, no: usize) -> Result<String, ParseError> {
    if f.len() != want {
        return Err(ParseError::at(
            no,
            format!("{:?} record wants {want} fields, got {}", f[0], f.len()),
        ));
    }
    unescape(f[idx], no)
}

fn unescape(s: &str, no: usize) -> Result<String, ParseError> {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => return Err(ParseError::at(no, format!("unknown escape \\{other}"))),
            None => return Err(ParseError::at(no, "field ends in a lone backslash")),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// snapshot
// ---------------------------------------------------------------------------

fn cmd_snapshot(args: &[String]) -> i32 {
    let mut out: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" | "-o" => match it.next() {
                Some(p) => out = Some(PathBuf::from(p)),
                None => {
                    eprintln!("cc-layout snapshot: --out needs a path");
                    return 2;
                }
            },
            other => {
                eprintln!("cc-layout snapshot: unexpected argument {other:?}");
                return 2;
            }
        }
    }

    let snap = match capture() {
        Ok(s) => s,
        // No server, no state: nothing to do is not a failure.
        Err(e) => {
            report(&format!("snapshot: no tmux server ({e})"));
            return 0;
        }
    };
    // Never overwrite a good snapshot with an empty one.
    if snap.sessions.is_empty() {
        report("snapshot: no sessions to record");
        return 0;
    }

    let text = serialize(&snap);
    let dir = t::state_dir();
    let primary = out.clone().unwrap_or_else(|| dir.join(LAST));

    if let Err(e) = write_atomic(&primary, &text) {
        eprintln!(
            "cc-layout snapshot: cannot write {}: {e}",
            primary.display()
        );
        return 1;
    }
    // An explicit `--out` is an explicit destination; the rolling history is
    // only kept for the default path that `prefix M-s` overwrites every time.
    if out.is_none() {
        let stamped = dir.join(format!("layout-{}.tsv", stamp()));
        if let Err(e) = write_atomic(&stamped, &text) {
            eprintln!(
                "cc-layout snapshot: cannot write {}: {e}",
                stamped.display()
            );
        }
    }

    let c = snap.counts();
    report(&format!(
        "snapshot: {} windows, {} claude panes → {}",
        c.windows,
        c.claude,
        primary.display()
    ));
    0
}

/// Read the live server into the model. `Err` means no server to talk to.
fn capture() -> Result<Snapshot, String> {
    let sessions = t::tmux(["list-sessions", "-F", "#{session_name}"])?;
    let mut snap = Snapshot::default();

    for name in sessions.lines().filter(|l| !l.is_empty()) {
        let mut session = Session {
            name: name.to_string(),
            windows: Vec::new(),
        };
        // `=` forces an exact-name match, so a session called `main` can never
        // resolve to `maintenance`.
        let target = format!("={name}");
        let windows =
            t::tmux(["list-windows", "-t", &target, "-F", WINDOW_FORMAT]).unwrap_or_default();

        for line in windows.lines() {
            let Some(f) = split_fields(line, 4) else {
                continue;
            };
            let mut window = Window {
                index: f[1].clone(),
                name: f[2].clone(),
                layout: f[3].clone(),
                panes: Vec::new(),
            };
            let panes = t::tmux(["list-panes", "-t", &f[0], "-F", PANE_FORMAT]).unwrap_or_default();
            for pl in panes.lines() {
                let Some(p) = split_fields(pl, 4) else {
                    continue;
                };
                window.panes.push(PaneRec {
                    index: p[0].clone(),
                    cwd: p[1].clone(),
                    session_id: p[2].clone(),
                    task: p[3].clone(),
                });
            }
            session.windows.push(window);
        }
        snap.sessions.push(session);
    }

    Ok(snap)
}

/// Exactly `n` tab-separated fields, or `None`. tmux emits one tab per
/// separator in its own format output, so a different count means the line is
/// not the record we asked for and is not worth guessing at.
fn split_fields(line: &str, n: usize) -> Option<Vec<String>> {
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() != n {
        return None;
    }
    Some(f.iter().map(|s| s.to_string()).collect())
}

/// Temp file + rename, so an interrupted snapshot leaves the previous one
/// intact instead of a truncated file restore would half-apply. The `sync_all`
/// earns its cost here specifically: this file exists to survive the kind of
/// unclean shutdown that also loses unflushed page cache.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.tmp"));
    {
        let mut f = File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// Local `YYYYmmdd-HHMM`. Shelling out to `date` keeps the local timezone
/// correct without pulling in a time crate — the same trade `cc-beacon` makes.
fn stamp() -> String {
    date(&["+%Y%m%d-%H%M"]).unwrap_or_else(|| "unknown".to_string())
}

fn date(args: &[&str]) -> Option<String> {
    Command::new("date")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Status line when there is a server to flash it on, stdout either way.
fn report(line: &str) {
    println!("{line}");
    t::message(&t::sanitize_format(line));
}

// ---------------------------------------------------------------------------
// restore
// ---------------------------------------------------------------------------

fn cmd_restore(args: &[String]) -> i32 {
    let mut path: Option<PathBuf> = None;
    let mut dry = false;
    for a in args {
        match a.as_str() {
            "--dry-run" | "-n" => dry = true,
            other if other.starts_with('-') => {
                eprintln!("cc-layout restore: unknown flag {other:?}");
                return 2;
            }
            other => path = Some(PathBuf::from(other)),
        }
    }
    let path = path.unwrap_or_else(|| t::state_dir().join(LAST));

    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "restore: no snapshot at {} — nothing to restore",
                path.display()
            );
            return 0;
        }
        // A file that exists but cannot be read is worth surfacing.
        Err(e) => {
            eprintln!("restore: cannot read {}: {e}", path.display());
            return 2;
        }
    };
    let snap = match parse(&text) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("restore: corrupt snapshot {} — {e}", path.display());
            return 2;
        }
    };
    if snap.sessions.is_empty() {
        println!(
            "restore: {} holds no sessions — nothing to restore",
            path.display()
        );
        return 0;
    }

    let (width, height) = client_size();
    let mut r = Restorer {
        dry,
        width,
        height,
        projects: t::home().join(".claude/projects"),
        claimed: Vec::new(),
        counts: Counts::default(),
        resumed: 0,
        skips: Vec::new(),
        attach: Vec::new(),
    };

    println!(
        "restore: {} ({}) into {width}x{height}{}",
        path.display(),
        snap.counts(),
        if dry {
            "  [dry-run — nothing is created]"
        } else {
            ""
        }
    );
    for s in &snap.sessions {
        r.session(s);
    }
    r.summary();
    0
}

/// Terminal size to build detached sessions at. Layout strings hold absolute
/// cell counts; tmux rescales them proportionally on a mismatch, so the closer
/// this is to the real client, the less rounding drift the geometry picks up.
fn client_size() -> (u32, u32) {
    let out =
        t::tmux(["list-clients", "-F", "#{client_width}\t#{client_height}"]).unwrap_or_default();
    for line in out.lines() {
        let mut f = line.split('\t');
        let w: Option<u32> = f.next().and_then(|s| s.parse().ok());
        let h: Option<u32> = f.next().and_then(|s| s.parse().ok());
        if let (Some(w), Some(h)) = (w, h) {
            if w > 0 && h > 0 {
                return (w, h);
            }
        }
    }
    DEFAULT_SIZE
}

struct Restorer {
    dry: bool,
    width: u32,
    height: u32,
    projects: PathBuf,
    /// Session names handed out during this run. `has-session` alone is not
    /// enough: a dry run creates nothing, so two snapshot sessions with the
    /// same name would otherwise both report the same restored name.
    claimed: Vec<String>,
    counts: Counts,
    resumed: usize,
    skips: Vec<String>,
    attach: Vec<String>,
}

impl Restorer {
    // -- sessions ----------------------------------------------------------

    fn session(&mut self, s: &Session) {
        let name = self.unique_name(&s.name);
        self.claimed.push(name.clone());
        let note = if name == s.name {
            String::new()
        } else {
            format!("  (from {:?} — that name is in use)", s.name)
        };
        println!("session {name}{note}");
        self.counts.sessions += 1;
        self.attach.push(name.clone());

        let mut first = true;
        for w in &s.windows {
            let win = if first {
                self.create_session(&name, w)
            } else {
                self.new_window(&name, w)
            };
            first = false;
            match win {
                Some(id) => self.window(&name, &id, w),
                // A dry run has no ids to work with and still walks the window.
                None if self.dry => self.window(&name, "", w),
                None => {}
            }
        }
    }

    /// Never collide with live work: an existing name restores alongside it,
    /// never into it.
    fn unique_name(&self, desired: &str) -> String {
        let taken = |n: &str| self.claimed.iter().any(|c| c == n) || session_exists(n);
        if !taken(desired) {
            return desired.to_string();
        }
        let base = format!("{desired}-restored");
        if !taken(&base) {
            return base;
        }
        let mut n = 2;
        loop {
            let candidate = format!("{base}-{n}");
            if !taken(&candidate) {
                return candidate;
            }
            n += 1;
        }
    }

    fn create_session(&mut self, name: &str, w: &Window) -> Option<String> {
        let mut args = vec![
            "new-session".to_string(),
            "-d".to_string(),
            "-P".to_string(),
            "-F".to_string(),
            "#{window_id}".to_string(),
            "-s".to_string(),
            name.to_string(),
            "-x".to_string(),
            self.width.to_string(),
            "-y".to_string(),
            self.height.to_string(),
        ];
        if !w.name.is_empty() {
            args.push("-n".to_string());
            args.push(w.name.clone());
        }
        // The window's first pane is born in the right cwd; only the *later*
        // panes need `split-window -c`.
        if let Some(cwd) = first_cwd(w) {
            args.push("-c".to_string());
            args.push(cwd);
        }
        if self.dry {
            return None;
        }
        match t::tmux(&args) {
            Ok(id) => {
                self.renumber(name, &id, &w.index);
                Some(id)
            }
            Err(e) => {
                self.skip(&format!("session {name} — new-session failed: {e}"));
                None
            }
        }
    }

    fn new_window(&mut self, name: &str, w: &Window) -> Option<String> {
        let mut args = vec![
            "new-window".to_string(),
            "-d".to_string(),
            "-P".to_string(),
            "-F".to_string(),
            "#{window_id}".to_string(),
            "-t".to_string(),
            format!("={name}:{}", w.index),
        ];
        if !w.name.is_empty() {
            args.push("-n".to_string());
            args.push(w.name.clone());
        }
        if let Some(cwd) = first_cwd(w) {
            args.push("-c".to_string());
            args.push(cwd);
        }
        if self.dry {
            return None;
        }
        match t::tmux(&args) {
            Ok(id) => Some(id),
            Err(_) => {
                // Index already in use (a duplicated record, or a base-index
                // shift): take whatever slot is free rather than drop a window.
                args[6] = format!("={name}");
                match t::tmux(&args) {
                    Ok(id) => Some(id),
                    Err(e) => {
                        self.skip(&format!("{name}:{} — new-window failed: {e}", w.index));
                        None
                    }
                }
            }
        }
    }

    /// `new-session` always lands on the session's `base-index`; move the
    /// window if the snapshot recorded a different slot, so `prefix 3` still
    /// means window 3.
    fn renumber(&self, name: &str, win: &str, want: &str) {
        if want.is_empty() {
            return;
        }
        let got = t::display(Some(win), "#{window_index}").unwrap_or_default();
        if got.is_empty() || got == want {
            return;
        }
        t::tmux_ok(["move-window", "-s", win, "-t", &format!("={name}:{want}")]);
    }

    // -- windows -----------------------------------------------------------

    fn window(&mut self, sess: &str, win: &str, w: &Window) {
        println!("  window {} {:?}  {} panes", w.index, w.name, w.panes.len());
        self.counts.windows += 1;

        // tmux inserts a split *after* the pane it splits, so always splitting
        // the pane we just made keeps pane indexes in snapshot order — which is
        // exactly the order `select-layout` maps onto the layout's slots.
        let mut last = if self.dry {
            String::new()
        } else {
            first_pane(win)
        };

        for (i, p) in w.panes.iter().enumerate() {
            if i > 0 {
                match self.split(win, &last, p) {
                    Some(id) => last = id,
                    None if self.dry => {}
                    None => continue,
                }
            }
            self.counts.panes += 1;
            self.pane(sess, w, i, p, &last);
        }

        if w.layout.is_empty() {
            return;
        }
        println!("    layout {}", w.layout);
        if self.dry {
            return;
        }
        // Once, after every pane of this window exists. Per-split application
        // would be undone by the split that follows it.
        if let Err(e) = t::tmux(["select-layout", "-t", win, &w.layout]) {
            self.skip(&format!("{sess}:{} — select-layout failed: {e}", w.index));
        }
    }

    fn split(&mut self, win: &str, from: &str, p: &PaneRec) -> Option<String> {
        let mut args = vec![
            "split-window".to_string(),
            "-d".to_string(),
            "-P".to_string(),
            "-F".to_string(),
            "#{pane_id}".to_string(),
            "-t".to_string(),
            if from.is_empty() {
                win.to_string()
            } else {
                from.to_string()
            },
        ];
        if !p.cwd.is_empty() {
            args.push("-c".to_string());
            args.push(p.cwd.clone());
        }
        if self.dry {
            return None;
        }
        match t::tmux(&args) {
            Ok(id) => Some(id),
            Err(_) => {
                // "no space for new pane" — even the window out and retry once.
                // The real geometry arrives with the layout string at the end,
                // so this intermediate arrangement is throwaway.
                t::tmux_ok(["select-layout", "-t", win, "tiled"]);
                match t::tmux(&args) {
                    Ok(id) => Some(id),
                    Err(e) => {
                        self.skip(&format!("pane {} of {win} — split failed: {e}", p.index));
                        None
                    }
                }
            }
        }
    }

    // -- panes -------------------------------------------------------------

    fn pane(&mut self, sess: &str, w: &Window, i: usize, p: &PaneRec, id: &str) {
        let at = format!("{sess}:{}.{i}", w.index);
        let action = pane_action(p, |uuid| find_transcript(&self.projects, uuid).is_some());
        println!("    pane {i}  {}  {}", p.cwd, action.describe());

        // Borders come back labeled. The pane *title* stays untouched — `talk
        // ping` reads it to decide busy/idle.
        if !self.dry && !id.is_empty() && !p.task.is_empty() {
            t::set_pane_opt(id, t::OPT_TASK, &t::sanitize_format(&p.task));
        }

        match action {
            PaneAction::Shell => {}
            PaneAction::Skip(reason) => self.skip(&format!("{at}  {reason}")),
            PaneAction::Resume(uuid) => {
                self.resumed += 1;
                if self.dry || id.is_empty() {
                    return;
                }
                // Literal text, then Enter as its own key: one `send-keys` would
                // have to push the whole line through tmux's key parser. And
                // `claude --resume` is cwd-scoped, which is why the pane was
                // created in its recorded cwd before this is typed.
                let cmd = format!("claude --resume {uuid}");
                if t::tmux(["send-keys", "-t", id, "-l", &cmd]).is_err()
                    || t::tmux(["send-keys", "-t", id, "Enter"]).is_err()
                {
                    self.resumed -= 1;
                    self.skip(&format!("{at}  send-keys failed for {uuid}"));
                }
            }
        }
    }

    // -- reporting ---------------------------------------------------------

    fn skip(&mut self, line: &str) {
        self.skips.push(line.to_string());
    }

    /// A restore that silently drops half the team is the failure mode this
    /// exists to prevent: every skip is printed with its reason.
    fn summary(&self) {
        let verb = if self.dry {
            "would restore"
        } else {
            "restored"
        };
        println!(
            "\n{verb}: {} sessions, {} windows, {} panes, {} claude sessions resumed, {} skipped",
            self.counts.sessions,
            self.counts.windows,
            self.counts.panes,
            self.resumed,
            self.skips.len()
        );
        if !self.skips.is_empty() {
            println!("skipped:");
            for s in &self.skips {
                println!("  {s}");
            }
        }
        if !self.dry {
            if let Some(first) = self.attach.first() {
                println!("attach: tmux attach -t {first}");
            }
        }
    }
}

/// What a restored pane should do about its recorded Claude session.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PaneAction {
    /// No session id was ever stamped here — a plain shell in the right cwd.
    Shell,
    Resume(String),
    /// A session id we refuse to type; the pane is left as a plain shell.
    Skip(String),
}

impl PaneAction {
    fn describe(&self) -> String {
        match self {
            PaneAction::Shell => "shell".to_string(),
            PaneAction::Resume(u) => format!("claude --resume {u}"),
            PaneAction::Skip(r) => format!("shell (skipped resume: {r})"),
        }
    }
}

/// Pure decision, so "never type a guess, never auto-retry" is testable without
/// a filesystem.
///
/// A missing transcript is a skip, not an error: `claude --resume` on a deleted
/// conversation leaves an error sitting in an otherwise good pane, and the run
/// must still report the rest of the team as restored.
fn pane_action(p: &PaneRec, has_transcript: impl Fn(&str) -> bool) -> PaneAction {
    if p.session_id.is_empty() {
        return PaneAction::Shell;
    }
    if !is_full_uuid(&p.session_id) {
        return PaneAction::Skip(format!(
            "{:?} is not a full session UUID (--resume rejects short ids)",
            p.session_id
        ));
    }
    if !has_transcript(&p.session_id) {
        return PaneAction::Skip(format!(
            "no transcript ~/.claude/projects/*/{}.jsonl",
            p.session_id
        ));
    }
    PaneAction::Resume(p.session_id.clone())
}

/// 8-4-4-4-12 hex. This string is typed into a live shell, so nothing that is
/// not exactly a resume handle gets through — and `claude --resume` rejects the
/// 8-char short id anyway (per the `claude-session-dispatch` skill).
fn is_full_uuid(s: &str) -> bool {
    let groups = [8usize, 4, 4, 4, 12];
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == groups.len()
        && parts
            .iter()
            .zip(groups)
            .all(|(p, n)| p.len() == n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// `~/.claude/projects/<project-slug>/<uuid>.jsonl`. Claude Code derives the
/// slug from the session's cwd, so scan the project dirs rather than
/// reverse-engineering that mapping.
fn find_transcript(root: &Path, uuid: &str) -> Option<PathBuf> {
    let name = format!("{uuid}.jsonl");
    for entry in fs::read_dir(root).ok()?.flatten() {
        let candidate = entry.path().join(&name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn session_exists(name: &str) -> bool {
    t::tmux_ok(["has-session", "-t", &format!("={name}")])
}

fn first_cwd(w: &Window) -> Option<String> {
    w.panes
        .first()
        .map(|p| p.cwd.clone())
        .filter(|c| !c.is_empty())
}

fn first_pane(win: &str) -> String {
    t::tmux(["list-panes", "-t", win, "-F", "#{pane_id}"])
        .unwrap_or_default()
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

fn cmd_list() -> i32 {
    let dir = t::state_dir();
    let mut files: Vec<PathBuf> = match fs::read_dir(&dir) {
        Ok(rd) => rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && is_snapshot_name(p))
            .collect(),
        Err(_) => Vec::new(),
    };
    if files.is_empty() {
        println!("no snapshots in {}", dir.display());
        return 0;
    }
    files.sort_by_key(|p| std::cmp::Reverse(mtime(p)));

    for f in &files {
        let name = f
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let when = date(&["-d", &format!("@{}", mtime(f)), "+%Y-%m-%d %H:%M"])
            .unwrap_or_else(|| "?".to_string());
        // A corrupt file is named as such rather than skipped — that is exactly
        // the file you need to know about before trusting a restore.
        let summary = match fs::read_to_string(f)
            .map_err(|e| e.to_string())
            .and_then(|text| parse(&text).map_err(|e| e.to_string()))
        {
            Ok(snap) => format!("{}  [{}]", snap.counts(), names(&snap)),
            Err(e) => format!("CORRUPT — {e}"),
        };
        println!("{when}  {name:<26}  {summary}");
    }
    0
}

fn is_snapshot_name(p: &Path) -> bool {
    let Some(n) = p.file_name().map(|s| s.to_string_lossy().to_string()) else {
        return false;
    };
    n == LAST || (n.starts_with("layout-") && n.ends_with(".tsv"))
}

fn mtime(p: &Path) -> u64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn names(snap: &Snapshot) -> String {
    snap.sessions
        .iter()
        .map(|s| s.name.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A real `#{window_layout}` string: commas, braces, brackets and a
    /// checksum — the shape a naive comma-separated format would shred.
    const LAYOUT: &str =
        "bb62,220x59,0,0{110x59,0,0,1,109x59,111,0[109x29,111,0,2,109x29,111,30,3]}";
    const UUID_A: &str = "aaaaaaaa-1111-2222-3333-444444444444";
    const UUID_B: &str = "bbbbbbbb-1111-2222-3333-444444444444";

    fn pane(index: &str, cwd: &str, sid: &str, task: &str) -> PaneRec {
        PaneRec {
            index: index.to_string(),
            cwd: cwd.to_string(),
            session_id: sid.to_string(),
            task: task.to_string(),
        }
    }

    fn sample() -> Snapshot {
        Snapshot {
            sessions: vec![
                Session {
                    name: "main".to_string(),
                    windows: vec![Window {
                        index: "0".to_string(),
                        name: "cc:tmux".to_string(),
                        layout: LAYOUT.to_string(),
                        panes: vec![
                            pane("0", "/home/fenrir/.tmux", UUID_A, "fix-keyd-ime"),
                            pane("1", "/home/fenrir/code/foo", "", ""),
                            pane("2", "/home/fenrir", UUID_B, "review-plan"),
                        ],
                    }],
                },
                Session {
                    name: "work2".to_string(),
                    windows: vec![
                        Window {
                            index: "1".to_string(),
                            name: "shell".to_string(),
                            layout: "abcd,80x24,0,0,0".to_string(),
                            panes: vec![pane("0", "/tmp", "", "")],
                        },
                        Window {
                            index: "4".to_string(),
                            name: "cc:api".to_string(),
                            layout: "ef01,80x24,0,0,7".to_string(),
                            panes: vec![pane("0", "/srv/api", UUID_A, "")],
                        },
                    ],
                },
            ],
        }
    }

    // -- serialization round trip -----------------------------------------

    #[test]
    fn round_trip_is_identical() {
        let snap = sample();
        let parsed = parse(&serialize(&snap)).expect("round trip parses");
        assert_eq!(parsed, snap);
        // And it is stable: re-serializing the parsed model is byte-identical.
        assert_eq!(serialize(&parsed), serialize(&snap));
    }

    #[test]
    fn pane_order_within_a_window_is_preserved() {
        let snap = parse(&serialize(&sample())).unwrap();
        let panes = &snap.sessions[0].windows[0].panes;
        assert_eq!(panes.len(), 3);
        assert_eq!(
            panes.iter().map(|p| p.cwd.as_str()).collect::<Vec<_>>(),
            vec![
                "/home/fenrir/.tmux",
                "/home/fenrir/code/foo",
                "/home/fenrir"
            ]
        );
        assert_eq!(
            panes.iter().map(|p| p.index.as_str()).collect::<Vec<_>>(),
            vec!["0", "1", "2"]
        );
    }

    #[test]
    fn a_layout_string_with_commas_and_braces_survives() {
        let snap = parse(&serialize(&sample())).unwrap();
        assert_eq!(snap.sessions[0].windows[0].layout, LAYOUT);
    }

    #[test]
    fn an_empty_session_id_round_trips_as_empty() {
        let snap = parse(&serialize(&sample())).unwrap();
        let p = &snap.sessions[0].windows[0].panes[1];
        assert_eq!(p.session_id, "");
        assert_eq!(p.task, "");
        assert_eq!(snap.counts().claude, 3);
    }

    #[test]
    fn tabs_and_newlines_in_a_field_cannot_corrupt_the_file() {
        let snap = Snapshot {
            sessions: vec![Session {
                name: "we\tird".to_string(),
                windows: vec![Window {
                    index: "0".to_string(),
                    name: "two\nlines".to_string(),
                    layout: "back\\slash".to_string(),
                    panes: vec![pane("0", "/tmp/a\tb", "", "task\ttab")],
                }],
            }],
        };
        let text = serialize(&snap);
        // V, S, W, P — not one field has split a line or a column.
        assert_eq!(text.lines().count(), 4);
        assert_eq!(parse(&text).unwrap(), snap);
    }

    #[test]
    fn counts_add_up() {
        let c = sample().counts();
        assert_eq!((c.sessions, c.windows, c.panes, c.claude), (2, 3, 5, 3));
    }

    // -- parse rejections --------------------------------------------------

    #[test]
    fn a_short_record_is_rejected_not_mis_parsed() {
        // A P record truncated mid-write: cwd present, ids gone.
        let e = parse("V\t1\nS\tmain\nW\t0\tw\tlay\nP\t0\t/tmp\n").unwrap_err();
        assert_eq!(e.line, 4);
        assert!(e.msg.contains("wants 5 fields, got 3"), "{}", e.msg);
        assert_eq!(e.to_string(), "line 4: \"P\" record wants 5 fields, got 3");
    }

    #[test]
    fn an_over_long_record_is_rejected_too() {
        let e = parse("V\t1\nS\tmain\nW\t0\tw\tlay\tstray\n").unwrap_err();
        assert!(e.msg.contains("wants 4 fields, got 5"), "{}", e.msg);
    }

    #[test]
    fn records_out_of_order_are_rejected() {
        let e = parse("V\t1\nW\t0\tw\tlay\n").unwrap_err();
        assert!(e.msg.contains("W record before any S"), "{}", e.msg);
        let e = parse("V\t1\nS\tmain\nP\t0\t/tmp\t\t\n").unwrap_err();
        assert!(e.msg.contains("P record before any W"), "{}", e.msg);
    }

    #[test]
    fn an_unknown_tag_or_version_is_rejected() {
        let e = parse("V\t1\nX\tsomething\n").unwrap_err();
        assert!(e.msg.contains("unknown record tag"), "{}", e.msg);
        let e = parse("V\t99\n").unwrap_err();
        assert!(e.msg.contains("unsupported format version"), "{}", e.msg);
    }

    #[test]
    fn a_broken_escape_is_rejected() {
        let e = parse("V\t1\nS\tbad\\q\n").unwrap_err();
        assert!(e.msg.contains("unknown escape"), "{}", e.msg);
        let e = parse("V\t1\nS\ttrailing\\\n").unwrap_err();
        assert!(e.msg.contains("lone backslash"), "{}", e.msg);
    }

    #[test]
    fn an_empty_file_parses_to_nothing_rather_than_failing() {
        // "nothing to restore" is an exit-0 case, so it must not be an error.
        assert_eq!(parse("").unwrap(), Snapshot::default());
        assert_eq!(parse("V\t1\n").unwrap(), Snapshot::default());
    }

    #[test]
    fn a_file_without_the_version_header_still_parses() {
        let snap = parse("S\tmain\nW\t0\tw\tlay\nP\t0\t/tmp\t\t\n").unwrap();
        assert_eq!(snap.counts().panes, 1);
    }

    // -- resume policy -----------------------------------------------------

    #[test]
    fn a_pane_without_a_session_id_is_a_plain_shell() {
        assert_eq!(
            pane_action(&pane("0", "/tmp", "", "slug"), |_| true),
            PaneAction::Shell
        );
    }

    #[test]
    fn a_present_transcript_resumes() {
        assert_eq!(
            pane_action(&pane("0", "/tmp", UUID_A, ""), |u| u == UUID_A),
            PaneAction::Resume(UUID_A.to_string())
        );
    }

    #[test]
    fn a_missing_transcript_is_a_skip_naming_the_uuid() {
        match pane_action(&pane("0", "/tmp", UUID_A, ""), |_| false) {
            PaneAction::Skip(r) => {
                assert!(r.contains(UUID_A) && r.contains("no transcript"), "{r}")
            }
            other => panic!("expected a skip, got {other:?}"),
        }
    }

    #[test]
    fn a_short_id_is_never_typed_even_if_something_matches_it() {
        match pane_action(&pane("0", "/tmp", "93f4a6c5", ""), |_| true) {
            PaneAction::Skip(r) => assert!(r.contains("not a full session UUID"), "{r}"),
            other => panic!("expected a skip, got {other:?}"),
        }
    }

    #[test]
    fn full_uuid_shape_is_enforced() {
        assert!(is_full_uuid("93f4a6c5-615e-47c8-81d6-9d05078aa68f"));
        assert!(!is_full_uuid("93f4a6c5"));
        assert!(!is_full_uuid(""));
        assert!(!is_full_uuid("93f4a6c5-615e-47c8-81d6-9d05078aa68"));
        assert!(!is_full_uuid("zzzzzzzz-615e-47c8-81d6-9d05078aa68f"));
        // Nothing that would mean something to a shell reaches `send-keys -l`.
        assert!(!is_full_uuid(
            "93f4a6c5-615e-47c8-81d6-9d05078aa68f; rm -rf ~"
        ));
        assert!(!is_full_uuid("$(whoami)"));
    }

    #[test]
    fn action_lines_name_what_will_happen() {
        assert_eq!(PaneAction::Shell.describe(), "shell");
        assert!(PaneAction::Resume(UUID_A.into())
            .describe()
            .starts_with("claude --resume "));
        assert!(PaneAction::Skip("why".into()).describe().contains("why"));
    }

    // -- transcript lookup -------------------------------------------------

    #[test]
    fn transcript_lookup_scans_project_dirs() {
        let root = std::env::temp_dir().join(format!("cc-layout-t{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let project = root.join("-home-fenrir--tmux");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join(format!("{UUID_A}.jsonl")), b"{}").unwrap();

        assert!(find_transcript(&root, UUID_A).is_some());
        assert!(find_transcript(&root, UUID_B).is_none());
        assert!(find_transcript(Path::new("/nonexistent-cc-layout"), UUID_A).is_none());

        let _ = fs::remove_dir_all(&root);
    }

    // -- session naming ----------------------------------------------------

    #[test]
    fn a_taken_name_falls_back_to_restored_then_to_numbers() {
        // `session_exists` needs a server; drive the collision through the
        // in-run claim list, which is the half a dry run depends on.
        let mut r = Restorer {
            dry: true,
            width: 200,
            height: 50,
            projects: PathBuf::from("/nonexistent"),
            claimed: Vec::new(),
            counts: Counts::default(),
            resumed: 0,
            skips: Vec::new(),
            attach: Vec::new(),
        };
        r.claimed.push("main".to_string());
        assert_eq!(r.unique_name("main"), "main-restored");
        r.claimed.push("main-restored".to_string());
        assert_eq!(r.unique_name("main"), "main-restored-2");
        assert_eq!(r.unique_name("other"), "other");
    }
}
