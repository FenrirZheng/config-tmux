#!/usr/bin/env bash
# Feature-intact demonstration at w=100. Throwaway -L server, never $TMUX.
set -u
SIFT=$HOME/.tmux/tools/target/release/sift
FIXTURE=$HOME/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/sift-fixture.sh
S=sift_feat
[ -n "${TMUX:-}" ] && case "$TMUX" in *"/$S"*) echo "refusing"; exit 1;; esac
trap 'tmux -L $S kill-server 2>/dev/null' EXIT

t() { tmux -L $S "$@"; }
boot() {
  t kill-server 2>/dev/null; sleep 0.3
  t -f /dev/null new-session -d -x 100 -y 20
  TARGET=$(t display-message -p '#{pane_id}')
  SOCK=$(t display-message -p '#{socket_path}')
  t send-keys -t "$TARGET" "bash '$FIXTURE'" Enter; sleep 1.2
  t new-window -d -n sift "TMUX='$SOCK,0,0' '$SIFT' '$TARGET'"
  SPANE=$(t display-message -p -t sift '#{pane_id}'); sleep 0.8
}
hdr() { t capture-pane -p -t "$SPANE" | sed -n 1p | sed 's/  */ /g'; }
row() { t capture-pane -p -t "$SPANE" | sed -n "${1}p"; }

boot
t send-keys -t "$SPANE" -l 'aa'; sleep 0.5
echo "[0] baseline (pattern typed, no mode)"; echo "    $(hdr)"

t send-keys -t "$SPANE" M-1; sleep 0.5
echo "[1] ENTRY  M-1"; echo "    $(hdr)"; echo "    sel row: $(row 2)"

t send-keys -t "$SPANE" -l '2'; sleep 0.4
echo "[2] EXTEND second digit '2'"; echo "    $(hdr)"

t send-keys -t "$SPANE" -l '0'; sleep 0.4
t send-keys -t "$SPANE" -l '9'; sleep 0.4
echo "[3] REFUSE '9' would make 1209 > 201 matches"; echo "    $(hdr)"

t send-keys -t "$SPANE" Down; sleep 0.4
echo "[4a] DOWN rewrites buffer"; echo "    $(hdr)"
t send-keys -t "$SPANE" Up; sleep 0.4
echo "[4b] UP rewrites buffer"; echo "    $(hdr)"

echo "[5] BACKSPACE x N pops out of the mode"
for i in 1 2 3; do
  t send-keys -t "$SPANE" BSpace; sleep 0.4
  echo "    bs$i: $(hdr)"
done

echo "[6] NON-DIGIT falls through into the pattern"
t send-keys -t "$SPANE" M-5; sleep 0.4; echo "    re-enter: $(hdr)"
t send-keys -t "$SPANE" -l 'b'; sleep 0.4; echo "    typed 'b': $(hdr)"

echo "[7] ENTER lands the TARGET pane on the right occurrence"
t kill-server 2>/dev/null; sleep 0.3
boot
RE='bb15'
t send-keys -t "$SPANE" -l "$RE"; sleep 0.6
echo "    $(hdr)"
# expected 3rd hit, computed from the headless seam
exp=$(TMUX="$SOCK,0,0" "$SIFT" rows "$TARGET" "$RE" | sed -n 3p)
echo "    sift rows 3rd hit (line<TAB>char_start<TAB>char_end<TAB>cell_start): $exp"
expline=$(printf '%s' "$exp" | cut -f1); expcell=$(printf '%s' "$exp" | cut -f4)
t send-keys -t "$SPANE" M-3; sleep 0.5
echo "    after M-3: $(hdr)"
echo "    selected row: $(row 4)"
t send-keys -t "$SPANE" Enter; sleep 1.0
read -r hs sp cy cx pim <<<"$(t display-message -p -t "$TARGET" \
  '#{history_size} #{scroll_position} #{copy_cursor_y} #{copy_cursor_x} #{pane_in_mode}')"
gotline=$((hs - sp + cy))
echo "    TARGET: pane_in_mode=$pim resolved_line=$gotline copy_cursor_x=$cx"
echo "    EXPECT: pane_in_mode=1 resolved_line=$expline copy_cursor_x=$expcell"
[ "$pim" = 1 ] && [ "$gotline" = "$expline" ] && [ "$cx" = "$expcell" ] \
  && echo "    => LANDED ON THE 3rd OCCURRENCE: PASS" || echo "    => FAIL"
t capture-pane -p -t "$TARGET" | sed -n "$((cy+1))p" | sed 's/^/    target screen line at cursor: /'
