#!/bin/bash
# t1 probe: does `tmux set-buffer` reach the Wayland clipboard under set-clipboard on?
#
# (a) does tmux EMIT OSC 52 on set-buffer?  -> capture a pty client's byte stream
# (b) does alacritty ACT on it?             -> write the real clipboard, then restore it
set -u
SP="$(dirname "$0")"

echo "########## (a) does tmux emit OSC 52 on set-buffer? ##########"
tmux kill-session -t osc52probe 2>/dev/null
tmux new-session -d -s osc52probe -x 80 -y 24 'sleep 60'
sleep 0.4
TERM=xterm-256color script -qfec "tmux attach -t osc52probe" "$SP/osc52-stream.txt" >/dev/null 2>&1 &
SPID=$!
sleep 1.2
tmux set-buffer -t osc52probe -- 'SENTINEL-EMIT-PROBE-7391'
sleep 0.8
kill $SPID 2>/dev/null
sleep 0.3
tmux kill-session -t osc52probe 2>/dev/null

if grep -qa $'\033]52;' "$SP/osc52-stream.txt"; then
  echo "RESULT(a): tmux DID emit OSC 52"
  grep -oa $'\033]52;[^\a]*' "$SP/osc52-stream.txt" | head -3 | cat -v
else
  echo "RESULT(a): NO OSC 52 in the client stream"
fi

echo
echo "########## (b) does the real Wayland clipboard change? ##########"
ORIG_FILE="$SP/clipboard-backup.bin"
wl-paste --no-newline > "$ORIG_FILE" 2>/dev/null
ORIG_BYTES=$(wc -c < "$ORIG_FILE")
echo "saved existing clipboard: ${ORIG_BYTES} bytes"

PROBE="SEEK-OSC52-PROBE-7391"
tmux set-buffer -- "$PROBE"
sleep 0.8
NOW="$(wl-paste --no-newline 2>/dev/null)"

if [ "$NOW" = "$PROBE" ]; then
  echo "RESULT(b): set-buffer DID reach the Wayland clipboard"
else
  echo "RESULT(b): set-buffer did NOT reach it — clipboard still holds something else"
  echo "           first 60 bytes now: [$(printf '%.60s' "$NOW")]"
fi

echo "top tmux buffer is: [$(tmux show-buffer 2>/dev/null | head -c 60)]"

# restore whatever was there before, byte for byte
if [ "$ORIG_BYTES" -gt 0 ]; then
  wl-copy < "$ORIG_FILE"
  sleep 0.3
  RESTORED="$(wl-paste --no-newline 2>/dev/null)"
  if [ "$RESTORED" = "$(cat "$ORIG_FILE")" ]; then
    echo "clipboard restored OK (${ORIG_BYTES} bytes)"
  else
    echo "WARNING: clipboard restore mismatch — backup kept at $ORIG_FILE"
  fi
else
  wl-copy --clear
  echo "clipboard was empty before; cleared back to empty"
fi
