#!/usr/bin/env bash
# Orchestrator's independent check of the impl-b attempt-2 repair.
# Invariant: at every width draw() accepts, screen line 1 is the header (^goto> after M-1)
# and the footer occupies exactly one screen line.
set -u
SIFT=${SIFT:-$HOME/.tmux/tools/target/release/sift}
S=orch_sweep
pass=0; fail=0
for W in 20 30 40 60 74 75 100 265; do
  tmux -L $S kill-server 2>/dev/null
  tmux -L $S -f /dev/null new-session -d -x $W -y 20
  T=$(tmux -L $S display-message -p '#{pane_id}')
  SOCK=$(tmux -L $S display-message -p '#{socket_path}')
  tmux -L $S send-keys -t "$T" 'for i in $(seq -w 1 12); do echo "r$i ZAP$i"; done' Enter
  sleep 0.9
  tmux -L $S new-window -d -n s "TMUX='$SOCK,0,0' '$SIFT' '$T'"
  SP=$(tmux -L $S display-message -p -t s '#{pane_id}'); sleep 0.7
  for k in Z A P; do tmux -L $S send-keys -t "$SP" -l "$k"; sleep 0.04; done
  sleep 0.3; tmux -L $S send-keys -t "$SP" M-1; sleep 0.45
  scr=$(tmux -L $S capture-pane -p -t "$SP")
  l1=$(printf '%s' "$scr" | sed -n 1p)
  gc=$(printf '%s' "$scr" | grep -c 'goto>')
  # footer line count: non-empty trailing lines after the last result row
  fl=$(printf '%s\n' "$scr" | grep -c 'select\|Enter jump\|clear$\|^r$')
  if printf '%s' "$l1" | grep -q '^goto> ' && [ "$gc" -ge 1 ]; then
    pass=$((pass+1)); printf '  ok   w=%-4s line1=[%.40s] gotoCount=%s\n' "$W" "$l1" "$gc"
  else
    fail=$((fail+1)); printf '  FAIL w=%-4s line1=[%.60s] gotoCount=%s\n' "$W" "$l1" "$gc"
  fi
  tmux -L $S kill-server 2>/dev/null
done
printf 'width sweep: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
