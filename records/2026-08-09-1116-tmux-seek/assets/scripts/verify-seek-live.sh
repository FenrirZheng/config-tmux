#!/usr/bin/env bash
# Live-search half of the seek Verify matrix.
#
# The trick that makes this scriptable: tmux-in-tmux. A copy-mode search is
# driven from the CLIENT's prompt, and `send-keys -t <pane>` delivers to the
# pane's *process*, bypassing tmux's key table -- which is why six earlier rigs
# failed. Running the inner server's client INSIDE a pane of an outer server
# makes the inner client's tty an ordinary pane, so `send-keys` on the outer
# server feeds real keystrokes to the inner client: prefix, prompt input, Enter.
#
# Covers Verify rows 1-4, 8 and 9. Row 7 (wl-copy degrade) and the Wayland
# clipboard itself still need a human -- wl-copy is stubbed here, and the copy
# path only reaches the real clipboard when the pane is visible in the client's
# current window.
set -u

OUT=seekverify_out; IN=seekverify_in
SEEK=/home/fenrir/.tmux/tools/target/release/seek
STUB=$(mktemp -d); PASS=0; FAIL=0
printf '#!/bin/sh\nexit 0\n' > "$STUB/wl-copy"; chmod +x "$STUB/wl-copy"

o() { tmux -L $OUT "$@"; }
i() { tmux -L $IN "$@"; }
cleanup() { o kill-server 2>/dev/null; i kill-server 2>/dev/null; rm -rf "$STUB"; }
trap cleanup EXIT

check() {
  if [ "$2" = "$3" ]; then printf '  PASS  %s\n' "$1"; PASS=$((PASS+1))
  else printf '  FAIL  %s\n        expected: [%s]\n        actual:   [%s]\n' "$1" "$2" "$3"; FAIL=$((FAIL+1)); fi
}

o kill-server 2>/dev/null; i kill-server 2>/dev/null; sleep 0.3
i -f /dev/null new-session -d -x 100 -y 28; sleep 0.4
i set-option -g history-limit 5000 >/dev/null
i source-file /home/fenrir/.tmux/records/2026-08-09-1116-tmux-seek/assets/seek-bindings.conf
IP=$(i list-panes -F '#{pane_id}' | head -1)
o -f /dev/null new-session -d -x 100 -y 30 "tmux -L $IN attach"; sleep 1.5
OP=$(o list-panes -F '#{pane_id}' | head -1)

i send-keys -t "$IP" 'clear; for n in $(seq -w 1 60); do echo "LINE$n needle$n"; done; echo "alpha bravo charlie"' Enter
sleep 1.5

# type a search through the outer client: prefix Space, text, Enter
search() {
  i send-keys -t "$IP" -X cancel 2>/dev/null; sleep 0.2
  o send-keys -t "$OP" C-b Space; sleep 0.6
  o send-keys -t "$OP" -l "$1"; sleep 0.6
  o send-keys -t "$OP" Enter; sleep 0.8
}
run() { # grain [extra...]
  i set-buffer -- "__EMPTY__"
  CC_TMUX_SOCKET=$IN PATH="$STUB:$PATH" "$SEEK" "$@" "$IP" >/dev/null 2>&1
  i show-buffer 2>/dev/null
}

echo "== live-search Verify matrix (tmux-in-tmux) =="

search 'needle05'
check "row1  search_present after a scrollback search" "1" "$(i display-message -p -t "$IP" '#{search_present}')"
check "row1  token grab lands on the MATCH, not the live screen" "needle05" "$(run word)"

search 'needle05'
check "row2  line grain after search" "LINE05 needle05" "$(run line)"

search 'alpha bravo'
check "row3  multi-token query delivers the whole match" "alpha bravo" "$(run word)"

search 'alpha bravo'
check "row4  line grain NEVER collapses to the query" "alpha bravo charlie" "$(run line)"

# row 9: repeat grab after a LIVE search, without leaving copy-mode
search 'needle07'
A=$(run word); STILL=$(i display-message -p -t "$IP" '#{pane_in_mode}'); B=$(run word)
check "row9  repeat grab #1"                    "needle07" "$A"
check "row9  still in copy-mode after delivery" "1"        "$STILL"
check "row9  repeat grab #2 (search still live)" "needle07" "$B"

# T5's guard: move the cursor off the match, the query must NOT be delivered.
# The contract is "not the query" -- the exact token depends on where the cursor
# lands, so assert the contract rather than a position-dependent value.
search 'alpha bravo'
i send-keys -t "$IP" -X cursor-up; sleep 0.3
MOVED=$(run word)
if [ "$MOVED" = "alpha bravo" ]; then
  printf '  FAIL  T5    cursor moved off match -> still delivered the stale query\n'; FAIL=$((FAIL+1))
elif [ -z "$MOVED" ] || [ "$MOVED" = "__EMPTY__" ]; then
  printf '  FAIL  T5    cursor moved off match -> delivered nothing\n'; FAIL=$((FAIL+1))
else
  printf '  PASS  T5    cursor moved off match -> grabbed the token under it [%s]\n' "$MOVED"; PASS=$((PASS+1))
fi

# the prompt label renders (row 8's normal-pane half)
i send-keys -t "$IP" -X cancel 2>/dev/null; sleep 0.2
o send-keys -t "$OP" C-b Space; sleep 0.8
SCR=$(o capture-pane -p -t "$OP")
printf '%s' "$SCR" | grep -qF '(seek)' && { echo "  PASS  row8  prompt label renders as (seek)"; PASS=$((PASS+1)); } \
  || { echo "  FAIL  row8  prompt label renders as (seek)"; FAIL=$((FAIL+1)); }
printf '%s' "$SCR" | grep -qF 'alternate_on' && { echo "  FAIL  row8  label leaked a literal format"; FAIL=$((FAIL+1)); } \
  || { echo "  PASS  row8  no literal format in the label"; PASS=$((PASS+1)); }

# a query containing a space AND a double quote survives %%%
o send-keys -t "$OP" -l 'a "b'; sleep 0.6; o send-keys -t "$OP" Enter; sleep 0.8
check "quote  query with a space and a double quote reaches tmux intact" 'a "b' "$(i display-message -p -t "$IP" '#{pane_search_string}')"

# row 7: the wl-copy degrade path. No `sudo mv` needed -- seek resolves wl-copy
# through PATH, so a stub that exits non-zero IS the failure, and the status line
# is readable from the outer capture because the inner client renders it.
BADSTUB=$(mktemp -d); printf '#!/bin/sh\nexit 1\n' > "$BADSTUB/wl-copy"; chmod +x "$BADSTUB/wl-copy"
search 'needle11'
i set-buffer -- "__EMPTY__"
CC_TMUX_SOCKET=$IN PATH="$BADSTUB:$PATH" "$SEEK" word "$IP" >/dev/null 2>&1
check "row7  buffer still filled when wl-copy fails" "needle11" "$(i show-buffer)"
sleep 0.5
MSG=$(o capture-pane -p -t "$OP" | tail -2)
printf '%s' "$MSG" | grep -qF 'wl-copy failed' \
  && { echo "  PASS  row7  degrade message names the failure"; PASS=$((PASS+1)); } \
  || { printf '  FAIL  row7  degrade message\n        got: [%s]\n' "$MSG"; FAIL=$((FAIL+1)); }
printf '%s' "$MSG" | grep -qF 'BUFFER' \
  && { echo "  PASS  row7  degrade message names the surviving sink"; PASS=$((PASS+1)); } \
  || { echo "  FAIL  row7  degrade message omits BUFFER"; FAIL=$((FAIL+1)); }
rm -rf "$BADSTUB"

# row 8, alt-screen half: the OTHER if-shell branch must render its own label
i send-keys -t "$IP" -X cancel 2>/dev/null; sleep 0.2
i send-keys -t "$IP" 'printf "\033[?1049h"; echo ALTSCREEN; sleep 30' Enter
sleep 1.5
check "row8  alternate_on is set" "1" "$(i display-message -p -t "$IP" '#{alternate_on}')"
o send-keys -t "$OP" C-b Space; sleep 1.0
SCR2=$(o capture-pane -p -t "$OP")
printf '%s' "$SCR2" | grep -qF 'visible screen only' \
  && { echo "  PASS  row8  alt-screen branch renders its warning label"; PASS=$((PASS+1)); } \
  || { echo "  FAIL  row8  alt-screen branch label"; FAIL=$((FAIL+1)); }

echo
echo "  passed=$PASS failed=$FAIL"
[ "$FAIL" -eq 0 ]
