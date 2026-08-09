#!/usr/bin/env bash
# Headless portion of the seek Verify matrix.
#
# What this CAN cover: grabber mode (search_present=0) with deterministic cursor
# positioning -- token extraction, line grain, wrapping, CJK, the `--` guard,
# fail-closed, repeat grabs, and the out-of-copy-mode exit.
#
# What it CANNOT cover, and why: anything needing search_present=1. A copy-mode
# search is driven from the *client's* prompt, and there is no scriptable way to
# type into a client prompt -- six rigs have now confirmed this. Those rows, plus
# the Wayland clipboard (which needs a visible pane), stay keyboard-only.
#
# Uses tmuxlib's CC_TMUX_SOCKET seam so every tmux call seek makes goes to a
# throwaway server; wl-copy is shadowed by a stub so the real clipboard is never
# touched.
set -u

SOCK=seekverify
SEEK=/home/fenrir/.tmux/tools/target/release/seek
STUB=$(mktemp -d)
PASS=0; FAIL=0

printf '#!/bin/sh\nexit 0\n' > "$STUB/wl-copy"; chmod +x "$STUB/wl-copy"
export PATH="$STUB:$PATH" CC_TMUX_SOCKET="$SOCK"

t() { tmux -L "$SOCK" "$@"; }
cleanup() { t kill-server 2>/dev/null; rm -rf "$STUB"; }
trap cleanup EXIT

check() {
  if [ "$2" = "$3" ]; then printf '  PASS  %s\n' "$1"; PASS=$((PASS+1))
  else printf '  FAIL  %s\n        expected: [%s]\n        actual:   [%s]\n' "$1" "$2" "$3"; FAIL=$((FAIL+1)); fi
}

t kill-server 2>/dev/null; sleep 0.2
t -f /dev/null new-session -d -x 40 -y 14; sleep 0.5
P=$(t list-panes -F '#{pane_id}' | head -1)

t send-keys -t "$P" 'clear; printf "%s\n" MARKA /home/fenrir/verylong/path/that/wraps/rows/main.rs:42 MARKB "中文中文中文中文中文中文中文中文中文中文中文中文 tail" MARKC "flag --release here" "   spaced   " MARKD' Enter
sleep 1.0

# screen row of a raw (unjoined) row matching $1
row_of() { t capture-pane -p -t "$P" -S 0 -E 13 | grep -n -m1 -F "$1" | cut -d: -f1 | awk '{print $1-1}'; }

# put the copy-mode cursor at screen row $1, column $2
goto() {
  t send-keys -t "$P" -X top-line
  for _ in $(seq 1 "$1"); do t send-keys -t "$P" -X cursor-down; done
  t send-keys -t "$P" -X start-of-line
  for _ in $(seq 1 "$2"); do t send-keys -t "$P" -X cursor-right; done
}

grab() { # grain row col [--claude]
  local grain="$1" row="$2" col="$3"; shift 3
  t send-keys -t "$P" -X cancel 2>/dev/null
  t copy-mode -t "$P"; goto "$row" "$col"
  t set-buffer -- "__EMPTY__"
  "$SEEK" "$grain" "$@" "$P" >/dev/null 2>&1
  t show-buffer 2>/dev/null
}

echo "== headless Verify matrix (grabber mode) =="

R=$(row_of "verylong"); check "wrapped path, from its first row" \
  "/home/fenrir/verylong/path/that/wraps/rows/main.rs:42" "$(grab word "$R" 3)"

check "wrapped path, from its CONTINUATION row" \
  "/home/fenrir/verylong/path/that/wraps/rows/main.rs:42" "$(grab word "$((R+1))" 3)"

R=$(row_of "中文"); check "CJK token (width-aware column)" \
  "中文中文中文中文中文中文中文中文中文中文中文中文" "$(grab word "$R" 4)"

# NOTE: no "second token on a continuation row" assertion -- `start-of-line`
# jumps to the start of the LOGICAL line, so the rig cannot position a column
# inside a continuation row. The continuation-row path itself is covered above.

R=$(row_of "release"); check "leading-dash token survives set-buffer --" \
  "--release" "$(grab word "$R" 6)"

check "line grain trims both ends [T4]" "flag --release here" "$(grab line "$R" 6)"

R=$(row_of "spaced"); check "line grain on a padded line" "spaced" "$(grab line "$R" 4)"
check "fail-closed: cursor on whitespace" "__EMPTY__" "$(grab word "$R" 0)"

# repeat grab: two grabs without leaving copy-mode (delivery must NOT cancel)
R=$(row_of "MARKC")
t send-keys -t "$P" -X cancel 2>/dev/null; t copy-mode -t "$P"; goto "$R" 2
t set-buffer -- "__EMPTY__"
"$SEEK" word "$P" >/dev/null 2>&1
FIRST=$(t show-buffer)
STILL=$(t display-message -p -t "$P" '#{pane_in_mode}')
t set-buffer -- "__EMPTY__"
"$SEEK" word "$P" >/dev/null 2>&1
SECOND=$(t show-buffer)
check "repeat grab #1"                     "MARKC" "$FIRST"
check "still in copy-mode after delivery"  "1"     "$STILL"
check "repeat grab #2 (no re-entry)"       "MARKC" "$SECOND"

# out of copy-mode -> silent, buffer untouched
t send-keys -t "$P" -X cancel 2>/dev/null; sleep 0.2
t set-buffer -- "__EMPTY__"
"$SEEK" word "$P" >/dev/null 2>&1
check "silent when the pane left copy-mode" "__EMPTY__" "$(t show-buffer)"

echo
echo "  passed=$PASS failed=$FAIL"
[ "$FAIL" -eq 0 ]
