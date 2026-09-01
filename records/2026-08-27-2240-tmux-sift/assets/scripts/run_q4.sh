#!/usr/bin/env bash
set -u
SOCK=$(tmux -L siftcheck display-message -p '#{socket_path}')
export TMUX="$SOCK,0,0"
BIN=/home/fenrir/.tmux/tools/target/release/sift
OUT=/tmp/claude-1000/-home-fenrir--tmux/c74280c4-fec1-4265-b56f-e0e428324cc0/scratchpad/q4_combined.txt

: > "$OUT"

P1='aa1[0-9][0-9]'
P2='中文測試'
P3='bb0(1|2)[0-9]'
P4='^row19[0-9] '

printf '### pattern: %s\n' "$P1" >> "$OUT"
"$BIN" rows %0 "$P1" < /dev/null >> "$OUT" 2>&1

printf '### pattern: %s\n' "$P2" >> "$OUT"
"$BIN" rows %0 "$P2" < /dev/null >> "$OUT" 2>&1

printf '### pattern: %s\n' "$P3" >> "$OUT"
"$BIN" rows %0 "$P3" < /dev/null >> "$OUT" 2>&1

printf '### pattern: %s\n' "$P4" >> "$OUT"
"$BIN" rows %0 "$P4" < /dev/null >> "$OUT" 2>&1

echo "DONE"
