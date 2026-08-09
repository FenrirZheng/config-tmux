#!/bin/bash
# t2 probes — three measurements the edge-case questions need.
#   A. is search-backward-incremental REGEX or PLAIN TEXT?
#   B. what does an empty query + Enter do?
#   C. what does select-word select when the match spans two words?
set -u
SP="$(dirname "$0")"

start_session () {                      # $1 = content printf string
  tmux kill-session -t t2p 2>/dev/null
  tmux new-session -d -s t2p -x 80 -y 24 "printf '$1'; sleep 90"
  sleep 0.5
}
open_client () {
  rm -f "$SP/kb2"; mkfifo "$SP/kb2"
  # the fifo MUST be script's stdin — without a reader, `exec 9>` blocks forever
  TERM=xterm-256color script -qfec "tmux attach -t t2p" "$SP/t2-out.txt" \
    < "$SP/kb2" >/dev/null 2>&1 &
  CPID=$!
  exec 9>"$SP/kb2"
  sleep 1.2
}
close_client () {
  exec 9>&- 2>/dev/null
  kill $CPID 2>/dev/null
  tmux kill-session -t t2p 2>/dev/null
  rm -f "$SP/kb2"
}

echo "########## A. regex or plain text? ##########"
# content has "axb" but NO literal "a.b".  Query "a.b":
#   regex      -> cursor lands on the axb line
#   plain text -> no match, cursor does not move
start_session 'filler one\nzzz axb zzz\nfiller three\n'
open_client
tmux display -p -t t2p 'before : y=#{copy_cursor_y}' 2>/dev/null
tmux copy-mode -t t2p
BEFORE=$(tmux display -p -t t2p '#{copy_cursor_y}')
printf '\022' >&9          # C-r  = search-backward-incremental prompt
sleep 0.4
printf 'a.b' >&9
sleep 0.7
AFTER=$(tmux display -p -t t2p '#{copy_cursor_y}')
LINE=$(tmux display -p -t t2p '#{copy_cursor_line}')
echo "  query 'a.b' : y ${BEFORE} -> ${AFTER}   line=[${LINE}]"
if [ "$BEFORE" != "$AFTER" ]; then echo "  RESULT(A): REGEX — the dot matched 'x'"; else echo "  RESULT(A): PLAIN TEXT — no match for 'a.b'"; fi
printf '\033' >&9; sleep 0.3
close_client

echo
echo "########## B. empty query + Enter ##########"
start_session 'alpha bravo\ncharlie delta\n'
open_client
tmux copy-mode -t t2p
tmux send-keys -t t2p -X history-top
tmux send-keys -t t2p -X -N 3 cursor-right
B4=$(tmux display -p -t t2p 'x=#{copy_cursor_x} y=#{copy_cursor_y} mode=#{pane_mode} q=[#{pane_search_string}]')
echo "  before : $B4"
printf '\022' >&9          # open the prompt
sleep 0.4
printf '\r'  >&9           # Enter with nothing typed
sleep 0.7
AF=$(tmux display -p -t t2p 'x=#{copy_cursor_x} y=#{copy_cursor_y} mode=[#{pane_mode}] q=[#{pane_search_string}]')
echo "  after  : $AF"
close_client

echo
echo "########## C. select-word on a match spanning two words ##########"
start_session 'alpha bravo charlie\n'
open_client
tmux copy-mode -t t2p
tmux send-keys -t t2p -X history-top
tmux send-keys -t t2p -X search-forward-text 'alpha bravo'
tmux display -p -t t2p '  after search (end+1): x=#{copy_cursor_x}'
tmux send-keys -t t2p -X -N 11 cursor-left      # walk back to the match start
tmux display -p -t t2p '  at match start      : x=#{copy_cursor_x} word=[#{copy_cursor_word}]'
tmux send-keys -t t2p -X select-word
sleep 0.3
tmux send-keys -t t2p -X copy-pipe-and-cancel
sleep 0.5
echo "  select-word selected: [$(tmux show-buffer 2>/dev/null | head -c 40)]"
close_client
