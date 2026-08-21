#!/bin/bash
# Follow-up: phantom-step count at an EARLY wrap (row breaks at 39 cells
# because the next wide char cannot fit), tmux 3.5a, 40-col pane.
# Line: 'x' + 25 CJK chars = 1 + 50 cells. Row 0: x + 19 CJK (39 cells,
# early wrap). Row 1: remaining 6 CJK. Char index of first char on row 1 = 20.
set -eu
SOCK=avymeasure
T() { tmux -L "$SOCK" "$@"; }
tmux -L "$SOCK" kill-server 2>/dev/null || true
T -f /dev/null new-session -d -x 40 -y 14
sleep 0.3
T send-keys -t 0 "clear; printf '%s\n' 'x中中中中中中中中中中中中中中中中中中中中中中中中中'" Enter
sleep 0.4
probe() { T display-message -p -t 0 '#{copy_cursor_x} #{copy_cursor_y}'; }
for k in 19 20 21 22; do
  T copy-mode -t 0
  T send-keys -t 0 -X top-line
  T send-keys -t 0 -X start-of-line
  T send-keys -t 0 -X -N "$k" cursor-right
  echo "N=$k -> $(probe)"
  T send-keys -t 0 -X cancel
done
tmux -L "$SOCK" kill-server
