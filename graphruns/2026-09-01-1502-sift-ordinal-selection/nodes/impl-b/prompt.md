You are implementing the SECOND and final slice of ADR-0007 in a C++20 program: **ordinal
mode** itself. The first slice (the always-rendered ordinal column) is already on disk and
verified — build clean at 0 warnings, both suites still green at their baselines.

## Repo and build

Repo: `/home/fenrir/.tmux` (branch `main`, single worktree). File to edit:
`tools/sift/src/main.cpp` — and ONLY that file.

    cd /home/fenrir/.tmux/tools/sift
    cmake --preset release && cmake --build --preset release

Never `cargo` — this is the one C++ tool in the repo. The compile bar is the warning set
already in `CMakeLists.txt` (`-Wall -Wextra -Wpedantic -Wconversion -Wsign-conversion
-Wold-style-cast -Wshadow`) and it must stay at **zero warnings**. This slice adds
ordinal↔index conversions between `size_t`, `int` and parsed digits — exactly where
`-Wconversion`/`-Wsign-conversion` bite. Use explicit `static_cast` in the file's existing
style; never a C-style cast.

Note: a clang LSP in this environment reports bogus errors around line 208 (`concept`,
`chain`) because it is not configured for C++20. Ignore them — `cmake --build` is the
authority.

## Read first (in this order)

1. `docs/adr/0007-select-a-sift-match-by-typing-its-ordinal.org` — **the settled design.
   You are implementing it, not revisiting it.** Read "The interaction, as settled" and
   "Consequences" in full.
2. `git diff tools/sift/src/main.cpp` — the first slice you are building on.
3. `tools/sift/src/main.cpp` — `read_key()`, `enum class Key`, `struct Input`,
   `struct Ui`, `draw()`, `run_ui()`'s switch.
4. `records/2026-09-01-1330-sift-ordinal-selection/sift-ordinal-selection.org` — the map;
   read ticket **t2** and the closed ticket **t1**, whose Resolution measured the entry
   channel on this very binary.

## What is already measured, so you do not re-derive it (t1, closed)

Left `Alt-<digit>` arrives at sift as the byte pair `0x1b` then the digit — confirmed on
this C++ binary by both a physical keypress through alacritty and a synthesised control
(3/3). Today those bytes land in `read_key()`'s escape branch:
`read_byte(40)` returns the digit, `c1 != '[' && c1 != 'O'` is true, and it returns
`Key::None`. **That `c1` branch is the implementation hook.** The existing 40 ms window is
what separates it from a bare `Esc`; do not change that timing.

Right Alt is unavailable on this machine (`~/.config/keyd/default.conf:40` gives
`rightalt` to the fcitx5 IME toggle at the kernel level), which is why every user-facing
string must say **left** Alt.

## The behaviour, exactly as ADR-0007 settles it

**Entry.** Left `Alt-<digit>` enters ordinal mode and *is itself the first digit*.
Ignored — no mode entry, nothing buffered — when the digit is `0`, when the hit list is
empty, or when the pattern is invalid (`u.re_error` non-empty). None of those names a
candidate.

**The buffer and the selection are one object.**
- Bare digits extend the buffer. A keystroke that would push the ordinal past N is **not
  buffered** — the buffer and selection are left exactly as they were, and there is no
  error path to write. Worked example from t3: with N = 12, `Alt-1` then `2` must still
  reach ordinal 12 (1 → 12, both in range); with N = 12, `Alt-1` then `5` leaves the
  buffer at `1`.
- The selection follows the buffer live: ordinal `k` means `u.sel = k - 1`. `Enter` is
  therefore a confirmation of something already on screen.
- **Invariant: `goto>` always names where the cursor actually is.** ADR-0007 states this
  for `↑↓`. Apply it uniformly to *every* key that moves the selection — `↑`/`↓`,
  `C-p`/`C-n`, `PgUp`/`PgDn`, `Home`/`End` — each moves the selection and rewrites the
  buffer to the new selection's ordinal. (The ADR names only `↑↓`; extending it to the
  other movers is the only reading under which the stated invariant holds. Say in your
  result block that you did this, so it can be recorded.)
- `Backspace` pops one digit; popping the **last** one leaves the mode. A second
  `Backspace` is the one that starts deleting the pattern — i.e. leaving the mode and
  deleting a pattern character are never the same keystroke.
- A **non-digit printable** character leaves the mode and lands in the pattern (the
  character is not swallowed).
- `Esc` leaves the **mode**, not sift. Outside the mode `Esc` still cancels sift, exactly
  as today.
- `Enter` jumps and exits, **unchanged**. ADR-0005's hand-back to tmux's own
  `search-backward` is untouched; do not go near `jump()`.

**Why this is safe**: every exit from the mode happens *before* the pattern can change, so
`refilter` can never run while the mode is live and the ordinals are frozen for the whole
life of the buffer. Preserve that property — it is load-bearing, not incidental.

**Rendering.**
- The prompt reads `goto> ` instead of `regex> ` while the mode is live, so the mode is
  never invisible. `draw()` currently hardcodes `const int plen = 7 + utf8_chars(...)`
  because `"regex> "` is 7 characters; `"goto> "` is 6. Recompute `plen` from whichever
  prompt is live, and keep the real cursor parked at the end of what is being typed — the
  buffer in ordinal mode, the pattern otherwise. A stale `plen` puts the cursor in the
  wrong cell, which is a visible bug, not a cosmetic one.
- The first slice renders the ordinal column in cyan (`kCyan`/`kDefaultFg`) while the line
  number stays dim. Keep that colour as the mode's visual anchor.
- **The footer must gain the new key, and must say left Alt.** ADR-0007's Consequences
  already commit to this ("the footer and runbook have to say 'left Alt'"), so it is
  settled, not yours to decide. Today's footer is
  `↑↓ select  Enter jump  Esc cancel  C-w word  C-u clear`. Add the minimum that is not
  misleading; do not invent an additional in-popup mode indicator beyond the `goto>`
  prompt — the map deliberately leaves that question open ("inventing one before the
  feature has been lived with is guessing"). Report your exact final footer string.

**Out of scope, explicitly**: `sift rows` gains nothing (the ordinal is just the output
row's position, so the headless seam has nothing to assert and nothing to break); the
ordinal column's jittering width stays as it is; `jump()`, `refilter()`, `render_line()`
and `origin_pane()` are not to be touched beyond what the above forces.

## Prove it

1. Build clean, **zero** warnings. Report the exact count.
2. Run BOTH suites; they must stay at their baselines — **jump 13/0** and **live 6/0**.
   They do not yet cover ordinal mode (a later node adds that), so what you are proving
   here is that you broke nothing.

       bash /home/fenrir/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-jump.sh
       bash /home/fenrir/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-live.sh

3. Drive the real behaviour yourself on a **throwaway** tmux server and paste captured
   evidence, not assertions. Copy the harness pattern from
   `records/2026-09-01-1330-sift-ordinal-selection/assets/scripts/probe-alt-digit-cpp.sh`
   (a `-L` socket; `send-keys -t <pane> M-1` is the synthesised Alt-digit, already proven
   equivalent to the physical key). At minimum capture: entry (`goto> 1`), a second digit
   extending it, an out-of-range digit refused, `↑` rewriting the buffer, `Backspace`
   popping out of the mode, a non-digit falling through into the pattern, and `Enter`
   landing the TARGET pane on the right occurrence.

## Hard prohibitions

- Never `git add`, `git commit`, `git checkout`, or `git stash`. Leave the work as
  uncommitted working-tree changes; a later gate handles committing.
- Never touch `$TMUX` or the user's real tmux server. Throwaway `-L <name>` sockets only,
  and kill them when done.
- Never `cargo` anything in `tools/sift`.
- Edit no file but `tools/sift/src/main.cpp`. In particular do **not** edit the
  verification suites, the runbook, the atlas node, or the map — other nodes own those.
- Do not use a compiler invocation as a syntax check that drops a `.o`/`.gch` beside the
  source; build only through the cmake preset, which writes into `tools/target/`.
- At most 6 cmake invocations.

## Output contract

Your deliverable is the edited source. **On failure, write NO artifact** — repair the file
with a targeted rewrite (never `git checkout`) and report the failure in the result block
only. **Return a terminal result — do not background any self-check and do not end your
turn waiting on one.**

End with exactly this fenced block:

```result
buildClean: true|false
warnings: <int>
jumpPass: <int>
jumpFail: <int>
livePass: <int>
liveFail: <int>
files: [tools/sift/src/main.cpp]
footerText: <the exact final footer string>
keyContract:
  entry: <how Alt-digit is decoded and where; the new Key enumerator name if any>
  modeState: <the Ui field(s) holding the buffer, and how "in mode" is decided>
  extend: <in-range and out-of-range behaviour>
  movers: <which keys rewrite the buffer>
  backspace: <pop, and the last-pop exit>
  fallthrough: <non-digit printable>
  esc: <in-mode vs out-of-mode>
  enter: <confirm unchanged>
  ignored: <Alt-0 / empty list / invalid pattern>
evidence: |
  <captured popup/target-pane output for each of the 7 behaviours above>
notes: <anything verify-impl, tests, docs or the atlas node must know; "none" if nothing>
```
