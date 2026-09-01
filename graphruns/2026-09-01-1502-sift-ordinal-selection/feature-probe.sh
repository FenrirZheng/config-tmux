#!/usr/bin/env bash
# Orchestrator's mechanical check that the footer repair did not move ordinal mode.
# Runs the 7 settled behaviours at a given width. N=12 fixture.
set -u
SIFT=${SIFT:-$HOME/.tmux/tools/target/release/sift}
W=${1:-100}
S=orch_feat_$W
pass=0; fail=0
ck(){ if printf '%s' "$2" | grep -q -- "$3"; then pass=$((pass+1)); printf '  ok   %s [%.46s]\n' "$1" "$2"
      else fail=$((fail+1)); printf '  FAIL %s\n     want: %s\n     got : [%s]\n' "$1" "$3" "$2"; fi; }
hdr(){ tmux -L $S capture-pane -p -t "$SP" | sed -n 1p; }
tmux -L $S kill-server 2>/dev/null
tmux -L $S -f /dev/null new-session -d -x $W -y 24
T=$(tmux -L $S display-message -p '#{pane_id}'); SOCK=$(tmux -L $S display-message -p '#{socket_path}')
trap 'tmux -L $S kill-server 2>/dev/null' EXIT
tmux -L $S send-keys -t "$T" 'for i in $(seq -w 1 12); do echo "r$i ZAP$i end"; done' Enter; sleep 1
tmux -L $S new-window -d -n s "TMUX='$SOCK,0,0' '$SIFT' '$T'"
SP=$(tmux -L $S display-message -p -t s '#{pane_id}'); sleep 0.8
k(){ tmux -L $S send-keys -t "$SP" "$@"; sleep 0.35; }
kl(){ tmux -L $S send-keys -t "$SP" -l "$1"; sleep 0.3; }
for c in Z A P; do kl "$c"; done
N=$(TMUX="$SOCK,0,0" "$SIFT" rows "$T" 'ZAP' | wc -l)   # measured, never assumed
ck "0 header agrees with the headless seam (N=$N)" "$(hdr)" "$N matches"
OOR=$((N+1))                                            # first ordinal past the end
k M-1;            ck "1 entry M-1 buffers the digit"        "$(hdr)" '^goto> 1'
kl 2;             ck "2 second digit extends to 12"         "$(hdr)" '^goto> 12'
k BSpace;         ck "3a back to 1"                          "$(hdr)" '^goto> 1'
kl "${OOR:1:1}"; ck "3b out-of-range 1${OOR:1:1} (>N=$N) refused"  "$(hdr)" '^goto> 1'
k Down;           ck "4a Down rewrites the buffer"           "$(hdr)" '^goto> 2'
k Up;             ck "4b Up rewrites the buffer"             "$(hdr)" '^goto> 1'
k BSpace;         ck "5 last pop leaves the mode, pattern intact" "$(hdr)" '^regex> ZAP'
k M-3;            ck "6a re-enter at 3"                      "$(hdr)" '^goto> 3'
kl x;             ck "6b non-digit falls through to pattern" "$(hdr)" '^regex> ZAPx'
k BSpace; k M-3;  ck "6c back to ordinal 3"                  "$(hdr)" '^goto> 3'
# the ordinal COLUMN must be on screen next to its row (the Goodhart case)
rows=$(tmux -L $S capture-pane -p -t "$SP" | sed -n '2,13p')
ck "7 ordinal column rendered beside its row" "$(printf '%s' "$rows" | grep '^>' | head -1)" '^> *3 '
k Enter; sleep 0.8
got=$(tmux -L $S display-message -p -t "$T" '#{pane_in_mode}|#{copy_cursor_x}')
ck "8 Enter jumps the TARGET pane" "$got" '^1|4$'
printf 'feature probe w=%s: %d passed, %d failed\n' "$W" "$pass" "$fail"
[ "$fail" -eq 0 ]
