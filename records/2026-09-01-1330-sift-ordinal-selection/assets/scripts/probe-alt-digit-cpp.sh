#!/usr/bin/env bash
# probe-alt-digit-cpp.sh — re-run the grill's SYNTHESISED Alt-digit control
# against the C++ sift binary (the 2026-09-01 revert of the Rust port).
#
# The grill (2026-09-01) measured this on the RUST binary and concluded
# "tmux delivers the pair as 0x1b 0x31, which read_key already decodes and
# discards". The substrate changed under that measurement, so it is re-run here.
# This is t1's CONTROL: without it, a physical-keypress failure cannot be told
# apart from a binary that mishandles ESC+digit.
#
# Throwaway server (-L), never $TMUX — same isolation as verify-sift-live.sh.
set -u
SIFT=${SIFT:-$HOME/.tmux/tools/target/release/sift}
FIXTURE=$HOME/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/sift-fixture.sh
S=sift_altprobe
pass=0; fail=0
ok()  { pass=$((pass+1)); printf '  ok   %s\n' "$1"; }
bad() { fail=$((fail+1)); printf '  FAIL %s\n     want: %s\n     got : %s\n' "$1" "$2" "$3"; }

# control: prove we are NOT driving the user's real server
[ -n "${TMUX:-}" ] && case "$TMUX" in *"/$S"*) echo "refusing: \$TMUX points at the probe socket"; exit 1;; esac

echo "== binary under test =="
file "$SIFT" | sed 's/^/  /'
[ -x "$SIFT" ] || { echo "sift not built at $SIFT"; exit 1; }

tmux -L $S kill-server 2>/dev/null
tmux -L $S -f /dev/null new-session -d -x 100 -y 30
trap 'tmux -L $S kill-server 2>/dev/null' EXIT
TARGET=$(tmux -L $S display-message -p '#{pane_id}')
SOCK=$(tmux -L $S display-message -p '#{socket_path}')
t() { tmux -L $S "$@"; }

t send-keys -t "$TARGET" "bash '$FIXTURE'" Enter
sleep 1.5
t new-window -d -n sift "TMUX='$SOCK,0,0' '$SIFT' '$TARGET'"
SPANE=$(t display-message -p -t sift '#{pane_id}')
sleep 0.8

echo "== 1. positive control: a plain letter reaches the pattern =="
t send-keys -t "$SPANE" -l e; sleep 0.3
scr=$(t capture-pane -p -t "$SPANE" | ug -m1 'regex>' || true)
case "$scr" in *"regex> e"*) ok "plain 'e' typed: $scr" ;; *) bad "plain 'e' typed" "regex> e" "$scr" ;; esac

echo "== 2. M-1 does not close the popup (ESC arrived paired, not bare) =="
t send-keys -t "$SPANE" M-1; sleep 0.5
if t list-windows -F '#{window_name}' | grep -qx sift; then
  ok "popup still open after M-1"
else
  bad "popup still open after M-1" "sift window present" "sift exited (bare Esc = cancel)"
fi

echo "== 3. the digit did not leak into the pattern =="
t send-keys -t "$SPANE" -l r; sleep 0.4
scr=$(t capture-pane -p -t "$SPANE" | ug -m1 'regex>' || true)
case "$scr" in
  *"regex> er"*) ok "prompt reads 'regex> er' — digit discarded: $scr" ;;
  *"regex> e1r"*) bad "digit discarded" "regex> er" "$scr (digit LEAKED)" ;;
  *) bad "digit discarded" "regex> er" "$scr" ;;
esac

echo
echo "passed $pass, failed $fail"
[ "$fail" -eq 0 ]
