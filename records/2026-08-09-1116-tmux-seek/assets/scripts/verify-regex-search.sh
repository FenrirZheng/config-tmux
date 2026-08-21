#!/usr/bin/env bash
# Verify the `prefix /` regex-search binding (claude.conf, added 2026-08-22 as
# the regex sibling of seek's prefix Space entry).
#
# tmux-in-tmux, same trick as verify-seek-live.sh: a copy-mode search is
# driven from the CLIENT's prompt and `send-keys -t <pane>` bypasses the key
# table, so the inner server's client runs inside a pane of an outer server
# and outer send-keys feeds it real keystrokes (prefix, pattern, Enter).
#
# The binding under test is EXTRACTED from claude.conf at run time, so this
# asserts the production text, never a drifting copy.
set -u

OUT=rxverify_out; IN=rxverify_in
CONF=/home/fenrir/.tmux/claude.conf
PASS=0; FAIL=0
TMPCONF=$(mktemp)

o() { tmux -L $OUT "$@"; }
i() { tmux -L $IN "$@"; }
cleanup() { o kill-server 2>/dev/null; i kill-server 2>/dev/null; rm -f "$TMPCONF"; }
trap cleanup EXIT

check() {
  if [ "$2" = "$3" ]; then printf '  PASS  %s\n' "$1"; PASS=$((PASS+1))
  else printf '  FAIL  %s\n        expected: [%s]\n        actual:   [%s]\n' "$1" "$2" "$3"; FAIL=$((FAIL+1)); fi
}

# The bind spans three lines; the range ends on the plain "(regex)" prompt.
sed -n '/^bind -T prefix \//,/"(regex)"/p' "$CONF" > "$TMPCONF"
[ -s "$TMPCONF" ] || { echo "FAIL: prefix / binding not found in $CONF"; exit 1; }

o kill-server 2>/dev/null; i kill-server 2>/dev/null; sleep 0.3
i -f /dev/null new-session -d -x 60 -y 15; sleep 0.4
i source-file "$TMPCONF"
IP=$(i list-panes -F '#{pane_id}' | head -1)
o -f /dev/null new-session -d -x 60 -y 17 "tmux -L $IN attach"; sleep 1.5
OP=$(o list-panes -F '#{pane_id}' | head -1)

i send-keys -t "$IP" 'clear; echo abc123; echo zzz456' Enter; sleep 1

rxsearch() { # type a regex through the outer client: prefix /, pattern, Enter
  i send-keys -t "$IP" -X cancel 2>/dev/null; sleep 0.2
  o send-keys -t "$OP" C-b /; sleep 0.6
  o send-keys -t "$OP" -l "$1"; sleep 0.4
  o send-keys -t "$OP" Enter; sleep 0.8
}
probe() { i display-message -p -t "$IP" '#{search_present} #{copy_cursor_x} #{copy_cursor_y}'; }

echo "== prefix / regex search (tmux-in-tmux) =="

# Screen: abc123 (row 0), zzz456 (row 1), prompt (row 2). Backward search
# from the prompt finds 456 first; n steps up to 123.
rxsearch '[0-9]+'
check "class+quantifier lands on nearest match (456)" "1 3 1" "$(probe)"
o send-keys -t "$OP" n; sleep 0.6
check "n steps to the previous match (123)"           "1 3 0" "$(probe)"

rxsearch 'a.c'
check "dot metachar matches abc (regex, not literal)" "1 0 0" "$(probe)"

echo "passed=$PASS failed=$FAIL"
[ $FAIL -eq 0 ]
