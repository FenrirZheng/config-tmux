#!/bin/bash
# Measurement: copy-mode cursor primitives on wrapped rows, tmux 3.5a.
# Q1: on a wrapped continuation row, does start-of-line go to the screen row
#     start or the logical line start?
# Q2: does `send-keys -X -N k cursor-right` step per character (CJK = 1 step)?
# Q3: does cursor-right past end-of-row wrap to the next screen row?
set -eu
SOCK=avymeasure
T() { tmux -L "$SOCK" "$@"; }
tmux -L "$SOCK" kill-server 2>/dev/null || true
T -f /dev/null new-session -d -x 40 -y 14
sleep 0.3
# Line A: 80 chars 0123..., wraps into two 40-col rows.
# Line B: pure CJK line.
T send-keys -t 0 "clear; printf '%s\n' '0123456789012345678901234567890123456789ABCDEFGHIJABCDEFGHIJABCDEFGHIJABCDEFGHIJ' '中文字元寬度測試行'" Enter
sleep 0.4
probe() { T display-message -p -t 0 '#{copy_cursor_x} #{copy_cursor_y}'; }

echo "--- Q1: start-of-line on continuation row"
T copy-mode -t 0
T send-keys -t 0 -X top-line
T send-keys -t 0 -X cursor-down          # row 1 = continuation of the 80-char line
T send-keys -t 0 -X -N 5 cursor-right
echo "before start-of-line: $(probe)   (expect x=5 y=1)"
T send-keys -t 0 -X start-of-line
echo "after  start-of-line: $(probe)   (x=0 y=1 => screen-row; x=0 y=0 => logical)"
T send-keys -t 0 -X cancel

echo "--- Q2: cursor-right stepping over CJK (row 2)"
T copy-mode -t 0
T send-keys -t 0 -X top-line
T send-keys -t 0 -X -N 2 cursor-down     # CJK row
T send-keys -t 0 -X start-of-line
T send-keys -t 0 -X -N 3 cursor-right    # 3 steps over wide chars
echo "after 3x cursor-right on CJK row: $(probe)   (x=6 => per-char; x=3 => per-cell)"
T send-keys -t 0 -X cancel

echo "--- Q3: cursor-right past end of screen row"
T copy-mode -t 0
T send-keys -t 0 -X top-line
T send-keys -t 0 -X -N 45 cursor-right   # row 0 is 40 cols wide (wrapped line)
echo "after 45x cursor-right from row0 col0: $(probe)"
T send-keys -t 0 -X cancel
tmux -L "$SOCK" kill-server
