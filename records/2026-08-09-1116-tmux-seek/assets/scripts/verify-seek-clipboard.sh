#!/usr/bin/env bash
# The one Verify-matrix assertion the headless/live suites left out: a real
# seek grab reaches the REAL Wayland clipboard and `wl-paste` returns the text.
#
# Previously marked manual-only because a test overwrites the user's clipboard.
# This rig closes that: the clipboard is saved first (only when it holds plain
# text — anything else aborts rather than lose it) and restored on exit.
#
# tmux side still uses the CC_TMUX_SOCKET throwaway-server seam; wl-copy is
# NOT stubbed — that is the point.
set -u

SOCK=seekclip
SEEK=/home/fenrir/.tmux/tools/target/release/seek
TOKEN=SEEKCLIPTOKEN42
PASS=0; FAIL=0

# --- save the real clipboard, abort if it is not plain text -----------------
if ! wl-paste --list-types 2>/dev/null | grep -q '^text/plain\|^UTF8_STRING\|^STRING\|^TEXT'; then
  echo "ABORT: clipboard does not hold plain text; refusing to overwrite it." >&2
  exit 2
fi
SAVED=$(wl-paste --no-newline 2>/dev/null)

restore() {
  printf '%s' "$SAVED" | wl-copy
  tmux -L "$SOCK" kill-server 2>/dev/null
}
trap restore EXIT

t() { tmux -L "$SOCK" "$@"; }

check() {
  if [ "$2" = "$3" ]; then printf '  PASS  %s\n' "$1"; PASS=$((PASS+1))
  else printf '  FAIL  %s\n        expected: [%s]\n        actual:   [%s]\n' "$1" "$2" "$3"; FAIL=$((FAIL+1)); fi
}

export CC_TMUX_SOCKET="$SOCK"
t kill-server 2>/dev/null; sleep 0.2
t -f /dev/null new-session -d -x 40 -y 14; sleep 0.5
P=$(t list-panes -F '#{pane_id}' | head -1)

t send-keys -t "$P" "clear; printf '%s\n' $TOKEN" Enter
sleep 0.8

t copy-mode -t "$P"
t send-keys -t "$P" -X top-line
t send-keys -t "$P" -X start-of-line
# cursor sits on the echoed token line? top-line row 0 is the printf command
# line; search the visible rows for the bare token instead.
ROW=$(t capture-pane -p -t "$P" -S 0 -E 13 | grep -n -m1 -x "$TOKEN" | cut -d: -f1 | awk '{print $1-1}')
for _ in $(seq 1 "$ROW"); do t send-keys -t "$P" -X cursor-down; done
t send-keys -t "$P" -X start-of-line

"$SEEK" word "$P" >/dev/null 2>&1

sleep 0.3   # wl-copy forks; give the serving child a beat
check "wl-paste returns the grabbed token" "$TOKEN" "$(wl-paste --no-newline 2>/dev/null)"

echo "  passed=$PASS failed=$FAIL"
[ "$FAIL" -eq 0 ]
