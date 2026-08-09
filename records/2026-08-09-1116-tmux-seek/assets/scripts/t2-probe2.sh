#!/bin/bash
# t2 probe v2 — same as v1 but with a POSITIVE CONTROL, because "no match" and
# "the prompt never opened" produce identical observations.
set -u
SP="$(dirname "$0")"

run_query () {                          # $1 = label, $2 = query, $3 = keys after
  tmux kill-session -t t2q 2>/dev/null
  tmux new-session -d -s t2q -x 80 -y 24 \
    "printf 'filler one\nzzz axb zzz\nfiller three\n'; sleep 60"
  sleep 0.5
  rm -f "$SP/kb3"; mkfifo "$SP/kb3"
  TERM=xterm-256color script -qfec "tmux attach -t t2q" "$SP/t2b-out.txt" \
    < "$SP/kb3" >/dev/null 2>&1 &
  local cpid=$!
  exec 8>"$SP/kb3"
  sleep 1.2
  tmux copy-mode -t t2q
  local before; before=$(tmux display -p -t t2q '#{copy_cursor_y}')
  printf '\022' >&8                     # C-r : search-backward-incremental
  sleep 0.4
  [ -n "$2" ] && printf '%s' "$2" >&8
  sleep 0.7
  local after line q
  after=$(tmux display -p -t t2q '#{copy_cursor_y}')
  line=$(tmux display -p -t t2q '#{copy_cursor_line}')
  q=$(tmux display -p -t t2q '#{pane_search_string}')
  printf '%-28s y %s -> %-3s search_string=[%s] line=[%s]\n' "$1" "$before" "$after" "$q" "$line"
  [ -n "$3" ] && { printf '%b' "$3" >&8; sleep 0.7; \
    tmux display -p -t t2q "  after final key      : x=#{copy_cursor_x} y=#{copy_cursor_y} mode=[#{pane_mode}] q=[#{pane_search_string}]"; }
  printf '\033' >&8 2>/dev/null; sleep 0.2
  exec 8>&- 2>/dev/null
  kill $cpid 2>/dev/null
  sleep 0.3
  tmux kill-session -t t2q 2>/dev/null
  rm -f "$SP/kb3"
}

echo "content: line0='filler one'  line1='zzz axb zzz'  line2='filler three'"
echo
echo "### POSITIVE CONTROL — does the prompt work at all? ###"
run_query "control: query 'axb'" "axb" ""
echo
echo "### THE ACTUAL QUESTION — regex or plain text? ###"
run_query "test: query 'a.b'"    "a.b" ""
echo
echo "### B. empty query then Enter ###"
run_query "empty query + Enter"  ""    '\r'
