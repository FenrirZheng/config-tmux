//! `seek` — copy-mode jump-and-grab; replaces tmux-thumbs on `prefix Space`.
//!
//!   seek word [--claude] [<pane>]    the token under the cursor, or the query
//!   seek line [--claude] [<pane>]    the whole logical line under the cursor
//!
//! `prefix Space` is pure tmux (`copy-mode` + `command-prompt` +
//! `search-backward-incremental`); this binary is only what the four output
//! keys `w` / `W` / `l` / `L` invoke afterwards. It never sends keys and never
//! cancels copy-mode, which is what makes those keys repeatable across one
//! search.
//!
//! Specification: `records/2026-08-09-1116-tmux-seek/tmux-seek.org`, section
//! **Current contract**. The tickets below that section are the rationale
//! trail; several describe mechanisms that no longer exist (a `v` key, an
//! `@seek_origin` pane option, `copy_cursor_word`). Current contract wins.

use std::io::Write;
use std::process::{Command, Stdio};

use tmuxlib as t;
use unicode_width::UnicodeWidthChar;

/// Status-line payloads are cut here, matching `@claude_activity`'s contract.
const TEXT_MAX: usize = 48;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(msg) = run(&args) {
        t::message_literal(&format!("seek: {msg}"));
    }
    // Deliberately no non-zero exit on any path: a failing binding surfaces as
    // a tmux error popup (tools/ARCHITECTURE.org, "Conventions").
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Grain {
    Word,
    Line,
}

/// An unrecognised word is an error rather than a silent default: a typo in a
/// key binding would otherwise grab the wrong grain forever without saying so.
fn parse_args(args: &[String]) -> Result<(Grain, bool, Option<String>), String> {
    let grain = match args.first().map(String::as_str) {
        Some("word") => Grain::Word,
        Some("line") => Grain::Line,
        Some(other) => return Err(format!("unknown grain: {other}")),
        None => return Err("usage: seek <word|line> [--claude] [<pane>]".into()),
    };
    let mut claude = false;
    let mut pane: Option<String> = None;
    for a in &args[1..] {
        match a.as_str() {
            "--claude" => claude = true,
            other if other.starts_with('-') => return Err(format!("unknown flag: {other}")),
            other if other.is_empty() => {}
            // A second positional would silently retarget the grab, and a typo
            // (`seek word wrod`) would surface only as an opaque state-read
            // failure. Reject it, like the sibling `to-claude` parser.
            other if pane.is_some() => return Err(format!("unexpected argument: {other}")),
            other => pane = Some(other.to_string()),
        }
    }
    Ok((grain, claude, pane))
}

// ---------------------------------------------------------------------------
// The pure half — no tmux, no processes, all unit-tested below
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Deliver {
    /// The search query itself, verbatim — the multi-token case.
    Query,
    /// The whitespace-delimited token under the cursor.
    Token,
    /// The whole logical line under the cursor.
    Line,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Route {
    Ref,
    Paste,
}

/// **Pure function 1 — the grain splitter.**
///
/// Line grain always takes the line, whatever the query looks like. Word grain
/// takes the query verbatim only when a live search matched something the
/// cursor is still sitting on and that query spans more than one token — the
/// case tmux cannot express as a "word".
///
/// `cursor_on_match` is the [T5] guard: `search_present` stays 1 after the user
/// moves the cursor off the match with `C-n` / `C-p`, so without it a later
/// grab would deliver the query rather than what the cursor is actually on.
fn choose_grain(
    grain: Grain,
    search_present: bool,
    query: &str,
    cursor_on_match: bool,
) -> Deliver {
    if grain == Grain::Line {
        return Deliver::Line;
    }
    let q = query.trim();
    if search_present && cursor_on_match && q.chars().any(char::is_whitespace) {
        Deliver::Query
    } else {
        Deliver::Token
    }
}

/// True when the text at `idx` still begins with the query.
///
/// Backward search lands on the match *start*, so at the landing this holds
/// exactly. Case-insensitive because tmux's incremental search is.
fn cursor_on_match(line: &str, idx: usize, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return false;
    }
    let rest: String = line.chars().skip(idx).collect();
    rest.to_lowercase().starts_with(&q.to_lowercase())
}

/// **Pure function 2 — the token extractor.**
///
/// The whitespace-delimited run containing `idx`, with surrounding punctuation
/// trimmed. `copy_cursor_word` is deliberately unused: tmux's default
/// `word-separators` contain `. / : -`, so it yields `home` on
/// `/home/fenrir/.tmux/tools/seek.rs` and `rs` on `main.rs:42` (ADR-0004).
fn extract_token(line: &str, idx: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    if idx >= chars.len() || chars[idx].is_whitespace() {
        return None;
    }
    let mut start = idx;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    let mut end = idx;
    while end + 1 < chars.len() && !chars[end + 1].is_whitespace() {
        end += 1;
    }
    let raw: String = chars[start..=end].iter().collect();
    let token = trim_punctuation(&raw);
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// Strip wrapping punctuation without eating path syntax.
///
/// A leading `.` or `/` is part of the token (`.bashrc`, `/etc/hosts`), so only
/// openers come off the front. If trimming would empty the token it is kept
/// whole instead — that is what leaves `..` intact for the classifier.
fn trim_punctuation(s: &str) -> String {
    const LEAD: &[char] = &['(', '[', '{', '<', '"', '\'', '`', ',', ';'];
    const TRAIL: &[char] = &[')', ']', '}', '>', '"', '\'', '`', ',', ';', ':', '.', '!', '?'];
    let trimmed = s
        .trim_start_matches(|c| LEAD.contains(&c))
        .trim_end_matches(|c| TRAIL.contains(&c));
    if trimmed.is_empty() {
        s.to_string()
    } else {
        trimmed.to_string()
    }
}

/// **Pure function 3 — the path classifier.**
///
/// `--ref` when the token contains `/`, or matches `name.ext:digits`, or is a
/// bare filename whose final dot-segment (1-8 chars) carries at least one ASCII
/// letter. Everything else is `--paste`. The letter test is what separates
/// `main.rs` from `v0.15.1`.
fn classify_path(token: &str) -> Route {
    if token.contains('/') {
        return Route::Ref;
    }
    if let Some((head, tail)) = token.rsplit_once(':') {
        if !tail.is_empty()
            && tail.chars().all(|c| c.is_ascii_digit())
            && has_letter_bearing_ext(head)
        {
            return Route::Ref;
        }
    }
    // "Bare filename" excludes anything still carrying a colon: `main.rs:x`
    // would otherwise qualify, because its final dot-segment (`rs:x`) is short
    // and letter-bearing. A colon is only meaningful in the `:digits` form
    // handled above. (Clarifies the map's Path rule, which did not say so.)
    if !token.contains(':') && has_letter_bearing_ext(token) {
        return Route::Ref;
    }
    Route::Paste
}

/// Only an extracted **token** is ever classified.
///
/// The contract says line grain always uses `--paste`, but a naive
/// `classify_path(&text)` would route the line `cargo build && ./target/foo` to
/// `--ref` on the strength of its slash, and `to-claude --ref` would type it
/// into the composer as `@cargo build && ./target/foo` — a file reference to
/// something that is not a file. A multi-token query has the same shape
/// (searching `src/main.rs and` would yield `@src/main.rs and`), so it is
/// decided the same way rather than left to fall through: the path rule
/// classifies a token, and neither of these is one.
fn route_for(delivery: Deliver, token: &str) -> Route {
    match delivery {
        Deliver::Token => classify_path(token),
        Deliver::Line | Deliver::Query => Route::Paste,
    }
}

fn has_letter_bearing_ext(token: &str) -> bool {
    let Some((_, ext)) = token.rsplit_once('.') else {
        return false;
    };
    let len = ext.chars().count();
    (1..=8).contains(&len) && ext.chars().any(|c| c.is_ascii_alphabetic())
}

/// How many screen rows `line` occupies when wrapped into `width`-cell rows.
///
/// Deliberately **not** `ceil(total_cells / width)`: tmux never splits a
/// double-width character across a row boundary, it leaves the last cell blank
/// and wraps. Those wasted cells are invisible to a division, so the cheap
/// formula undercounts. Measured on tmux 3.5a, 41-column pane, 41 × `中`: the
/// raw rows are 20 + 20 + 1 characters, i.e. **3** rows, while
/// `ceil(82 / 41)` says 2 — and every logical line below it then resolves one
/// row off, which is a silent wrong-line grab.
fn rows_for(line: &str, width: usize) -> usize {
    let w = width.max(1);
    let mut rows = 1usize;
    let mut col = 0usize;
    for ch in line.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if cw == 0 {
            continue;
        }
        if col + cw > w {
            rows += 1;
            col = cw;
        } else {
            col += cw;
        }
    }
    rows
}

/// Char index of the cell at (`row` within the wrapped line, `col` within that
/// row), laid out with the same wide-character wrap rule as [`rows_for`].
///
/// This replaces the old `rows_in * pane_width + cursor_x` arithmetic, which
/// inherited the same wasted-cell error.
fn wrapped_char_index(line: &str, width: usize, row: usize, col: usize) -> usize {
    let w = width.max(1);
    let mut r = 0usize;
    let mut c = 0usize;
    for (i, ch) in line.chars().enumerate() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if cw == 0 {
            continue;
        }
        if c + cw > w {
            r += 1;
            c = 0;
        }
        if r > row {
            return i;
        }
        if r == row && col < c + cw {
            return i;
        }
        c += cw;
    }
    line.chars().count()
}

/// Which logical line covers viewport row `y`, and how many rows into it that is.
///
/// `capture-pane -J` joins wrapped rows, so the joined line index is *not* the
/// screen row — measured on tmux 3.5a: a 59-column path in a 40-column pane
/// occupies screen rows 1-2 but is joined line 1. `-J` also only joins inside
/// the requested range, so a single-row capture returns the fragment.
fn locate_row(lines: &[String], pane_width: usize, y: usize) -> Option<(usize, usize)> {
    let mut row = 0usize;
    for (i, line) in lines.iter().enumerate() {
        let rows = rows_for(line, pane_width);
        if y < row + rows {
            return Some((i, y - row));
        }
        row += rows;
    }
    None
}

// ---------------------------------------------------------------------------
// The tmux half
// ---------------------------------------------------------------------------

struct State {
    in_mode: bool,
    search_present: bool,
    alternate_on: bool,
    cursor_x: usize,
    cursor_y: usize,
    /// Lines the copy-mode viewport is scrolled *up* from the pane's bottom.
    /// Zero when the view is at the live screen.
    scroll: usize,
    pane_width: usize,
    pane_height: usize,
    query: String,
}

/// One `display-message` round trip for every field.
///
/// `pane_search_string` goes last and is taken as the whole remainder: a query
/// may contain anything the user typed, tabs included, so it must not be split.
fn read_state(pane: &str) -> Result<State, String> {
    const FMT: &str = "#{pane_in_mode}\t#{search_present}\t#{alternate_on}\t\
                       #{copy_cursor_x}\t#{copy_cursor_y}\t#{scroll_position}\t\
                       #{pane_width}\t#{pane_height}\t#{pane_search_string}";
    let raw = t::display(Some(pane), FMT)?;
    let f: Vec<&str> = raw.splitn(9, '\t').collect();
    if f.len() < 8 {
        return Err("could not read copy-mode state".into());
    }
    let num = |s: &str| s.trim().parse::<usize>().unwrap_or(0);
    Ok(State {
        in_mode: f[0].trim() == "1",
        search_present: f[1].trim() == "1",
        alternate_on: f[2].trim() == "1",
        cursor_x: num(f[3]),
        cursor_y: num(f[4]),
        scroll: num(f[5]),
        pane_width: num(f[6]),
        pane_height: num(f[7]),
        query: f.get(8).unwrap_or(&"").to_string(),
    })
}

/// The logical (wrap-joined) line under the cursor, plus the cursor's char
/// index within it.
/// The logical (wrap-joined) line under the cursor, plus the cursor's char
/// index within it.
///
/// **The capture range must follow the copy-mode viewport.** `copy_cursor_y` is
/// relative to what the user is looking at, but `capture-pane` knows nothing
/// about the copy-mode scroll offset: `-S 0` is always the live, un-scrolled
/// top. Since the whole feature is "search backwards into scrollback, then
/// grab", the scrolled case is the *dominant* path, not an edge case — and it
/// fails silently, because `search_present`, the match test and the extractor
/// all succeed on whatever text happens to sit at that row of the live screen.
/// Measured on tmux 3.5a (40x10 pane, 60 lines of history): after searching
/// `needle005`, `scroll_position` was 165 and `-S 0 -E 9` returned LINE058-060,
/// so the grab delivered `needle060` and reported `→ CLIP needle060`.
fn logical_line(pane: &str, st: &State) -> Result<(String, usize), String> {
    let top = -(st.scroll as i64);
    let bottom = st.pane_height.saturating_sub(1) as i64 - st.scroll as i64;
    let out = t::tmux([
        "capture-pane",
        "-p",
        "-J",
        "-t",
        pane,
        "-S",
        &top.to_string(),
        "-E",
        &bottom.to_string(),
    ])?;
    let lines: Vec<String> = out.split('\n').map(|s| s.to_string()).collect();
    let (idx, rows_in) =
        locate_row(&lines, st.pane_width, st.cursor_y).ok_or("nothing under the cursor")?;
    let line = lines.get(idx).cloned().unwrap_or_default();
    let char_idx = wrapped_char_index(&line, st.pane_width, rows_in, st.cursor_x);
    Ok((line, char_idx))
}

fn run(args: &[String]) -> Result<(), String> {
    let (grain, claude, pane_arg) = parse_args(args)?;
    let pane = pane_arg
        .or_else(t::current_pane)
        .ok_or("no pane id — the binding must pass #{pane_id}")?;

    let st = read_state(&pane)?;
    if !st.in_mode {
        // The pane left copy-mode before this ran. Saying "nothing under the
        // cursor" would be a lie about an empty cell; stay quiet.
        return Ok(());
    }

    let (line, idx) = logical_line(&pane, &st)?;
    let on_match = cursor_on_match(&line, idx, &st.query);

    let delivery = choose_grain(grain, st.search_present, &st.query, on_match);
    let text = match delivery {
        Deliver::Query => st.query.trim().to_string(),
        // Line grain trims both ends [T4]: `capture-pane -J` preserves the
        // trailing spaces tmux pads rows with, and leading indentation is noise
        // in a clipboard grab.
        Deliver::Line => line.trim().to_string(),
        Deliver::Token => extract_token(&line, idx).ok_or("nothing under the cursor")?,
    };
    if text.is_empty() {
        return Err("nothing under the cursor".into());
    }

    // Both sinks first, always, for all four keys.
    let wl_ok = wl_copy(&text);
    let buf_ok = t::tmux_ok(["set-buffer", "--", &text]);

    let shown = t::truncate(&text, TEXT_MAX);
    if claude {
        // to-claude speaks for the delivery itself. A wl-copy failure is
        // reported *after* it returns so the receipt does not overwrite it.
        let sent = to_claude(&text, route_for(delivery, &text));
        if !wl_ok {
            t::message_literal("⚠ wl-copy failed");
        }
        sent?;
    } else {
        let alt = if st.search_present && st.alternate_on {
            " (visible screen only)"
        } else {
            ""
        };
        if wl_ok {
            t::message_literal(&format!("→ CLIP {shown}{alt}"));
        } else if buf_ok {
            t::message_literal(&format!("⚠ wl-copy failed → BUFFER {shown}{alt}"));
        } else {
            // Never claim the buffer as the surviving sink when it did not
            // survive either — the degrade message exists to say where the text
            // actually landed.
            t::message_literal("⚠ wl-copy AND set-buffer failed — nothing was copied");
        }
    }
    Ok(())
}

/// Best-effort Wayland clipboard write. `false` degrades the message; it never
/// refuses the grab, because `set-buffer` still landed.
fn wl_copy(text: &str) -> bool {
    let Ok(mut child) = Command::new("wl-copy")
        .arg("--")
        .arg(text)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    matches!(child.wait(), Ok(s) if s.success())
}

/// Pipe the text to `to-claude` on stdin — it reads stdin only and rejects
/// unknown argv words, so the grabbed text must never become an argument.
///
/// A non-zero exit is swallowed: `to-claude`'s own `fail()` already
/// display-messages. Failing to *execute* it is different and is reported,
/// because the load-time guard covers only the `seek` binary.
fn to_claude(text: &str, route: Route) -> Result<(), String> {
    let bin = t::tmux_dir().join("tools/target/release/to-claude");
    let mode = match route {
        Route::Ref => "--ref",
        Route::Paste => "--paste",
    };
    let mut child = Command::new(&bin)
        .arg(mode)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|_| {
            "to-claude not built — cd ~/.tmux/tools && cargo build --release".to_string()
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let _ = child.wait();
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests — the fixture is the map's worked-edge table under "** Path rule"
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_rule_worked_edges() {
        let cases: &[(&str, Route)] = &[
            ("main.rs", Route::Ref),
            (".bashrc", Route::Ref),
            ("foo.tar.gz", Route::Ref),
            ("main.rs:42", Route::Ref),
            ("https://x.io/a", Route::Ref),
            ("v0.15.1", Route::Paste),
            ("1.5", Route::Paste),
            ("Makefile", Route::Paste),
            ("..", Route::Paste),
        ];
        for (token, want) in cases {
            assert_eq!(classify_path(token), *want, "classifying {token}");
        }
    }

    #[test]
    fn path_rule_extra_shapes() {
        assert_eq!(classify_path("/home/fenrir/.tmux"), Route::Ref);
        assert_eq!(classify_path("./rel/path"), Route::Ref);
        // A colon suffix only routes when the head is itself file-shaped.
        assert_eq!(classify_path("foo:42"), Route::Paste);
        assert_eq!(classify_path("main.rs:x"), Route::Paste);
        // An over-long final segment is prose, not an extension.
        assert_eq!(classify_path("sentence.endingword"), Route::Paste);
        assert_eq!(classify_path("plainword"), Route::Paste);
    }

    #[test]
    fn token_extraction_trims_surrounding_punctuation() {
        assert_eq!(extract_token("see main.rs, here", 4).as_deref(), Some("main.rs"));
        assert_eq!(extract_token("(main.rs)", 1).as_deref(), Some("main.rs"));
        assert_eq!(extract_token("\"quoted\"", 2).as_deref(), Some("quoted"));
        assert_eq!(extract_token("end of line.", 7).as_deref(), Some("line"));
        // Trimming that would empty the token keeps it whole — this is what
        // leaves `..` classifiable.
        assert_eq!(extract_token("cd .. now", 3).as_deref(), Some(".."));
    }

    #[test]
    fn token_extraction_fails_closed_on_whitespace_and_overrun() {
        assert_eq!(extract_token("a  b", 1), None);
        assert_eq!(extract_token("abc", 99), None);
        assert_eq!(extract_token("", 0), None);
    }

    #[test]
    fn token_extraction_spans_the_whole_run_from_any_offset() {
        for i in 0.."/home/fenrir/.tmux/tools/seek.rs".chars().count() {
            assert_eq!(
                extract_token("/home/fenrir/.tmux/tools/seek.rs", i).as_deref(),
                Some("/home/fenrir/.tmux/tools/seek.rs"),
                "from offset {i}"
            );
        }
    }

    #[test]
    fn cell_column_maps_through_display_width_not_char_count() {
        // Each CJK char is two cells, so cell 6 is the 4th char (index 3).
        // Width 80 keeps everything on row 0, isolating the column mapping.
        let line = "中文中文 tail";
        let at = |col| wrapped_char_index(line, 80, 0, col);
        assert_eq!(at(0), 0);
        assert_eq!(at(2), 1);
        assert_eq!(at(6), 3);
        // Landing inside a wide char resolves to that char, not past it.
        assert_eq!(at(5), 2);
        assert_eq!(at(9), 5);
        // Past the end clamps instead of panicking.
        assert_eq!(at(9999), line.chars().count());
        assert_eq!(wrapped_char_index("", 80, 0, 4), 0);
    }

    #[test]
    fn cjk_token_extraction_lands_on_the_right_token() {
        let line = "中文中文 main.rs";
        let idx = wrapped_char_index(line, 80, 0, 9); // first cell of "main.rs"
        assert_eq!(extract_token(line, idx).as_deref(), Some("main.rs"));
    }

    #[test]
    fn row_mapping_matches_the_measured_layout() {
        // Measured on tmux 3.5a in a 40-column pane, 2026-08-09.
        let lines: Vec<String> = [
            "AAA",
            "/home/fenrir/verylongpath/that/wraps/across/rows/main.rs:42", // 59 cells -> 2 rows
            "BBB",
            "中文中文中文中文中文中文中文中文中文中文中文中文 tail",      // 53 cells -> 2 rows
            "CCC",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(locate_row(&lines, 40, 0), Some((0, 0)));
        assert_eq!(locate_row(&lines, 40, 1), Some((1, 0)));
        assert_eq!(locate_row(&lines, 40, 2), Some((1, 1))); // continuation row
        assert_eq!(locate_row(&lines, 40, 3), Some((2, 0)));
        assert_eq!(locate_row(&lines, 40, 4), Some((3, 0)));
        assert_eq!(locate_row(&lines, 40, 5), Some((3, 1))); // CJK continuation
        assert_eq!(locate_row(&lines, 40, 6), Some((4, 0)));
        assert_eq!(locate_row(&lines, 40, 99), None);
    }

    #[test]
    fn wide_chars_straddling_a_row_boundary_are_counted_as_wrapped() {
        // Measured on tmux 3.5a, 41-column pane: 41 × 中 renders as raw rows of
        // 20 + 20 + 1 characters, i.e. 3 rows. tmux will not split a
        // double-width cell, so the 41st column of rows 1-2 is wasted — which
        // `ceil(82 / 41) = 2` cannot see.
        let line: String = "中".repeat(41);
        assert_eq!(rows_for(&line, 41), 3);
        // Even widths are not immune: "A" + 39×中 + "B" is 80 cells but the
        // trailing B cannot fit beside the last 中.
        let mixed = format!("A{}B", "中".repeat(39));
        assert_eq!(rows_for(&mixed, 40), 3);
        // A following line must therefore not be shifted up.
        let lines = vec![line, "MARKER".to_string()];
        assert_eq!(locate_row(&lines, 41, 3), Some((1, 0)));
    }

    #[test]
    fn wrapped_char_index_follows_the_same_wrap_rule_as_rows_for() {
        // 40-column pane, a 59-char path: row 1 col 9 is the 'm' of "main".
        let line = "/home/fenrir/verylongpath/that/wraps/across/rows/main.rs:42";
        let idx = wrapped_char_index(line, 40, 1, 9);
        assert_eq!(extract_token(line, idx).as_deref(), Some(line));
        assert_eq!(line.chars().nth(idx), Some('m'));
        // Row 0 col 0 is the leading slash.
        assert_eq!(wrapped_char_index(line, 40, 0, 0), 0);
        // Past the end clamps rather than panicking.
        assert_eq!(wrapped_char_index(line, 40, 99, 0), line.chars().count());
    }

    #[test]
    fn line_and_query_grain_never_route_to_ref() {
        // The contract: line grain always uses --paste. A slash in the line is
        // not a file reference.
        let line = "cargo build --release && ./target/foo";
        assert_eq!(route_for(Deliver::Line, line), Route::Paste);
        assert_eq!(route_for(Deliver::Query, "src/main.rs and"), Route::Paste);
        // A token still classifies normally.
        assert_eq!(route_for(Deliver::Token, "src/main.rs"), Route::Ref);
        assert_eq!(route_for(Deliver::Token, "Makefile"), Route::Paste);
    }

    #[test]
    fn a_line_exactly_pane_width_wide_does_not_swallow_the_next_row() {
        // The case the "this row is full, so the next one wrapped" heuristic
        // gets wrong: 40 cells fills its row without wrapping.
        let lines: Vec<String> = ["x".repeat(40), "next".into()]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(locate_row(&lines, 40, 0), Some((0, 0)));
        assert_eq!(locate_row(&lines, 40, 1), Some((1, 0)));
    }

    #[test]
    fn blank_lines_still_occupy_one_row() {
        let lines: Vec<String> = ["a".into(), "".into(), "b".into()]
            .iter()
            .map(|s: &String| s.to_string())
            .collect();
        assert_eq!(locate_row(&lines, 40, 1), Some((1, 0)));
        assert_eq!(locate_row(&lines, 40, 2), Some((2, 0)));
    }

    #[test]
    fn grain_line_never_delivers_the_query() {
        // The [T8] correction: t5's rule was unscoped by key, which collapsed
        // line grain into word grain for any multi-token query.
        assert_eq!(
            choose_grain(Grain::Line, true, "a b", true),
            Deliver::Line
        );
        assert_eq!(choose_grain(Grain::Line, false, "", false), Deliver::Line);
    }

    #[test]
    fn grain_word_delivers_the_query_only_for_a_live_multi_token_match() {
        assert_eq!(choose_grain(Grain::Word, true, "a b", true), Deliver::Query);
        // Single token -> the containing word, not the query.
        assert_eq!(choose_grain(Grain::Word, true, "alpha", true), Deliver::Token);
        // No live search -> uniform grabber.
        assert_eq!(choose_grain(Grain::Word, false, "a b", true), Deliver::Token);
        // Cursor moved off the match -> grab what the cursor is on [T5].
        assert_eq!(choose_grain(Grain::Word, true, "a b", false), Deliver::Token);
        // A whitespace-only query trims to empty and falls through.
        assert_eq!(choose_grain(Grain::Word, true, "   ", true), Deliver::Token);
    }

    #[test]
    fn cursor_on_match_tracks_the_landing_position() {
        let line = "the alpha bravo end";
        let idx = 4; // start of "alpha bravo"
        assert!(cursor_on_match(line, idx, "alpha bravo"));
        assert!(cursor_on_match(line, idx, "ALPHA BRAVO")); // search is case-insensitive
        assert!(cursor_on_match(line, idx, " alpha bravo ")); // query is trimmed
        assert!(!cursor_on_match(line, 8, "alpha bravo")); // cursor moved
        assert!(!cursor_on_match(line, idx, ""));
    }

    #[test]
    fn argument_parsing_rejects_unknown_words() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(parse_args(&a(&["word"])).unwrap(), (Grain::Word, false, None));
        assert_eq!(
            parse_args(&a(&["line", "--claude", "%41"])).unwrap(),
            (Grain::Line, true, Some("%41".into()))
        );
        assert_eq!(
            parse_args(&a(&["word", "%7"])).unwrap(),
            (Grain::Word, false, Some("%7".into()))
        );
        assert!(parse_args(&a(&["words"])).is_err());
        assert!(parse_args(&a(&["word", "--paste"])).is_err());
        assert!(parse_args(&a(&[])).is_err());
        // A second positional would silently retarget the grab.
        assert!(parse_args(&a(&["word", "%41", "%7"])).is_err());
    }
}
