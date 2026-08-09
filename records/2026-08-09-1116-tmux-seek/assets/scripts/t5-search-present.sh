#!/bin/bash
# t5 probe — is #{search_present} the liveness signal @seek_origin was invented to fake?
# Measured across every state the redesign has to tell apart.
set -u
SP="$(dirname "$0")"
S=t5p

boot () {
  tmux kill-session -t $S 2>/dev/null
  tmux new-session -d -s $S -x 80 -y 24 \
    "printf 'filler one\nzzz axb zzz\nfiller three\n'; sleep 90"
  sleep 0.5
  rm -f "$SP/kb6"; mkfifo "$SP/kb6"
  TERM=xterm-256color script -qfec "tmux attach -t $S" "$SP/t5-out.txt" \
    < "$SP/kb6" >/dev/null 2>&1 &
  CPID=$!
  exec 5>"$SP/kb6"
  sleep 1.3
}
teardown () {
  exec 5>&- 2>/dev/null; kill $CPID 2>/dev/null; sleep 0.3
  tmux kill-session -t $S 2>/dev/null; rm -f "$SP/kb6"
}
show () { printf '  %-42s search_present=%s  q=[%s]  y=%s\n' "$1" \
  "$(tmux display -p -t $S '#{search_present}')" \
  "$(tmux display -p -t $S '#{pane_search_string}')" \
  "$(tmux display -p -t $S '#{copy_cursor_y}')"; }

boot
tmux copy-mode -t $S
show "1. fresh copy-mode, never searched"

printf '\022' >&5; sleep 0.4; printf 'axb' >&5; sleep 0.6; printf '\r' >&5; sleep 0.5
show "2. search 'axb' MATCHED, Enter"

# leave and re-enter copy-mode: does it reset?
tmux send-keys -t $S -X cancel; sleep 0.4
tmux copy-mode -t $S; sleep 0.3
show "3. left copy-mode, re-entered (stale q?)"

printf '\022' >&5; sleep 0.4; printf 'a.b' >&5; sleep 0.6; printf '\r' >&5; sleep 0.5
show "4. search 'a.b' NO MATCH, Enter"

tmux send-keys -t $S -X cancel; sleep 0.4
tmux copy-mode -t $S; sleep 0.3
printf '\022' >&5; sleep 0.4; printf 'axb' >&5; sleep 0.6
printf '\033' >&5; sleep 0.6
show "5. search 'axb' matched, then ESCAPE"

tmux send-keys -t $S -X cancel; sleep 0.4
tmux copy-mode -t $S; sleep 0.3
printf '\022' >&5; sleep 0.4; printf '\r' >&5; sleep 0.5
show "6. opened prompt, typed NOTHING, Enter"

teardown
