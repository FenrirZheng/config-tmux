#!/bin/bash
# Headless verification for avy (tmux 3.5a) — drives `avy ui` as the
# foreground process of a pane in a SEPARATE WINDOW of a throwaway server
# (CC_TMUX_SOCKET seam), feeding real keystrokes with send-keys and asserting
# on the target pane's copy-mode cursor. A separate window, not a split: a
# split would change the target pane's height and shift every expectation.
#
# The popup path (`avy launch`) needs an attached client and stays
# keyboard-only — verify it manually per runbooks/avy.md (same limitation
# class as seek's search_present rows).
#
# Timing: @avy-timeout defaults to 500ms; the 1.2s sleeps after a query let
# the timer fire (label phase or single-match jump).
set -eu

BIN="$HOME/.tmux/tools/target/release/avy"
[ -x "$BIN" ] || { echo "SKIP-AS-FAIL: $BIN not built"; exit 1; }

SOCK=avyverify
export CC_TMUX_SOCKET="$SOCK"
T() { tmux -L "$SOCK" "$@"; }
cleanup() { tmux -L "$SOCK" kill-server 2>/dev/null || true; }
trap cleanup EXIT

pass=0; fail=0
ok()  { pass=$((pass+1)); echo "ok   - $1"; }
bad() { fail=$((fail+1)); echo "FAIL - $1 (got: $2)"; }

probe() { T display-message -p -t %0 '#{copy_cursor_x},#{copy_cursor_y},#{pane_in_mode}'; }

# Fresh server: %0 = 40x14 target pane (window 0), %1 = ui runner (window 1).
setup() {
  cleanup
  T -f /dev/null new-session -d -x 40 -y 14
  T new-window -d
  sleep 0.3
}
type_content() { # print fixed lines into the target pane
  T send-keys -t %0 "clear; printf '%s\n' $1" Enter
  sleep 0.4
}
start_ui() {
  T send-keys -t %1 "exec $BIN ui %0" Enter
  sleep 0.4
}

# ── case 1: unique match jumps immediately (no label press needed) ──────────
setup
type_content "'alpha beta' 'gamma delta' 'omega'"
start_ui
T send-keys -t %1 -l 'omeg'
sleep 1.2
r=$(probe)
[ "$r" = "0,2,1" ] && ok "unique match jumps to line start" || bad "unique match" "$r"

# ── case 2: multiple matches -> labels; 's' picks the second match ──────────
setup
type_content "'foo one' 'foo two' 'foo three'"
start_ui
T send-keys -t %1 -l 'foo'
sleep 1.2   # labels now shown: a,s,d (default @avy-keys)
T send-keys -t %1 -l 's'
sleep 0.6
r=$(probe)
[ "$r" = "0,1,1" ] && ok "label 's' jumps to second match" || bad "label select" "$r"

# ── case 3: mid-line target column ──────────────────────────────────────────
setup
type_content "'find the needle here'"
start_ui
T send-keys -t %1 -l 'needle'
sleep 1.2
r=$(probe)
[ "$r" = "9,0,1" ] && ok "mid-line column (char 9)" || bad "mid-line column" "$r"

# ── case 4: CJK row — cursor lands on the right char (2 cells/char) ─────────
setup
type_content "'前面 目標 後面'"
start_ui
T send-keys -t %1 -l '目標'
sleep 1.2
r=$(probe)
# chars: 前(0)面(1)␣(2)目(3) → target char 3 sits at cell 2+2+1 = 5
[ "$r" = "5,0,1" ] && ok "CJK target (cell 5)" || bad "CJK target" "$r"

# ── case 5: wrapped logical line — target past the wrap boundary ────────────
setup
type_content "'0123456789012345678901234567890123456789NEEDLE-TAIL'"
start_ui
T send-keys -t %1 -l 'NEEDLE'
sleep 1.2
r=$(probe)
# NEEDLE starts at char 40 of the 51-char line = wrapped row 1, col 0
[ "$r" = "0,1,1" ] && ok "target beyond wrap boundary" || bad "wrap boundary" "$r"

# ── case 6: Escape cancels; pane never enters copy-mode ─────────────────────
setup
type_content "'alpha alto'"
start_ui
T send-keys -t %1 -l 'al'   # 2 matches: even if the timer fires we only reach labels
sleep 0.1
T send-keys -t %1 Escape
sleep 0.5
r=$(T display-message -p -t %0 '#{pane_in_mode}')
[ "$r" = "0" ] && ok "escape leaves pane out of copy-mode" || bad "escape cancel" "$r"

# ── case 7: pane already in copy-mode and scrolled ──────────────────────────
setup
T send-keys -t %0 "clear; for i in \$(seq 1 30); do echo line-\$i; done; echo MARKER" Enter
sleep 0.5
# Screen after printing: 32 rows total (30 lines + MARKER + prompt), 14 visible
# → viewport top at scroll 0 is line-19. scroll-up 5 → top is line-14, so
# line-20 sits on viewport row 6.
T copy-mode -t %0
T send-keys -t %0 -X -N 5 scroll-up
sleep 0.2
start_ui
T send-keys -t %1 -l 'line-20'
sleep 1.2
r=$(probe)
[ "$r" = "0,6,1" ] && ok "scrolled copy-mode viewport jump" || bad "scrolled viewport" "$r"

echo "----"
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
