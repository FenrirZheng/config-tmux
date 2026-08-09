#!/bin/bash
# t1 probe v2 — which tmux operations emit OSC 52 under `set-clipboard on`?
#
# Layer A (emission): a scripted pty client captures the byte stream. TERM is
# xterm-256color, which DOES carry the Ms capability, so a negative result here
# is a real negative, not a capability artifact.
set -u
SP="$(dirname "$0")"

probe_emit () {                       # $1 = label, rest = commands to run
  local label="$1"; shift
  tmux kill-session -t osc52p2 2>/dev/null
  tmux new-session -d -s osc52p2 -x 80 -y 24 \
    "printf 'alpha bravo charlie\ndelta echo foxtrot\n'; sleep 60"
  sleep 0.4
  local stream="$SP/stream-$label.txt"
  TERM=xterm-256color script -qfec "tmux attach -t osc52p2" "$stream" >/dev/null 2>&1 &
  local spid=$!
  sleep 1.2
  "$@"                                 # the operation under test
  sleep 0.8
  kill $spid 2>/dev/null
  sleep 0.3
  tmux kill-session -t osc52p2 2>/dev/null

  if grep -qa $'\033]52;' "$stream"; then
    printf '%-34s EMITS OSC 52   ' "$label"
    grep -oa $'\033]52;[^\a]*' "$stream" | head -1 | cat -v | cut -c1-70
  else
    printf '%-34s no OSC 52\n' "$label"
  fi
}

op_set_buffer () {
  tmux set-buffer -t osc52p2 -- 'PROBE-SET-BUFFER'
}

op_copy_pipe () {                      # what copy-mode Y and DoubleClick do
  tmux copy-mode -t osc52p2
  tmux send-keys -t osc52p2 -X history-top
  tmux send-keys -t osc52p2 -X select-word
  sleep 0.3
  tmux send-keys -t osc52p2 -X copy-pipe-and-cancel
}

op_copy_selection () {
  tmux copy-mode -t osc52p2
  tmux send-keys -t osc52p2 -X history-top
  tmux send-keys -t osc52p2 -X begin-selection
  tmux send-keys -t osc52p2 -X -N 5 cursor-right
  sleep 0.3
  tmux send-keys -t osc52p2 -X copy-selection-and-cancel
}

echo "########## Layer A — which operations emit OSC 52? ##########"
probe_emit "set-buffer"                op_set_buffer
probe_emit "select-word + copy-pipe"   op_copy_pipe
probe_emit "copy-selection-and-cancel" op_copy_selection
