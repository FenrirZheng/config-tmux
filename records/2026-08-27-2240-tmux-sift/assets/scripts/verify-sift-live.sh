#!/usr/bin/env bash
# verify-sift-live.sh — end-to-end: drive the real TUI with real keystrokes and
# assert the target pane ends up on the right match.
#
# verify-sift-jump.sh asserts the arithmetic by issuing the tmux commands by
# hand; that would still pass if sift's own jump() were wired up wrongly. Here
# sift runs in a second pane of a throwaway server, receives keys through
# send-keys, and the assertions read the TARGET pane. Also measures the filter
# cost against a full 100000-line scrollback.
#
# Sections 6-8 pin ordinal mode (ADR-0007). Every one of those thirteen assertions
# was watched to FAIL against a pre-ordinal build of tools/sift/src/main.cpp
# (`git show <pre-ordinal>:tools/sift/src/main.cpp | g++ -std=c++20 -O2 -x c++ -`)
# while all six older assertions still passed there. Both suites honour $SIFT,
# which is how that control is run — never overwrite tools/target/release/sift.
set -u
SIFT=${SIFT:-$HOME/.tmux/tools/target/release/sift}
FIXTURE=${FIXTURE:-$(dirname "$0")/sift-fixture.sh}
S=sift_live
pass=0; fail=0
ok()   { pass=$((pass+1)); printf '  ok   %s\n' "$1"; }
bad()  { fail=$((fail+1)); printf '  FAIL %s\n     want: %s\n     got : %s\n' "$1" "$2" "$3"; }
check(){ [ "$2" = "$3" ] && ok "$1" || bad "$1" "$2" "$3"; }

[ -x "$SIFT" ] || { echo "sift not built at $SIFT"; exit 1; }
tmux -L $S kill-server 2>/dev/null
tmux -L $S -f /dev/null new-session -d -x 100 -y 30
trap 'tmux -L $S kill-server 2>/dev/null' EXIT
TARGET=$(tmux -L $S display-message -p '#{pane_id}')
SOCK=$(tmux -L $S display-message -p '#{socket_path}')
t() { tmux -L $S "$@"; }

t send-keys -t "$TARGET" "bash '$FIXTURE'" Enter
sleep 1.5

# sift runs in its own window of the same server, told to search $TARGET.
t new-window -d -n sift "TMUX='$SOCK,0,0' '$SIFT' '$TARGET'"
SPANE=$(t display-message -p -t sift '#{pane_id}')
sleep 0.8

type_keys() { for k in "$@"; do t send-keys -t "$SPANE" -l "$k"; sleep 0.05; done; }

echo "== 1. the popup renders and counts matches live =="
type_keys b b 1 5 0
sleep 0.5
screen=$(t capture-pane -p -t "$SPANE")
case "$screen" in
  *"1 matches"*|*"1 match"*) ok "header reports 1 match for bb150" ;;
  *) bad "header reports 1 match for bb150" "'1 matches' in header" "$(printf '%s' "$screen" | head -1)" ;;
esac

echo "== 2. Enter jumps the TARGET pane onto the match start =="
t send-keys -t "$SPANE" Enter
sleep 0.8
# bb150 sits at column 13 of "row150 aa150 bb150 cc150".
got=$(t display-message -p -t "$TARGET" '#{copy_cursor_x}|#{search_present}|#{pane_in_mode}')
check "target cursor on bb150, search registered, still in copy-mode" "13|1|1" "$got"

echo "== 3. the popup process exited on its own (display-popup -E would close) =="
t list-windows -F '#{window_name}' | grep -qx sift && \
  bad "sift window gone" "no sift window" "still present" || ok "sift exited after the jump"

echo "== 4. Esc cancels without touching the target =="
t send-keys -X -t "$TARGET" cancel
before=$(t display-message -p -t "$TARGET" '#{pane_in_mode}')
t new-window -d -n sift2 "TMUX='$SOCK,0,0' '$SIFT' '$TARGET'"
S2=$(t display-message -p -t sift2 '#{pane_id}')
sleep 0.8
for k in b b 1 5 0; do t send-keys -t "$S2" -l "$k"; sleep 0.05; done
t send-keys -t "$S2" Escape
sleep 0.6
check "target untouched by Esc (pane_in_mode)" "$before" \
      "$(t display-message -p -t "$TARGET" '#{pane_in_mode}')"

echo "== 5. filter cost against a full 100000-line scrollback =="
t set-option -g history-limit 100000 >/dev/null
t new-window -d -n big
BIG=$(t display-message -p -t big '#{pane_id}')
t send-keys -t "$BIG" 'seq 1 100000 | sed "s/^/line /"' Enter
sleep 8
hs=$(t display-message -p -t "$BIG" '#{history_size}')
echo "  history_size=$hs"
start=$(date +%s%N)
n=$(TMUX="$SOCK,0,0" "$SIFT" rows "$BIG" 'line 9[0-9]{4}' | wc -l)
ms=$(( ($(date +%s%N) - start) / 1000000 ))
echo "  matched $n lines in ${ms}ms"
[ "$ms" -lt 300 ] && ok "one filter pass under 300ms (${ms}ms)" \
                  || bad "one filter pass under 300ms" "<300ms" "${ms}ms"

echo "== 6. ordinal mode: entry, buffer, out-of-range, movers, Backspace, fallthrough =="
# ADR-0007. Nine assertions across sections 6-8 pin the mode; each one was shown
# to redden against a pre-ordinal build of tools/sift/src/main.cpp before it was
# kept (see the ordinal-mode note in the map record).
#
# N is MEASURED here, never assumed: the out-of-range case needs a pattern whose
# hit count is small and known, so that ordinal 12 is reachable and 15 is not.
# /[abc][abc]19[0-3]/ is rows 190-193 x aa/bb/cc = 12 OCCURRENCES over 4 lines,
# which also means an ordinal is not a line number.
ORD_RE='[abc][abc]19[0-3]'
ORD_N=$(TMUX="$SOCK,0,0" "$SIFT" rows "$TARGET" "$ORD_RE" | wc -l)
echo "  /$ORD_RE/ -> $ORD_N occurrences (want 12)"
# Fail closed: if the fixture drifts, the assertions below are measuring
# something else and their verdicts are meaningless.
[ "$ORD_N" -eq 12 ] || { fail=$((fail+1)); printf '  FAIL ordinal fixture drifted\n     want: 12 hits for /%s/\n     got : %s\n' "$ORD_RE" "$ORD_N"; }

t new-window -d -n ord "TMUX='$SOCK,0,0' '$SIFT' '$TARGET'"
OP=$(t display-message -p -t ord '#{pane_id}')
sleep 0.8
# The header is "<prompt><typed>" + padding + "<status>". The typed field is
# everything before the first double space — no pattern used here contains one.
typed() { t capture-pane -p -t "$OP" | sed -n 1p | sed 's/  .*$//'; }
key()   { t send-keys -t "$OP" "$1"; sleep 0.3; }
lit()   { t send-keys -t "$OP" -l "$1"; sleep 0.3; }
for ((i = 0; i < ${#ORD_RE}; i++)); do t send-keys -t "$OP" -l "${ORD_RE:i:1}"; sleep 0.06; done
sleep 0.4

key M-1
check "M-<digit> enters ordinal mode and is itself the first digit" "goto> 1" "$(typed)"

lit 2
reach=$(typed)
check "a bare digit extends the buffer" "goto> 12" "$reach"

# Out of range is refused WITHOUT stranding what is reachable: at N=12 the same
# leading `1` must still reach #12 (above), while `1` then `5` is not buffered.
key BSpace
lit 5
check "out-of-range digit refused, reachable candidates not stranded (N=$ORD_N)" \
      "goto> 12|goto> 1" "$reach|$(typed)"

# Buffer and selection are ONE object, so every mover rewrites the buffer.
# Home/End are deliberately absent: tmux sends ESC [ 1 ~ / ESC [ 4 ~ for them and
# sift's CSI switch decodes only ESC [ H / ESC [ F, so the `~` lands in the
# pattern. That is pre-existing (it predates ordinal mode) and out of scope here.
movers=""
for k in Down C-n PgDn Up C-p PgUp; do key "$k"; movers="$movers$(typed)|"; done
check "Down/C-n/PgDn/Up/C-p/PgUp move the selection and the buffer follows" \
      "goto> 2|goto> 3|goto> 12|goto> 11|goto> 10|goto> 1|" "$movers"

# Leaving the mode and eating a pattern character are never the same keystroke,
# so three Backspaces from a two-digit buffer must produce three DIFFERENT
# states. Two digits deep on purpose: from a one-digit buffer the first pop and
# a plain pattern-delete land on the same string, and a pre-ordinal build
# satisfies the assertion by accident (measured — it did).
lit 2                        # buffer is "12" again
key BSpace; pop1=$(typed)    # pops a digit, still in the mode
key BSpace; pop2=$(typed)    # the last pop leaves the mode, pattern untouched
key BSpace; pop3=$(typed)    # only now does Backspace eat the pattern
check "Backspace pops a digit; the last pop leaves the mode; the next deletes pattern" \
      "goto> 1|regex> $ORD_RE|regex> ${ORD_RE%?}" "$pop1|$pop2|$pop3"
lit ']'   # restore the pattern the rest of this section depends on

key M-4;  entered=$(typed)
lit x;    fell=$(typed)
check "a non-digit printable leaves the mode and lands in the pattern" \
      "goto> 4|regex> ${ORD_RE}x" "$entered|$fell"
key BSpace   # drop the x again

# The Goodhart case: an implementation that buffers digits perfectly while
# rendering no column would satisfy every assertion above and still be useless,
# because the number has to be readable before it is typed (ADR-0007). Rows are
# "> " or "  ", then the ordinal right-aligned at the width of N.
key M-1; lit 2
scr=$(t capture-pane -p -t "$OP")
ords=$(printf '%s\n' "$scr" | sed -n '2,13p' | cut -c3-4 | tr -d ' ' | paste -sd,)
marker=$(printf '%s\n' "$scr" | sed -n 13p | cut -c1-4)
check "the ordinal column is on screen beside its row, selection marked" \
      "1,2,3,4,5,6,7,8,9,10,11,12|> 12" "$ords|$marker"

echo "== 7. Enter jumps to the ORDINAL's occurrence — asserted on the TARGET pane =="
# Read on the target, never on the popup's own rendering, exactly as section 2
# does.
#
# Ordinal 12 first, because the buffer is already there from the column
# assertion above and it is a real use case (reach the LAST match by number):
# the 12th row of `sift rows` is the cc193 token at column 19.
t send-keys -t "$OP" Enter
sleep 0.9
check "ordinal 12 — the last match — puts the target cursor on cc193" \
      "19|1|1" "$(t display-message -p -t "$TARGET" '#{copy_cursor_x}|#{search_present}|#{pane_in_mode}')"
t send-keys -X -t "$TARGET" cancel

# ...but ordinal 12 is ALSO the default selection: sift seats `sel` on the last
# hit, so a build whose push_ordinal_digit was a no-op would jump there anyway
# and satisfy the assertion above while ordinal selection was entirely broken.
# The load-bearing case must therefore be a NON-default ordinal. Ordinal 5 is
# the bb191 token: column 13 (the default's is 19) on a different fixture line
# (row191, not row193), so one assertion discriminates on both axes — a no-op
# digit buffer lands on 19, and a line-level rather than occurrence-level
# ordinal lands somewhere else again.
#
# Only STRUCTURAL fields are compared. The column is structural; so is the
# matched line's text. The absolute scrollback index is NOT — it moves with the
# pane's history depth and differs per throwaway server — so it is never
# asserted for equality. #{copy_cursor_line} is safe here only because this
# fixture line is ASCII: it truncates at the first wide character, which is
# exactly what jump suite section 7 pins on the CJK line. Do not reuse this
# shape there.
t new-window -d -n ord5 "TMUX='$SOCK,0,0' '$SIFT' '$TARGET'"
OP=$(t display-message -p -t ord5 '#{pane_id}')
sleep 0.8
for ((i = 0; i < ${#ORD_RE}; i++)); do t send-keys -t "$OP" -l "${ORD_RE:i:1}"; sleep 0.06; done
sleep 0.4
key M-5
check "M-5 selects the non-default ordinal 5" "goto> 5" "$(typed)"
t send-keys -t "$OP" Enter
sleep 0.9
got=$(t display-message -p -t "$TARGET" \
      '#{copy_cursor_x}|#{search_present}|#{pane_in_mode}|#{copy_cursor_line}' | sed 's/ *$//')
check "ordinal 5 lands on bb191 (col 13, row191 line) — not the default cc193/col 19" \
      "13|1|1|row191 aa191 bb191 cc191" "$got"
t send-keys -X -t "$TARGET" cancel

echo "== 8. the header survives a narrow pane, and the footer fits its own budget =="
# The regression test for 2026-09-01: the footer had grown to 75 cells with no
# width guard, so at any sift width <= 74 it wrapped, the pane scrolled, and the
# header — the `goto>` prompt AND the match count — left the screen. Ordinal mode
# was fully functional and completely invisible, and every assertion above, all
# written at 100 columns, passed straight through it.
# (The footer is 81 cells since t5 reworded it; 74 is still below that, so the
# guard's behaviour at this width is unchanged and the 75 above stays as the
# figure that was measured on the day.)
#
# Asserted as screen LINE 1, not as a grep over the whole screen: a grep passes
# while the header sits anywhere, and the whole point is that it is on line 1.
t new-session -d -s narrow -x 74 -y 20
t new-window -d -t narrow -n ordn "TMUX='$SOCK,0,0' '$SIFT' '$TARGET'"
NP=$(t display-message -p -t narrow:ordn '#{pane_id}')
sleep 0.8
geo=$(t display-message -p -t "$NP" '#{pane_width}x#{pane_height}')
echo "  narrow sift pane: $geo"
# Fail closed: a pane that is not actually narrow cannot reproduce the defect.
[ "$geo" = "74x20" ] || { fail=$((fail+1)); printf '  FAIL narrow harness geometry\n     want: 74x20\n     got : %s\n' "$geo"; }
for ((i = 0; i < ${#ORD_RE}; i++)); do t send-keys -t "$NP" -l "${ORD_RE:i:1}"; sleep 0.06; done
sleep 0.4
t send-keys -t "$NP" M-1
sleep 0.6
line1=$(t capture-pane -p -t "$NP" | sed -n 1p)
case "$line1" in
  "goto> "*) ok "at 74x20, screen line 1 is still the goto> header" ;;
  *) bad "at 74x20, screen line 1 is still the goto> header" \
         "line 1 begins with 'goto> '" "$line1" ;;
esac
t kill-session -t narrow

# ... and the other edge: the footer at EXACTLY its own width. Asserting that the
# ordinal item merely "survives the cut" would prove nothing — utf8_fit cuts from
# the RIGHT, that item sits at cells 12-36, and every future addition lands after
# it, so such an assertion is true at any width >= 36 and can never redden. What
# growth actually threatens is the tail. Asserting the WHOLE line at exact fit is
# what has teeth: one added cell anywhere on it reddens this.
#
# The -x 81 and the string below are two encodings of one measurement and move
# together; the string is transcribed from kFooter in tools/sift/src/main.cpp.
FOOTER='↑↓ select  left-Alt-<n> goto match n  Enter jump  Esc cancel  C-w word  C-u clear'
t new-session -d -s fitw -x 81 -y 20
t new-window -d -t fitw -n ordf "TMUX='$SOCK,0,0' '$SIFT' '$TARGET'"
FP=$(t display-message -p -t fitw:ordf '#{pane_id}')
sleep 0.8
geo=$(t display-message -p -t "$FP" '#{pane_width}x#{pane_height}')
echo "  exact-fit sift pane: $geo"
# Fail closed: a pane that is not the footer's own width cannot test the budget.
[ "$geo" = "81x20" ] || { fail=$((fail+1)); printf '  FAIL exact-fit harness geometry\n     want: 81x20\n     got : %s\n' "$geo"; }
for ((i = 0; i < ${#ORD_RE}; i++)); do t send-keys -t "$FP" -l "${ORD_RE:i:1}"; sleep 0.06; done
sleep 0.4
t send-keys -t "$FP" M-1
sleep 0.6
scr=$(t capture-pane -p -t "$FP")
line1=$(printf '%s\n' "$scr" | sed -n 1p)
case "$line1" in
  "goto> "*) ok "at 81x20, screen line 1 is still the goto> header" ;;
  *) bad "at 81x20, screen line 1 is still the goto> header" \
         "line 1 begins with 'goto> '" "$line1" ;;
esac
# Line 20, not `tail -1`: draw()'s h-line contract fixes the footer's row, so a
# footer that wrapped would put something else there — which is the failure this
# is aimed at, and `tail -1` would hide it.
check "at 81x20 the whole footer renders, uncut" "$FOOTER" \
      "$(printf '%s\n' "$scr" | sed -n 20p | sed 's/ *$//')"
t kill-session -t fitw

echo "== 9. the REAL binding, driven by a real prefix keypress =="
# The regression test for 2026-08-28: tests 1-4 launch sift in a window with the
# pane id already substituted by the shell, so they never exercise the binding.
# `display-popup` does NOT format-expand its shell-command, so a binding passing
# `#{pane_id}` handed sift a literal string and the popup flashed shut — invisible
# to every assertion above. This drives claude.conf's actual binding through a
# nested client, the only way a real `prefix /` keystroke can be delivered.
#
# What it detects is the flash-and-close failure mode in general, NOT that one
# cause: sift's origin_pane() now self-heals a literal `#{pane_id}`, so
# reintroducing the old binding alone no longer reddens this. Verified to have
# teeth by pointing the binding at a nonexistent pane (`sift %99999`), which
# reproduces the same user-visible symptom and does fail here.
O=sift_live_outer
tmux -L $O kill-server 2>/dev/null
tmux -L $O -f /dev/null new-session -d -x 282 -y 71
OP=$(tmux -L $O display-message -p '#{pane_id}')
tmux -L $O send-keys -t "$OP" "tmux -L $S attach" Enter
sleep 1.5
# Load the real config so the binding under test is the shipped one.
t source-file "$HOME/.tmux/claude.conf" 2>/dev/null
tmux -L $O send-keys -t "$OP" C-b; sleep 0.3
tmux -L $O send-keys -t "$OP" /;   sleep 1.5
screen=$(tmux -L $O capture-pane -p -t "$OP")
case "$screen" in
  *"regex>"*) ok "prefix / opens sift and it renders its prompt" ;;
  *) bad "prefix / opens sift and it renders its prompt" "'regex>' on screen" \
         "popup absent or closed immediately" ;;
esac
tmux -L $O send-keys -t "$OP" Escape; sleep 0.4
tmux -L $O kill-server 2>/dev/null

echo
printf 'passed %d, failed %d\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
