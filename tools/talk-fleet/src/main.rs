//! `talk-fleet` — role addressing and fan-out layered on top of the bash `talk`
//! CLI.
//!
//! `talk`'s primitives (`send_text`, `ping`, `read-since`) are proven and stay
//! untouched; see Collision 3 in [ARCHITECTURE.org](../ARCHITECTURE.org). This
//! binary adds the two things a supervisor actually wants — addressing workers
//! by *role* instead of by `%N`, and fanning one task out to N of them — and
//! shells out to `talk` for every keystroke that reaches a pane. There is no
//! second send path and no second idle heuristic.
//!
//! ```text
//!   talk-fleet role set reviewer          bind $TMUX_PANE as @reviewer
//!   talk-fleet resolve @reviewer          -> %42        (the bash talk's hook)
//!   talk-fleet bcast "audit the diff" @reviewer @impl
//!   talk-fleet collect R1754460000-1234   -> rounds/<id>/reviewer.txt, impl.txt
//! ```
//!
//! Exit codes are part of the contract because callers script against them:
//! `0` everything collected, `1` timeout / partial / a resolution failure,
//! `2` usage error. That deliberately differs from the "always exit 0" rule the
//! key-binding binaries in this workspace follow — nothing here runs from a
//! tmux key or a Claude Code hook.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use tmuxlib as t;

/// Let workers pick the task up before the first ping. Without it every worker
/// reads as idle on the first pass — see [`decide`].
const SETTLE: Duration = Duration::from_secs(5);
/// Ping cadence. `talk ping` reads only the pane title, so this is nearly free.
const POLL: Duration = Duration::from_secs(3);
const DEFAULT_TIMEOUT_SECS: u64 = 900;

const EXIT_OK: i32 = 0;
const EXIT_PARTIAL: i32 = 1;
const EXIT_USAGE: i32 = 2;

const USAGE: &str = "\
talk-fleet — role addressing and fan-out for the `talk` CLI

  talk-fleet role set <name> [<pane>]       bind <name> to a pane (default $TMUX_PANE)
  talk-fleet role unset <name>              drop the binding and the border badge
  talk-fleet role list                      list bindings; dead ones are garbage-collected
  talk-fleet resolve <target>               @role -> %N; anything else passes through
  talk-fleet bcast <message> <target...>    fan one task out; prints round= and manifest=
  talk-fleet collect <round> [--timeout N]  block until every worker answered (default 900s)

Targets are @role, %N, or session:window.pane.
Exit: 0 = all collected, 1 = timeout/partial/unresolvable, 2 = usage error.
";

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(sub) = args.first().map(String::as_str) else {
        eprint!("{USAGE}");
        return EXIT_USAGE;
    };

    if matches!(sub, "help" | "-h" | "--help") {
        print!("{USAGE}");
        return EXIT_OK;
    }

    // `talk` itself hard-fails outside tmux; failing here first gives the same
    // answer with a better message and without a fork.
    if !t::in_tmux() {
        eprintln!("talk-fleet: not inside a tmux session");
        return EXIT_USAGE;
    }

    match sub {
        "role" => cmd_role(&args[1..]),
        "resolve" => cmd_resolve(&args[1..]),
        "bcast" => cmd_bcast(&args[1..]),
        "collect" => cmd_collect(&args[1..]),
        _ => {
            eprintln!("talk-fleet: unknown command: {sub}");
            eprint!("{USAGE}");
            EXIT_USAGE
        }
    }
}

// ---------------------------------------------------------------------------
// Names, ids and markers — pure
// ---------------------------------------------------------------------------

/// Role names become filenames under [`tmuxlib::roles_dir`], so the character
/// set is ASCII-restricted at bind time. [`tmuxlib::resolve_target`] accepts
/// unicode alphanumerics; being stricter here means the wider set is simply
/// never creatable, which is the safe direction.
fn valid_role_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Unique and lexically sortable: rounds list in dispatch order.
fn round_id(epoch: u64, pid: u32) -> String {
    format!("R{epoch}-{pid}")
}

/// Unique per round *and* per pane. Reusing one marker across workers is an
/// observed failure mode — the first worker's reply then satisfies everyone's
/// `read-since`.
fn marker_for(round: &str, pane: &str) -> String {
    format!("CHK-{round}-{}", pane.trim_start_matches('%'))
}

/// What the worker is asked to echo, and what `read-since` searches for.
fn marker_line(marker: &str) -> String {
    format!("=== {marker} ===")
}

/// The display name is the result file's stem: the role without its `@`, or the
/// resolved pane id for a raw target. Both are filename-safe by construction —
/// a raw `session:window.pane` target is deliberately *not* used, since a
/// session name can contain anything.
fn display_name(target: &str, pane: &str) -> String {
    match target.strip_prefix('@') {
        Some(role) if valid_role_name(role) => role.to_string(),
        _ => pane.to_string(),
    }
}

/// Normalize any target to an immutable `%N`, or fail.
///
/// Fails closed at three points, each a verified tmux behaviour (2026-08-06)
/// that would otherwise deliver to the wrong pane:
///   * an **empty target** is not "no pane" to tmux — `-t ''` means *the
///     current pane*, so an unset shell variable would silently retarget;
///   * `display-message -t %9999` exits **0 with empty output** for a pane that
///     does not exist, so a successful exit proves nothing;
///   * the resulting id must still be in the live inventory.
///
/// The first two were found by testing: `role set ghost %9999` bound an empty
/// id, and the subsequent `set-option -t ''` stamped the badge onto an
/// unrelated live pane.
fn resolve_pane_id(target: &str) -> Result<String, String> {
    if target.trim().is_empty() {
        return Err("empty pane target".to_string());
    }
    let id =
        t::display(Some(target), "#{pane_id}").map_err(|_| format!("no such pane: {target}"))?;
    if id.is_empty() || !t::pane_alive(&id) {
        return Err(format!("no such pane: {target}"));
    }
    Ok(id)
}

/// The task, plus provenance and the marker instruction. Handed to `talk send`
/// as a single argv element — never interpolated into a shell string.
fn compose_message(msg: &str, round: &str, marker: &str) -> String {
    format!(
        "{msg}\n\n[via talk-fleet bcast round={round}]\nBegin your reply with the exact line:\n{}",
        marker_line(marker)
    )
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    /// Dispatched, not yet answered.
    Pending,
    /// Reply harvested into `<name>.txt`.
    Collected,
    /// Worker went idle but never echoed its marker.
    NoMarker,
    /// Pane vanished between dispatch and reply.
    Died,
    /// Still pending when `--timeout` expired.
    TimedOut,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Collected => "collected",
            Status::NoMarker => "no-marker",
            Status::Died => "died",
            Status::TimedOut => "timeout",
        }
    }

    fn parse(s: &str) -> Status {
        match s.trim() {
            "collected" => Status::Collected,
            "no-marker" => Status::NoMarker,
            "died" => Status::Died,
            "timeout" => Status::TimedOut,
            _ => Status::Pending,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Worker {
    pane: String,
    name: String,
    marker: String,
    status: Status,
}

fn serialize_manifest(workers: &[Worker]) -> String {
    let mut out = String::new();
    for w in workers {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            w.pane,
            w.name,
            w.marker,
            w.status.as_str()
        ));
    }
    out
}

/// Inverse of [`serialize_manifest`]. Blank lines are skipped; a short or
/// marker-less row is fatal rather than silently yielding a worker whose empty
/// marker would make `read-since` harvest the entire scrollback.
fn parse_manifest(text: &str) -> Result<Vec<Worker>, String> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 4 || f[0].is_empty() || f[2].is_empty() {
            return Err(format!("manifest line {}: malformed row", i + 1));
        }
        out.push(Worker {
            pane: f[0].to_string(),
            name: f[1].to_string(),
            marker: f[2].to_string(),
            status: Status::parse(f[3]),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// The collect state machine
// ---------------------------------------------------------------------------

/// `talk ping`'s three exit codes: 0 idle, 1 busy, 2 unknown (empty title).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ping {
    Idle,
    Busy,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Already resolved this round.
    Skip,
    /// Pane is gone: record it and stop polling.
    RecordDead,
    /// Busy→idle edge: the reply is complete, read it.
    Harvest,
    /// Working. Remember it — this is what arms the harvest.
    NoteBusy,
    /// Not armed yet, or no evidence either way.
    KeepWaiting,
}

/// One worker's next action, given everything known about it.
///
/// The load-bearing rule is the busy→idle *edge*: a worker that has not picked
/// the task up yet is still idle, and reading then harvests the echoed prompt
/// instead of the answer. So idle means "done" only once the worker has been
/// seen busy at least once.
///
/// `Ping::Unknown` (empty pane title) waits rather than counting as busy: an
/// unreadable title is not evidence that the worker started, and arming the
/// harvest on it would let one glitched poll produce a premature read. The
/// `--timeout` bounds the wait either way.
fn decide(done: bool, alive: bool, seen_busy: bool, ping: Ping) -> Action {
    if done {
        return Action::Skip;
    }
    if !alive {
        return Action::RecordDead;
    }
    match ping {
        Ping::Busy => Action::NoteBusy,
        Ping::Idle if seen_busy => Action::Harvest,
        Ping::Idle | Ping::Unknown => Action::KeepWaiting,
    }
}

// ---------------------------------------------------------------------------
// Shelling out to `talk`
// ---------------------------------------------------------------------------

/// `talk` is a bash script symlinked into `~/.local/bin`; a process started
/// from a tmux key binding or a hook inherits a PATH that may not include it,
/// so try the known locations before falling back to PATH.
fn talk_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("TALK_BIN") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    for cand in [
        t::home().join(".local/bin/talk"),
        t::home().join(".claude/skills/talk/talk"),
    ] {
        if cand.is_file() {
            return cand;
        }
    }
    PathBuf::from("talk")
}

fn talk(bin: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {}: {e}", bin.display()))
}

/// One paste + Enter into the worker's input box. This is agent→agent
/// dispatch, so the message is submitted rather than staged.
fn talk_send(bin: &Path, pane: &str, body: &str) -> Result<(), String> {
    let out = Command::new(bin)
        .args(["send", pane])
        // One argv element: the task text is never seen by a shell.
        .arg(body)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {}: {e}", bin.display()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Never call this for a pane that has not just been confirmed alive — `talk
/// ping` also exits 1 when the pane is missing, which would read as "busy".
fn talk_ping(bin: &Path, pane: &str) -> Ping {
    match talk(bin, &["ping", pane]) {
        Ok(out) => match out.status.code() {
            Some(0) => Ping::Idle,
            Some(1) => Ping::Busy,
            _ => Ping::Unknown,
        },
        Err(_) => Ping::Unknown,
    }
}

/// Scrollback after the *last* occurrence of the marker — the reply, not the
/// echoed prompt. `Err` means the marker was never found.
fn talk_read_since(bin: &Path, pane: &str, marker_line: &str) -> Result<String, String> {
    let out = talk(bin, &["read-since", pane, marker_line])?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

// ---------------------------------------------------------------------------
// role
// ---------------------------------------------------------------------------

fn cmd_role(args: &[String]) -> i32 {
    let arg = |n: usize| args.get(n).map(String::as_str);
    match arg(0).unwrap_or("list") {
        "set" => role_set(arg(1), arg(2)),
        "unset" => role_unset(arg(1)),
        "list" | "ls" => role_list(),
        other => {
            eprintln!("talk-fleet: role: unknown subcommand: {other}");
            eprint!("{USAGE}");
            EXIT_USAGE
        }
    }
}

fn role_set(name: Option<&str>, pane: Option<&str>) -> i32 {
    let Some(name) = name else {
        eprintln!("talk-fleet: usage: talk-fleet role set <name> [<pane>]");
        return EXIT_USAGE;
    };
    if !valid_role_name(name) {
        eprintln!("talk-fleet: invalid role name: {name} (allowed: A-Z a-z 0-9 _ -)");
        return EXIT_USAGE;
    }

    let requested = match pane {
        Some(p) => p.to_string(),
        None => match t::current_pane() {
            Some(p) => p,
            None => {
                eprintln!("talk-fleet: no pane given and $TMUX_PANE is unset");
                return EXIT_USAGE;
            }
        },
    };

    // Normalize to `%N`: the registry body is compared against `pane_id`, so a
    // `session:window.pane` binding would never resolve.
    let id = match resolve_pane_id(&requested) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("talk-fleet: {e}");
            return EXIT_USAGE;
        }
    };

    let dir = t::roles_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("talk-fleet: cannot create {}: {e}", dir.display());
        return EXIT_USAGE;
    }

    // Re-binding a name must not leave the previous pane wearing the badge.
    let path = dir.join(name);
    if let Ok(prev) = fs::read_to_string(&path) {
        let prev = prev.trim();
        if !prev.is_empty() && prev != id && t::pane_alive(prev) {
            t::unset_pane_opt(prev, t::OPT_ROLE);
        }
    }

    if let Err(e) = fs::write(&path, &id) {
        eprintln!("talk-fleet: cannot write {}: {e}", path.display());
        return EXIT_USAGE;
    }

    // The badge is rendered by the workspace's single global
    // `pane-border-format` (`#{?@claude_role,[#{@claude_role}],}`). Six
    // features share that one string, so setting it here — or touching
    // `pane-border-status` — would clobber the other five.
    t::set_pane_opt(&id, t::OPT_ROLE, name);

    println!("{name} -> {id}");
    t::message(&format!("role {name} -> {}", t::sanitize_format(&id)));
    EXIT_OK
}

/// Idempotent: unsetting an unbound name is a no-op, not an error, so cleanup
/// paths can run unconditionally.
fn role_unset(name: Option<&str>) -> i32 {
    let Some(name) = name else {
        eprintln!("talk-fleet: usage: talk-fleet role unset <name>");
        return EXIT_USAGE;
    };
    if !valid_role_name(name) {
        eprintln!("talk-fleet: invalid role name: {name}");
        return EXIT_USAGE;
    }

    let path = t::roles_dir().join(name);
    let bound = fs::read_to_string(&path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let _ = fs::remove_file(&path);

    if bound.is_empty() {
        println!("{name} was not bound");
        return EXIT_OK;
    }
    if t::pane_alive(&bound) {
        t::unset_pane_opt(&bound, t::OPT_ROLE);
    }
    println!("{name} unbound (was {bound})");
    EXIT_OK
}

/// Listing is also the garbage collector: a role whose pane is gone is dropped
/// on sight, so the registry can never resolve to a recycled pane id.
fn role_list() -> i32 {
    let dir = t::roles_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return EXIT_OK; // no registry yet == no roles
    };

    let mut rows: Vec<(String, String)> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let id = fs::read_to_string(e.path()).ok()?.trim().to_string();
            Some((name, id))
        })
        .collect();
    rows.sort();

    for (name, id) in &rows {
        if t::pane_alive(id) {
            println!("{name:<14} {id:<6} alive");
        } else {
            println!("{name:<14} {id:<6} DEAD (gc)");
            let _ = fs::remove_file(dir.join(name));
        }
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// resolve — the hook the bash `talk` calls
// ---------------------------------------------------------------------------

fn cmd_resolve(args: &[String]) -> i32 {
    let Some(target) = args.first().map(String::as_str) else {
        eprintln!("talk-fleet: usage: talk-fleet resolve <target>");
        return EXIT_USAGE;
    };
    match t::resolve_target(target) {
        Ok(id) => {
            println!("{id}");
            EXIT_OK
        }
        Err(e) => {
            eprintln!("talk-fleet: {e}");
            EXIT_PARTIAL
        }
    }
}

// ---------------------------------------------------------------------------
// bcast
// ---------------------------------------------------------------------------

fn cmd_bcast(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("talk-fleet: usage: talk-fleet bcast <message> <target...>");
        return EXIT_USAGE;
    }
    let msg = &args[0];

    // Resolve every target before sending anything: a typo in the third target
    // must not leave two workers holding a task nobody will collect.
    let round = round_id(t::now_epoch(), std::process::id());
    let mut workers: Vec<Worker> = Vec::new();
    for target in &args[1..] {
        let pane = match t::resolve_target(target) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("talk-fleet: {e}");
                return EXIT_USAGE;
            }
        };
        // Normalize `session:window.pane` to `%N` so the manifest, the marker
        // and the ping all key off the same immutable id.
        let pane = match resolve_pane_id(&pane) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("talk-fleet: {e} (from target {target})");
                return EXIT_USAGE;
            }
        };
        if workers.iter().any(|w| w.pane == pane) {
            eprintln!("talk-fleet: {target} resolves to {pane}, already in this round — skipped");
            continue;
        }
        workers.push(Worker {
            marker: marker_for(&round, &pane),
            name: display_name(target, &pane),
            pane,
            status: Status::Pending,
        });
    }
    if workers.is_empty() {
        eprintln!("talk-fleet: no targets");
        return EXIT_USAGE;
    }

    let dir = t::rounds_dir().join(&round);
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("talk-fleet: cannot create {}: {e}", dir.display());
        return EXIT_USAGE;
    }
    let manifest = dir.join("manifest.tsv");
    if let Err(e) = fs::write(&manifest, serialize_manifest(&workers)) {
        eprintln!("talk-fleet: cannot write {}: {e}", manifest.display());
        return EXIT_USAGE;
    }

    let bin = talk_bin();
    let mut failed = 0;
    for w in &workers {
        let body = compose_message(msg, &round, &w.marker);
        match talk_send(&bin, &w.pane, &body) {
            Ok(()) => eprintln!("sent -> {} ({}) marker={}", w.pane, w.name, w.marker),
            Err(e) => {
                eprintln!("talk-fleet: send to {} ({}) failed: {e}", w.pane, w.name);
                failed += 1;
            }
        }
    }

    // Machine-readable on stdout; the per-worker chatter above stays on stderr.
    println!("round={round}");
    println!("manifest={}", manifest.display());
    if failed > 0 {
        eprintln!("talk-fleet: {failed} of {} sends failed", workers.len());
        return EXIT_PARTIAL;
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// collect
// ---------------------------------------------------------------------------

/// A manifest row plus the two bits the state machine needs.
struct Live {
    w: Worker,
    seen_busy: bool,
    done: bool,
}

fn cmd_collect(args: &[String]) -> i32 {
    let mut round: Option<&str> = None;
    let mut timeout = DEFAULT_TIMEOUT_SECS;

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--timeout" => {
                let Some(v) = args.get(i + 1).and_then(|s| s.parse::<u64>().ok()) else {
                    eprintln!("talk-fleet: --timeout needs a number of seconds");
                    return EXIT_USAGE;
                };
                timeout = v;
                i += 1;
            }
            _ if a.starts_with("--timeout=") => {
                let Ok(v) = a["--timeout=".len()..].parse::<u64>() else {
                    eprintln!("talk-fleet: --timeout needs a number of seconds");
                    return EXIT_USAGE;
                };
                timeout = v;
            }
            _ if a.starts_with('-') => {
                eprintln!("talk-fleet: unknown flag: {a}");
                return EXIT_USAGE;
            }
            _ if round.is_none() => round = Some(a),
            _ => {
                eprintln!("talk-fleet: unexpected argument: {a}");
                return EXIT_USAGE;
            }
        }
        i += 1;
    }

    let Some(round) = round else {
        eprintln!("talk-fleet: usage: talk-fleet collect <round> [--timeout <secs>]");
        return EXIT_USAGE;
    };

    let dir = t::rounds_dir().join(round);
    let manifest = dir.join("manifest.tsv");
    let Ok(text) = fs::read_to_string(&manifest) else {
        eprintln!(
            "talk-fleet: no such round: {round} ({} missing)",
            manifest.display()
        );
        return EXIT_USAGE;
    };
    let workers = match parse_manifest(&text) {
        Ok(w) if !w.is_empty() => w,
        Ok(_) => {
            eprintln!("talk-fleet: round {round} has no workers");
            return EXIT_USAGE;
        }
        Err(e) => {
            eprintln!("talk-fleet: {e}");
            return EXIT_USAGE;
        }
    };

    collect_loop(&dir, &manifest, workers, Duration::from_secs(timeout))
}

fn collect_loop(dir: &Path, manifest: &Path, workers: Vec<Worker>, timeout: Duration) -> i32 {
    let bin = talk_bin();
    let started = Instant::now();
    let total = workers.len();
    let mut live: Vec<Live> = workers
        .into_iter()
        .map(|w| Live {
            // Re-running collect over a finished manifest re-polls only what is
            // still outstanding.
            done: w.status != Status::Pending,
            w,
            seen_busy: false,
        })
        .collect();

    println!(
        "collecting {total} worker(s), timeout {}s",
        timeout.as_secs()
    );
    sleep(SETTLE);

    let mut timed_out = false;
    loop {
        let mut pending = 0;
        for l in &mut live {
            if l.done {
                continue;
            }
            let alive = t::pane_alive(&l.w.pane);
            let ping = if alive {
                talk_ping(&bin, &l.w.pane)
            } else {
                Ping::Unknown
            };

            match decide(l.done, alive, l.seen_busy, ping) {
                Action::Skip => {}
                Action::RecordDead => {
                    write_result(
                        dir,
                        &l.w.name,
                        &format!(
                            "PANE DIED before reply\npane {} vanished during the round; \
                             nothing was harvested.\n",
                            l.w.pane
                        ),
                    );
                    l.w.status = Status::Died;
                    l.done = true;
                    progress(started, &l.w, "DIED — recorded, round continues");
                }
                Action::NoteBusy => {
                    if !l.seen_busy {
                        l.seen_busy = true;
                        progress(started, &l.w, "busy — picked the task up");
                    }
                    pending += 1;
                }
                Action::Harvest => {
                    l.w.status = harvest(&bin, dir, &l.w);
                    l.done = true;
                    progress(
                        started,
                        &l.w,
                        match l.w.status {
                            Status::Collected => "idle — collected",
                            _ => "idle — marker not found",
                        },
                    );
                }
                Action::KeepWaiting => pending += 1,
            }
        }

        if pending == 0 {
            break;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            break;
        }
        sleep(POLL);
    }

    if timed_out {
        for l in &mut live {
            if !l.done {
                write_result(
                    dir,
                    &l.w.name,
                    &format!(
                        "TIMED OUT after {}s\nno reply harvested from pane {}; it never \
                         completed a busy→idle cycle.\nrecover by hand: talk read {} 120\n",
                        timeout.as_secs(),
                        l.w.pane,
                        l.w.pane
                    ),
                );
                l.w.status = Status::TimedOut;
                l.done = true;
                progress(started, &l.w, "TIMEOUT — no reply");
            }
        }
    }

    // The manifest is the round's machine-readable summary; rewrite it with the
    // final per-worker status so a caller need not re-derive it from the files.
    let final_workers: Vec<Worker> = live.into_iter().map(|l| l.w).collect();
    if let Err(e) = fs::write(manifest, serialize_manifest(&final_workers)) {
        eprintln!("talk-fleet: cannot update {}: {e}", manifest.display());
    }

    let collected = final_workers
        .iter()
        .filter(|w| w.status == Status::Collected)
        .count();
    println!("collected {collected}/{total} -> {}", dir.display());

    if collected == total {
        return EXIT_OK;
    }
    for w in final_workers
        .iter()
        .filter(|w| w.status != Status::Collected)
    {
        eprintln!("talk-fleet: {} ({}) {}", w.name, w.pane, w.status.as_str());
    }
    EXIT_PARTIAL
}

/// Read the reply, or leave a file that explains why there is none. Never an
/// empty file — an empty file reads as "the worker had nothing to say".
fn harvest(bin: &Path, dir: &Path, w: &Worker) -> Status {
    let line = marker_line(&w.marker);
    match talk_read_since(bin, &w.pane, &line) {
        Ok(body) if !body.trim().is_empty() => {
            write_result(dir, &w.name, &body);
            Status::Collected
        }
        Ok(_) => {
            write_result(
                dir,
                &w.name,
                &format!(
                    "MARKER FOUND BUT REPLY EMPTY\n{line} is the last thing in pane {}'s \
                     scrollback.\nrecover by hand: talk read {} 120\n",
                    w.pane, w.pane
                ),
            );
            Status::NoMarker
        }
        Err(e) => {
            write_result(
                dir,
                &w.name,
                &format!(
                    "MARKER NOT FOUND: {line}\npane {} answered without echoing the marker, \
                     or the reply scrolled past read-since's 5000-line window.\n\
                     talk said: {e}\nrecover by hand: talk read {} 120\n",
                    w.pane, w.pane
                ),
            );
            Status::NoMarker
        }
    }
}

fn write_result(dir: &Path, name: &str, body: &str) {
    let path = dir.join(format!("{name}.txt"));
    if let Err(e) = fs::write(&path, body) {
        eprintln!("talk-fleet: cannot write {}: {e}", path.display());
    }
}

/// One line per state transition, so a human watching a 15-minute collect can
/// tell it apart from a hang.
fn progress(started: Instant, w: &Worker, what: &str) {
    println!(
        "[{:>4}s] {:<12} {:<6} {what}",
        started.elapsed().as_secs(),
        w.name,
        w.pane
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- names, ids, markers -------------------------------------------------

    #[test]
    fn role_names_are_filename_safe() {
        assert!(valid_role_name("reviewer"));
        assert!(valid_role_name("impl-2"));
        assert!(valid_role_name("worker_A9"));
        assert!(!valid_role_name(""));
        assert!(!valid_role_name("../etc/passwd"));
        assert!(!valid_role_name("two words"));
        assert!(!valid_role_name("re/viewer"));
        assert!(!valid_role_name("審查者")); // ASCII-only at bind time
    }

    #[test]
    fn round_ids_are_unique_and_sortable() {
        assert_eq!(round_id(1754460000, 1234), "R1754460000-1234");
        assert!(round_id(1754460000, 1) < round_id(1754460001, 1));
        assert_ne!(round_id(1754460000, 1), round_id(1754460000, 2));
    }

    #[test]
    fn markers_differ_per_pane_within_a_round() {
        let r = round_id(1754460000, 1234);
        let a = marker_for(&r, "%42");
        assert_eq!(a, "CHK-R1754460000-1234-42");
        assert_ne!(a, marker_for(&r, "%43"));
        assert_ne!(a, marker_for(&round_id(1754460001, 1234), "%42"));
        assert_eq!(marker_line(&a), "=== CHK-R1754460000-1234-42 ===");
    }

    #[test]
    fn display_name_prefers_the_role_else_the_pane() {
        assert_eq!(display_name("@reviewer", "%42"), "reviewer");
        assert_eq!(display_name("%42", "%42"), "%42");
        // A raw session target must never become the filename.
        assert_eq!(display_name("main:1.0", "%42"), "%42");
        assert_eq!(display_name("@bad name", "%42"), "%42");
    }

    #[test]
    fn composed_message_carries_provenance_and_marker() {
        let body = compose_message("audit the diff", "R1-2", "CHK-R1-2-42");
        assert!(body.starts_with("audit the diff\n"));
        assert!(body.contains("[via talk-fleet bcast round=R1-2]"));
        // The marker instruction is the last thing the worker reads.
        assert!(body.ends_with("=== CHK-R1-2-42 ==="));
    }

    #[test]
    fn composed_message_leaves_shell_metacharacters_verbatim() {
        let raw = "run `id`; echo $HOME && rm -rf \"x\"";
        assert!(compose_message(raw, "R1-2", "M").starts_with(raw));
    }

    // -- manifest ------------------------------------------------------------

    fn sample() -> Vec<Worker> {
        vec![
            Worker {
                pane: "%42".into(),
                name: "reviewer".into(),
                marker: "CHK-R1-2-42".into(),
                status: Status::Pending,
            },
            Worker {
                pane: "%43".into(),
                name: "impl".into(),
                marker: "CHK-R1-2-43".into(),
                status: Status::Collected,
            },
        ]
    }

    #[test]
    fn manifest_round_trips() {
        let w = sample();
        assert_eq!(parse_manifest(&serialize_manifest(&w)).unwrap(), w);
    }

    #[test]
    fn manifest_rows_are_four_tab_separated_fields() {
        let text = serialize_manifest(&sample());
        assert_eq!(
            text.lines().next().unwrap(),
            "%42\treviewer\tCHK-R1-2-42\tpending"
        );
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn manifest_parse_rejects_rows_that_would_harvest_everything() {
        assert!(parse_manifest("%42\treviewer\n").is_err()); // short
        assert!(parse_manifest("%42\treviewer\t\tpending\n").is_err()); // empty marker
        assert!(parse_manifest("\treviewer\tM\tpending\n").is_err()); // no pane
    }

    #[test]
    fn manifest_parse_skips_blanks_and_defaults_unknown_status() {
        let w = parse_manifest("\n%42\tr\tM\twhatever\n\n").unwrap();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].status, Status::Pending);
    }

    #[test]
    fn every_status_round_trips_through_its_string() {
        for s in [
            Status::Pending,
            Status::Collected,
            Status::NoMarker,
            Status::Died,
            Status::TimedOut,
        ] {
            assert_eq!(Status::parse(s.as_str()), s);
        }
    }

    // -- the collect state machine ------------------------------------------

    #[test]
    fn idle_before_busy_does_not_harvest() {
        // The worker has not picked the task up; reading now would harvest the
        // echoed prompt instead of the answer.
        assert_eq!(decide(false, true, false, Ping::Idle), Action::KeepWaiting);
    }

    #[test]
    fn busy_arms_the_harvest() {
        assert_eq!(decide(false, true, false, Ping::Busy), Action::NoteBusy);
        assert_eq!(decide(false, true, true, Ping::Busy), Action::NoteBusy);
    }

    #[test]
    fn busy_then_idle_harvests() {
        assert_eq!(decide(false, true, true, Ping::Idle), Action::Harvest);
    }

    #[test]
    fn a_dead_pane_short_circuits_every_other_signal() {
        for seen_busy in [false, true] {
            for ping in [Ping::Idle, Ping::Busy, Ping::Unknown] {
                assert_eq!(decide(false, false, seen_busy, ping), Action::RecordDead);
            }
        }
    }

    #[test]
    fn a_finished_worker_is_skipped_even_if_its_pane_is_gone() {
        assert_eq!(decide(true, false, true, Ping::Idle), Action::Skip);
        assert_eq!(decide(true, true, true, Ping::Idle), Action::Skip);
    }

    #[test]
    fn an_unreadable_title_neither_arms_nor_harvests() {
        assert_eq!(
            decide(false, true, false, Ping::Unknown),
            Action::KeepWaiting
        );
        // Already armed: an unknown title is still not an idle edge.
        assert_eq!(
            decide(false, true, true, Ping::Unknown),
            Action::KeepWaiting
        );
    }

    #[test]
    fn the_whole_worker_lifecycle_walks_waiting_to_harvest() {
        let mut seen_busy = false;
        let mut done = false;
        let mut actions = Vec::new();
        // idle (not started) → busy → busy → idle (finished)
        for ping in [Ping::Idle, Ping::Busy, Ping::Busy, Ping::Idle] {
            let a = decide(done, true, seen_busy, ping);
            match a {
                Action::NoteBusy => seen_busy = true,
                Action::Harvest => done = true,
                _ => {}
            }
            actions.push(a);
        }
        assert_eq!(
            actions,
            vec![
                Action::KeepWaiting,
                Action::NoteBusy,
                Action::NoteBusy,
                Action::Harvest
            ]
        );
        // And the harvested worker is never polled again.
        assert_eq!(decide(done, true, seen_busy, Ping::Idle), Action::Skip);
    }
}
