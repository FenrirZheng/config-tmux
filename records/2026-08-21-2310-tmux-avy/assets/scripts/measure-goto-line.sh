#!/bin/bash
# Measurement: goto-line coordinate space + resulting column, tmux 3.5a.
# 40x14 pane; print 30 numbered lines so ~16 lines are in history.
set -eu
SOCK=avymeasure
T() { tmux -L "$SOCK" "$@"; }
tmux -L "$SOCK" kill-server 2>/dev/null || true
T -f /dev/null new-session -d -x 40 -y 14
sleep 0.3
T send-keys -t 0 "clear; for i in \$(seq 0 29); do printf 'line-%02d filler\n' \$i; done" Enter
sleep 0.5
probe() { T display-message -p -t 0 'x=#{copy_cursor_x} y=#{copy_cursor_y} scroll=#{scroll_position}'; }
grab() { T send-keys -t 0 -X begin-selection; T send-keys -t 0 -X next-space-end; T send-keys -t 0 -X copy-selection; T show-buffer -b "$(T list-buffers -F '#{buffer_name}' | head -1)" 2>/dev/null || T show-buffer; }
for n in 0 5 29; do
  T copy-mode -t 0
  # move cursor to a nonzero column first so we can see if goto-line resets it
  T send-keys -t 0 -X -N 4 cursor-right
  T send-keys -t 0 -X goto-line "$n"
  echo "goto-line $n -> $(probe)"
  T send-keys -t 0 -X begin-selection
  T send-keys -t 0 -X end-of-line
  T send-keys -t 0 -X copy-selection
  echo "  line text from cursor: [$(T show-buffer)]"
  T send-keys -t 0 -X cancel
done
tmux -L "$SOCK" kill-server
