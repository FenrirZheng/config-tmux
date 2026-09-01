#!/usr/bin/env bash
# Width sweep for the sift footer guard. Throwaway -L server, never $TMUX.
set -u
SIFT=$HOME/.tmux/tools/target/release/sift
FIXTURE=$HOME/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/sift-fixture.sh
S=sift_widthsweep
OUT=${OUT:-/tmp/claude-1000/-home-fenrir--tmux/4fe3a1de-bb6d-4dc2-b6cc-78a34030775a/scratchpad/sweep-out}
mkdir -p "$OUT"

[ -n "${TMUX:-}" ] && case "$TMUX" in *"/$S"*) echo "refusing: \$TMUX is the probe socket"; exit 1;; esac
[ -x "$SIFT" ] || { echo "sift not built"; exit 1; }

trap 'tmux -L $S kill-server 2>/dev/null' EXIT

for W in 20 30 40 60 74 75 100 265; do
  tmux -L $S kill-server 2>/dev/null
  sleep 0.3
  tmux -L $S -f /dev/null new-session -d -x "$W" -y 20
  TARGET=$(tmux -L $S display-message -p '#{pane_id}')
  SOCK=$(tmux -L $S display-message -p '#{socket_path}')
  tmux -L $S send-keys -t "$TARGET" "bash '$FIXTURE'" Enter
  sleep 1.2
  tmux -L $S new-window -d -n sift "TMUX='$SOCK,0,0' '$SIFT' '$TARGET'"
  SPANE=$(tmux -L $S display-message -p -t sift '#{pane_id}')
  sleep 0.8
  tmux -L $S send-keys -t "$SPANE" -l 'aa'
  sleep 0.5
  tmux -L $S send-keys -t "$SPANE" M-1
  sleep 0.6
  # real geometry of the sift pane, as a control on -x
  GEOM=$(tmux -L $S display-message -p -t "$SPANE" '#{pane_width}x#{pane_height}')
  tmux -L $S capture-pane -p -t "$SPANE" > "$OUT/w$W.txt"
  tmux -L $S kill-server 2>/dev/null

  LINE1=$(sed -n '1p' "$OUT/w$W.txt")
  GOTOC=$(grep -c 'goto>' "$OUT/w$W.txt")
  # footer lines = non-empty screen lines from the ↑↓ line to the end
  FL=$(awk '/↑↓/{f=NR} END{}' "$OUT/w$W.txt" >/dev/null; \
       awk -v n=0 'BEGIN{f=0} /↑↓/{f=1} f&&NF{n++} END{print n}' "$OUT/w$W.txt")
  HDROK=$(printf '%s' "$LINE1" | grep -c '^goto> ')
  printf 'w=%-4s geom=%-8s hdrOK=%s gotoCount=%s footerLines=%s\n  line1=%s\n' \
     "$W" "$GEOM" "$HDROK" "$GOTOC" "$FL" "$LINE1"
done
