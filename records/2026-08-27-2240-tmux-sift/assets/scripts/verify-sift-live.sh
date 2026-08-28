#!/usr/bin/env bash
# verify-sift-live.sh — end-to-end: drive the real TUI with real keystrokes and
# assert the target pane ends up on the right match.
#
# verify-sift-jump.sh asserts the arithmetic by issuing the tmux commands by
# hand; that would still pass if sift's own jump() were wired up wrongly. Here
# sift runs in a second pane of a throwaway server, receives keys through
# send-keys, and the assertions read the TARGET pane. Also measures the filter
# cost against a full 100000-line scrollback.
set -u
SIFT=${SIFT:-$HOME/.tmux/tools/target/release/sift}
FIXTURE=${FIXTURE:-$(dirname "$0")/sift-fixture.sh}
S=sift_live
pass=0; fail=0
ok()   { pass=$((pass+1)); printf '  ok   %s\n' "$1"; }
bad()  { fail=$((fail+1)); printf '  FAIL %s\n     want: %s\n     got : %s\n' "$1" "$2" "$3"; }
check(){ [ "$2" = "$3" ] && ok "$1" || bad "$1" "$2" "$3"; }

[ -x "$SIFT" ] || { echo "sift not built at $SIFT"; exit 1; }
tmux -L $S kill-server 2>/dev/null
tmux -L $S -f /dev/null new-session -d -x 100 -y 30
trap 'tmux -L $S kill-server 2>/dev/null' EXIT
TARGET=$(tmux -L $S display-message -p '#{pane_id}')
SOCK=$(tmux -L $S display-message -p '#{socket_path}')
t() { tmux -L $S "$@"; }

t send-keys -t "$TARGET" "bash '$FIXTURE'" Enter
sleep 1.5

# sift runs in its own window of the same server, told to search $TARGET.
t new-window -d -n sift "TMUX='$SOCK,0,0' '$SIFT' '$TARGET'"
SPANE=$(t display-message -p -t sift '#{pane_id}')
sleep 0.8

type_keys() { for k in "$@"; do t send-keys -t "$SPANE" -l "$k"; sleep 0.05; done; }

echo "== 1. the popup renders and counts matches live =="
type_keys b b 1 5 0
sleep 0.5
screen=$(t capture-pane -p -t "$SPANE")
case "$screen" in
  *"1 matches"*|*"1 match"*) ok "header reports 1 match for bb150" ;;
  *) bad "header reports 1 match for bb150" "'1 matches' in header" "$(printf '%s' "$screen" | head -1)" ;;
esac

echo "== 2. Enter jumps the TARGET pane onto the match start =="
t send-keys -t "$SPANE" Enter
sleep 0.8
# bb150 sits at column 13 of "row150 aa150 bb150 cc150".
got=$(t display-message -p -t "$TARGET" '#{copy_cursor_x}|#{search_present}|#{pane_in_mode}')
check "target cursor on bb150, search registered, still in copy-mode" "13|1|1" "$got"

echo "== 3. the popup process exited on its own (display-popup -E would close) =="
t list-windows -F '#{window_name}' | grep -qx sift && \
  bad "sift window gone" "no sift window" "still present" || ok "sift exited after the jump"

echo "== 4. Esc cancels without touching the target =="
t send-keys -X -t "$TARGET" cancel
before=$(t display-message -p -t "$TARGET" '#{pane_in_mode}')
t new-window -d -n sift2 "TMUX='$SOCK,0,0' '$SIFT' '$TARGET'"
S2=$(t display-message -p -t sift2 '#{pane_id}')
sleep 0.8
for k in b b 1 5 0; do t send-keys -t "$S2" -l "$k"; sleep 0.05; done
t send-keys -t "$S2" Escape
sleep 0.6
check "target untouched by Esc (pane_in_mode)" "$before" \
      "$(t display-message -p -t "$TARGET" '#{pane_in_mode}')"

echo "== 5. filter cost against a full 100000-line scrollback =="
t set-option -g history-limit 100000 >/dev/null
t new-window -d -n big
BIG=$(t display-message -p -t big '#{pane_id}')
t send-keys -t "$BIG" 'seq 1 100000 | sed "s/^/line /"' Enter
sleep 8
hs=$(t display-message -p -t "$BIG" '#{history_size}')
echo "  history_size=$hs"
start=$(date +%s%N)
n=$(TMUX="$SOCK,0,0" "$SIFT" rows "$BIG" 'line 9[0-9]{4}' | wc -l)
ms=$(( ($(date +%s%N) - start) / 1000000 ))
echo "  matched $n lines in ${ms}ms"
[ "$ms" -lt 300 ] && ok "one filter pass under 300ms (${ms}ms)" \
                  || bad "one filter pass under 300ms" "<300ms" "${ms}ms"

echo "== 6. the REAL binding, driven by a real prefix keypress =="
# The regression test for 2026-08-28: tests 1-4 launch sift in a window with the
# pane id already substituted by the shell, so they never exercise the binding.
# `display-popup` does NOT format-expand its shell-command, so a binding passing
# `#{pane_id}` handed sift a literal string and the popup flashed shut — invisible
# to every assertion above. This drives claude.conf's actual binding through a
# nested client, the only way a real `prefix /` keystroke can be delivered.
#
# What it detects is the flash-and-close failure mode in general, NOT that one
# cause: sift's origin_pane() now self-heals a literal `#{pane_id}`, so
# reintroducing the old binding alone no longer reddens this. Verified to have
# teeth by pointing the binding at a nonexistent pane (`sift %99999`), which
# reproduces the same user-visible symptom and does fail here.
O=sift_live_outer
tmux -L $O kill-server 2>/dev/null
tmux -L $O -f /dev/null new-session -d -x 282 -y 71
OP=$(tmux -L $O display-message -p '#{pane_id}')
tmux -L $O send-keys -t "$OP" "tmux -L $S attach" Enter
sleep 1.5
# Load the real config so the binding under test is the shipped one.
t source-file "$HOME/.tmux/claude.conf" 2>/dev/null
tmux -L $O send-keys -t "$OP" C-b; sleep 0.3
tmux -L $O send-keys -t "$OP" /;   sleep 1.5
screen=$(tmux -L $O capture-pane -p -t "$OP")
case "$screen" in
  *"regex>"*) ok "prefix / opens sift and it renders its prompt" ;;
  *) bad "prefix / opens sift and it renders its prompt" "'regex>' on screen" \
         "popup absent or closed immediately" ;;
esac
tmux -L $O send-keys -t "$OP" Escape; sleep 0.4
tmux -L $O kill-server 2>/dev/null

echo
printf 'passed %d, failed %d\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
