#!/bin/bash
# Follow-up: exact cursor-right step arithmetic across a wrap, tmux 3.5a.
# Row 0 holds chars 0..39 of an 80-char logical line; row 1 holds chars 40..79.
set -eu
SOCK=avymeasure
T() { tmux -L "$SOCK" "$@"; }
tmux -L "$SOCK" kill-server 2>/dev/null || true
T -f /dev/null new-session -d -x 40 -y 14
sleep 0.3
T send-keys -t 0 "clear; printf '%s\n' '0123456789012345678901234567890123456789ABCDEFGHIJABCDEFGHIJABCDEFGHIJABCDEFGHIJ'" Enter
sleep 0.4
probe() { T display-message -p -t 0 '#{copy_cursor_x} #{copy_cursor_y}'; }
for k in 38 39 40 41 42; do
  T copy-mode -t 0
  T send-keys -t 0 -X top-line
  T send-keys -t 0 -X start-of-line
  T send-keys -t 0 -X -N "$k" cursor-right
  echo "N=$k -> $(probe)"
  T send-keys -t 0 -X cancel
done
tmux -L "$SOCK" kill-server
