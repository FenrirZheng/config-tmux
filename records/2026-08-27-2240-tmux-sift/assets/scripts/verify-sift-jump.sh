#!/usr/bin/env bash
# verify-sift-jump.sh — the load-bearing assertions for sift's jump arithmetic.
#
# Every claim here was a measured surprise on tmux 3.5a, not a guess:
#   * `goto-line N` sets the scroll offset from the BOTTOM and leaves the cursor
#     row alone, so `history-top` must pin cy=0 first;
#   * `search-backward` seats the cursor on the match START (search-forward
#     seats it one cell past the END), which is what seek's w/W/l/L need;
#   * both searches wrap silently on failure, so a landing must be verified.
#
# sift finds its server through $TMUX like every other tool here, so the harness
# must point $TMUX at the throwaway socket — otherwise `sift rows` quietly
# queries the user's real server and the assertions measure the wrong thing.
#
# Run: bash verify-sift-jump.sh    (exit 0 = all assertions passed)
set -u
SIFT=${SIFT:-$HOME/.tmux/tools/target/release/sift}
S=sift_verify
pass=0; fail=0
ok()   { pass=$((pass+1)); printf '  ok   %s\n' "$1"; }
bad()  { fail=$((fail+1)); printf '  FAIL %s\n     want: %s\n     got : %s\n' "$1" "$2" "$3"; }
check(){ [ "$2" = "$3" ] && ok "$1" || bad "$1" "$2" "$3"; }
# A blank field means the seam produced nothing; that must fail loudly rather
# than flow into `-N ""` and be misread as a jump bug.
need() { [ -n "$2" ] || { bad "$1" "non-empty" "<empty>"; return 1; }; }

[ -x "$SIFT" ] || { echo "sift not built at $SIFT"; exit 1; }
tmux -L $S kill-server 2>/dev/null
tmux -L $S -f /dev/null new-session -d -x 100 -y 24
trap 'tmux -L $S kill-server 2>/dev/null' EXIT
P=$(tmux -L $S display-message -p '#{pane_id}')
SOCK=$(tmux -L $S display-message -p '#{socket_path}')
sift() { TMUX="$SOCK,0,0" "$SIFT" "$@"; }
t()    { tmux -L $S "$@"; }

# Control case: the harness must be aimed at the throwaway server. If this
# fails, every assertion below would be measuring the user's real tmux.
check "harness targets the throwaway server" "$P" "$(TMUX="$SOCK,0,0" tmux display-message -p '#{pane_id}')"

# Three occurrences per line so an occurrence-level jump can be told apart from
# a line-level one, plus a CJK line so cell/char/byte confusion would show.
FIXTURE=${FIXTURE:-$(dirname "$0")/sift-fixture.sh}
[ -r "$FIXTURE" ] || { echo "fixture not found: $FIXTURE"; exit 1; }
t send-keys -t "$P" "bash '$FIXTURE'" Enter
sleep 1.5

echo "== 1. rows: the headless seam finds every occurrence =="
check "bb1xx occurrence count" "100" "$(sift rows "$P" 'bb1[0-9][0-9]' | wc -l)"

echo "== 2. rows: columns are CHARACTERS, not bytes (CJK line) =="
# "中文測試 aa999 尾巴": 4 CJK + 1 space = 5 characters before "aa999",
# but 13 bytes. A byte-based seam would report 13/18 here.
check "CJK char_start/char_end" "$(printf '5\t10')" "$(sift rows "$P" 'aa999' | head -1 | cut -f2,3)"

echo "== 3. the jump lands on the picked OCCURRENCE, cursor on its start =="
line=$(sift rows "$P" '^row150 ' | head -1 | cut -f1)
if need "row150 located" "$line"; then
for spec in "aa150 7" "bb150 13" "cc150 19"; do
  set -- $spec; tok=$1; want_x=$2
  ce=$(sift rows "$P" "$tok" | head -1 | cut -f3)
  need "char_end for $tok" "$ce" || continue
  hs=$(t display-message -p -t "$P" '#{history_size}')
  t copy-mode -t "$P"
  t send-keys -X -t "$P" history-top
  t send-keys -X -t "$P" goto-line $((hs - line))
  t send-keys -X -N "$ce" -t "$P" cursor-right
  t send-keys -X -t "$P" search-backward '[abc][abc]1[0-9][0-9]'
  check "jump to $tok" "$want_x|1" "$(t display-message -p -t "$P" '#{copy_cursor_x}|#{search_present}')"
done
fi

echo "== 4. after the jump, n / N still step (tmux registered the pattern) =="
t send-keys -X -t "$P" search-again
check "n steps backward one occurrence" "13" "$(t display-message -p -t "$P" '#{copy_cursor_x}')"

echo "== 5. the pane stays in copy-mode so seek's w/W/l/L can chain =="
check "pane_in_mode" "1" "$(t display-message -p -t "$P" '#{pane_in_mode}')"

echo "== 6. a target in the VISIBLE screen (index > history_size) =="
t send-keys -X -t "$P" cancel
row=$(sift rows "$P" '中文測試' | head -1)
last=$(printf '%s' "$row" | cut -f1)
if need "CJK line located" "$last"; then
  hs=$(t display-message -p -t "$P" '#{history_size}')
  t copy-mode -t "$P"; t send-keys -X -t "$P" history-top
  if [ "$last" -gt "$hs" ]; then
    t send-keys -X -t "$P" goto-line 0
    t send-keys -X -N $((last - hs)) -t "$P" cursor-down
  else
    t send-keys -X -t "$P" goto-line $((hs - last))
  fi
  # Positional, not textual: see test 7 for why the text probe is unusable here.
  read -r vh vs vy <<<"$(t display-message -p -t "$P" '#{history_size} #{scroll_position} #{copy_cursor_y}')"
  check "screen-region target lands on line $last" "$last" "$((vh - vs + vy))"
fi

echo "== 7. #{copy_cursor_line} is NOT a usable probe on wide characters =="
# Pins the 3.5a behaviour that forced sift's verification to be positional: the
# format truncates at the first wide char. If a future tmux fixes this, this
# assertion fails and the comment in jump() can be revisited.
check "copy_cursor_line truncates at the first CJK cell" "中" \
      "$(t display-message -p -t "$P" '#{copy_cursor_line}' | sed 's/ *$//')"

echo "== 8. cell_start vs char_start: the CJK line distinguishes them =="
# "中文測試 aa999 尾巴" — 5 characters but 9 CELLS before "aa999".
check "char_start/cell_start for aa999" "$(printf '5\t9')" \
      "$(sift rows "$P" 'aa999' | head -1 | cut -f2,4)"

echo "== 9. regex parity: sift's match set == tmux's own search =="
# sift lists the hits but tmux performs the jump, so a divergence would show as
# "it is in the list but the cursor goes elsewhere". Walk tmux backwards over
# the whole scrollback with search-again and count what it finds.
RE='bb1[0-9][0-9]'
t send-keys -X -t "$P" cancel
t copy-mode -t "$P"
t send-keys -X -t "$P" search-backward "$RE"
seen=""; n=0
for _ in $(seq 1 120); do
  pos=$(t display-message -p -t "$P" '#{history_size}|#{scroll_position}|#{copy_cursor_y}|#{copy_cursor_x}')
  case " $seen " in *" $pos "*) break;; esac
  seen="$seen $pos"; n=$((n+1))
  t send-keys -X -t "$P" search-again
done
check "tmux finds the same number of $RE hits as sift" "$(sift rows "$P" "$RE" | wc -l)" "$n"

echo "== 10. zero-width patterns terminate instead of spinning =="
timeout 20 env TMUX="$SOCK,0,0" "$SIFT" rows "$P" '^' >/dev/null 2>&1
check "'^' returns (no infinite loop)" "0" "$?"

echo
printf 'passed %d, failed %d\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
