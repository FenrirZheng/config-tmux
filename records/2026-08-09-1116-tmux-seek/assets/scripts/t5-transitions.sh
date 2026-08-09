#!/bin/bash
# t5 probe 2 — WITHIN one copy-mode session, does search_present track the latest
# search, or latch on the first match?  If it latches, pane_search_string and
# search_present can disagree and seek would cursor-left by a wrong length.
set -u
SP="$(dirname "$0")"
S=t5t

tmux kill-session -t $S 2>/dev/null
tmux new-session -d -s $S -x 80 -y 24 \
  "printf 'filler one\nzzz axb zzz\nfiller three\n'; sleep 90"
sleep 0.5
rm -f "$SP/kb7"; mkfifo "$SP/kb7"
TERM=xterm-256color script -qfec "tmux attach -t $S" "$SP/t5t-out.txt" \
  < "$SP/kb7" >/dev/null 2>&1 &
CPID=$!
exec 4>"$SP/kb7"
sleep 1.3

show () { printf '  %-46s present=%s q=[%s] y=%s x=%s\n' "$1" \
  "$(tmux display -p -t $S '#{search_present}')" \
  "$(tmux display -p -t $S '#{pane_search_string}')" \
  "$(tmux display -p -t $S '#{copy_cursor_y}')" \
  "$(tmux display -p -t $S '#{copy_cursor_x}')"; }

tmux copy-mode -t $S
show "start"

printf '\022' >&4; sleep 0.4; printf 'axb' >&4; sleep 0.6; printf '\r' >&4; sleep 0.5
show "A. matched 'axb'"

# same copy-mode session: now search something that does NOT match
printf '\022' >&4; sleep 0.4; printf 'zzzz' >&4; sleep 0.7; printf '\r' >&4; sleep 0.5
show "B. then NO-MATCH 'zzzz' in the same session"

# and n / N after a match
printf '\022' >&4; sleep 0.4; printf 'filler' >&4; sleep 0.6; printf '\r' >&4; sleep 0.5
show "C. matched 'filler'"
tmux send-keys -t $S -X search-again; sleep 0.4
show "D. after search-again (n)"

exec 4>&- 2>/dev/null; kill $CPID 2>/dev/null; sleep 0.3
tmux kill-session -t $S 2>/dev/null; rm -f "$SP/kb7"
