You are writing the **behavioural specification** for an existing C++ program so that a
DIFFERENT engineer can reimplement it in Rust without ever reading the C++ source, and so
that a THIRD engineer can verify the reimplementation against your spec alone.

You are read-only. Do not create, edit, or delete any file outside your output path.

## Subject

`/home/fenrir/.tmux/tools/sift/src/main.cpp` (800 lines) and
`/home/fenrir/.tmux/tools/sift/CMakeLists.txt`. `sift` is a tmux popup regex-search tool
bound to `prefix /`. Supporting context you should read, in this order:

1. `/home/fenrir/.tmux/runbooks/sift.md` — the operational description and key table.
2. `/home/fenrir/.tmux/docs/adr/0005-own-the-interaction-loop-for-regex-search.org` — why
   it owns its interaction loop.
3. `/home/fenrir/.tmux/tools/atlas/sift.org` — the atlas node.
4. `/home/fenrir/.tmux/claude.conf` (the `prefix /` block, ~line 176-212) — how it is invoked.
5. The C++ source itself. This is the authority: where a document and the source
   disagree, the source wins and you say so explicitly in a "documented vs actual"
   note.

## What the spec must contain

Write it as markdown with these sections, in this order. Be exhaustive on the
platform-level details — they are exactly what a naive Rust port gets wrong.

1. **CLI surface** — every argv shape, what each does, every exit code, every stderr
   message verbatim, and the behaviour when run outside tmux or with a bad pane id.
2. **tmux interaction** — every tmux command the program issues, verbatim, with the
   format strings, in the order issued, and what it does with each result. Cover: pane
   resolution when no pane id is given, geometry query, scrollback capture (including
   the alternate-screen case), and the jump sequence (`copy-mode`, `history-top`,
   `goto-line`, `search-backward`/`search-forward`, cursor seating). Note which of these
   are load-bearing surprises — the source's comments call several out.
3. **Regex semantics** — the exact `regcomp` flags (`REG_EXTENDED`; note the absence of
   `REG_ICASE`), the use of `REG_NOTBOL` on continuation scans, that offsets are BYTE
   offsets, the empty-match advance rule, the per-line and total match caps
   (`kMatchCap`), and what a malformed pattern does. State the observable consequences
   of POSIX ERE semantics that would differ under a different regex engine: leftmost-
   longest alternation, backreference support, `{n,m}` and bracket-expression handling,
   and how anchors behave on continuation scans.
4. **Text handling** — UTF-8 decoding, the char-index vs terminal-cell-column
   distinction, `wcwidth`/`cell_width` behaviour including the control/combining/
   non-printable cases, the effect of `setlocale(LC_ALL, "")`, and how a line is
   truncated and the match highlighted at a given width.
5. **Terminal control** — the exact termios flags set and cleared for raw mode, the
   restore path (including on signals), `TIOCGWINSZ` and the SIGWINCH handling, the
   `poll()` timeout discipline, and every ANSI escape sequence the program emits, verbatim.
6. **Key map** — every key and escape sequence recognised, what each does, and the
   selection/paging arithmetic (initial selection, clamping, page size, wrap or no wrap).
7. **Rendering** — the exact header text in every state (including the alternate-screen
   warning), the row format, the highlight, and the no-match / empty-pattern states.
8. **Documented vs actual** — anything where the runbook, ADR, or atlas describes
   behaviour the source does not have, or vice versa.
9. **Port hazards** — a ranked list of the things a Rust reimplementation is most likely
   to get subtly wrong, each with the specific observable symptom.

## Output contract

Write the spec to `/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/nodes/spec/spec.md`.

Then end your reply with a fenced result block, exactly this shape:

```result
status: ok | failed
spec_path: <path or ->
sections_complete: <how many of the 9 sections you fully populated>
regex_flags: <the literal flags found>
tmux_commands: <count of distinct tmux commands documented>
notes: <one line; on failure, why>
```

On failure write NO artifact — report it in the result block only. Return a terminal
result: do not background any self-check and do not end your turn waiting on anything.
