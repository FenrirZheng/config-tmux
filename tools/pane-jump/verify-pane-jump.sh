#!/usr/bin/env bash
# verify-pane-jump.sh — the load-bearing assertions for pane-jump, headless.
#
# Two blocks, 28 assertions:
#   groups 1-5   the `rows` and `jump` seams (spec R1 to R4), no TUI involved;
#   groups 6-8   the whole thing through the bound key: a popup opened by
#                `prefix g`, keystrokes typed into it, Enter or Escape
#                (spec R1, R3, R4, R5).
#
# Everything runs on TWO throwaway servers and never touches the user's:
#   -L pj_verify   the server under test: session `t`, three panes titled
#                  alpha-build / beta-server / gamma-notes. `select-pane -T` is
#                  used to set them, which the tool itself must never do — this
#                  is a fixture on a disposable server, not the tool.
#   -L pj_outer    one pane running `tmux -L pj_verify attach`, i.e. the
#                  terminal of pj_verify's client. It exists for two reasons:
#                  `switch-client` needs an attached client at all, and keys for
#                  a PREFIX binding must arrive at that client's terminal —
#                  `send-keys` into a pj_verify pane would inject into the pane
#                  and bypass the key table entirely (runbooks/switch-pane-by-
#                  name.md, testing note).
#
# pane-jump finds its server through $TMUX like every other tool here, so the
# harness must point $TMUX at the throwaway socket — otherwise `pane-jump rows`
# quietly queries the user's real server and the assertions measure the wrong
# thing. Assertion 1 is the control for exactly that.
#
# Run: bash verify-pane-jump.sh        (exit 0 = all assertions passed)
#      PANE_JUMP=…/pane-jump-asan bash verify-pane-jump.sh   (sanitizer twin)
set -u
BIN=${PANE_JUMP:-$(dirname "$0")/../target/release/pane-jump}
V=pj_verify
O=pj_outer
pass=0; fail=0
ok()   { pass=$((pass+1)); printf '  ok   %s\n' "$1"; }
bad()  { fail=$((fail+1)); printf '  FAIL %s\n     want: %s\n     got : %s\n' "$1" "$2" "$3"; }
check(){ [ "$2" = "$3" ] && ok "$1" || bad "$1" "$2" "$3"; }
# Substring assertion, for the two places where the exact text around the match
# is not the claim: a stderr line and a captured screen. A blank haystack fails
# it, so a seam that produced nothing is reported rather than passing quietly.
contains() { case "$3" in *"$2"*) ok "$1";; *) bad "$1" "*$2*" "$3";; esac; }

[ -x "$BIN" ] || { echo "pane-jump not built at $BIN"; exit 1; }

tmux -L $V kill-server 2>/dev/null
tmux -L $O kill-server 2>/dev/null
trap 'tmux -L $V kill-server 2>/dev/null; tmux -L $O kill-server 2>/dev/null' EXIT

# ── fixture ────────────────────────────────────────────────────────────────
tmux -L $V -f /dev/null new-session -d -s t -x 100 -y 30
tmux -L $V split-window -d -t %0
tmux -L $V split-window -d -t %1
tmux -L $V select-pane -t %0 -T alpha-build
tmux -L $V select-pane -t %1 -T beta-server
tmux -L $V select-pane -t %2 -T gamma-notes
tmux -L $V select-pane -t %0

SOCK=$(tmux -L $V display-message -p '#{socket_path}')
pj()     { TMUX="$SOCK,0,0" "$BIN" "$@"; }
active() { tmux -L $V display-message -p '#{pane_id}'; }

echo "== 1. control: the harness is aimed at the throwaway server =="
# If this fails, every assertion below would be measuring the user's real tmux.
check "TMUX points at pj_verify" "$(active)" \
      "$(TMUX="$SOCK,0,0" tmux display-message -p '#{pane_id}')"

echo "== 2. rows: one line per pane, list-panes order (R1) =="
check "row count" "3" "$(pj rows | wc -l)"
check "titles in order" "$(printf 'alpha-build\nbeta-server\ngamma-notes')" "$(pj rows | cut -f4)"
check "four tab-separated fields" "%0	t:0.0	bash	alpha-build" "$(pj rows | head -1)"

echo "== 3. rows: the filter is the title column, case-insensitive (R2) =="
check "BETA matches beta-server" "beta-server" "$(pj rows BETA | cut -f4)"
check "'bash' matches nothing — the command column is shown, not matched" \
      "0" "$(pj rows bash | wc -l)"
check "a miss is empty, not everything" "0" "$(pj rows zzz | wc -l)"

echo "== 4. jump: one switch-client moves session, window and pane =="
# switch-client needs an attached client, so bring up the outer server first.
tmux -L $O -f /dev/null new-session -d -x 100 -y 30 "tmux -L $V attach -t t"
OP=$(tmux -L $O display-message -p '#{pane_id}')
sleep 1
check "pj_verify has an attached client" "1" "$(tmux -L $V list-clients | wc -l)"

pj jump %1; rc=$?
check "jump %1 exits 0" "0" "$rc"
check "jump %1 makes %1 active" "%1" "$(active)"

echo "== 5. jump: an empty target is refused, never a silent no-op (R4) =="
# `tmux switch-client -t ""` exits 0 and moves nothing, so the guard is the tool's.
err=$(pj jump '' 2>&1 >/dev/null); rc=$?
contains "empty id reports on stderr" "no pane selected" "$err"
check "empty id still exits 0" "0" "$rc"
check "empty id moved nothing" "%1" "$(active)"

err=$(pj jump 2>&1 >/dev/null); rc=$?
contains "missing id reports on stderr" "no pane selected" "$err"
check "missing id still exits 0" "0" "$rc"
check "missing id moved nothing" "%1" "$(active)"

# ── the TUI, through the bound key ─────────────────────────────────────────
#
# The binding is the exact line claude.conf installs. It is bound here by hand
# because this server was started with -f /dev/null: no config, so nothing else
# can be responsible for what the key does.
echo "== 6. the popup, driven by real keystrokes through prefix g (R1/R3/R4/R5) =="
tmux -L $V bind -T prefix g display-popup -E -w 80% -h 60% -T ' pane name ' "$BIN"

keys()    { tmux -L $O send-keys -t "$OP" "$@"; sleep 0.4; }
screen()  { tmux -L $O capture-pane -p -t "$OP"; }
# C-b is pj_verify's prefix (this server has no config), and the extra sleep is
# for the popup's own startup: pane-jump runs list-panes and draws before the
# first typed character can be accepted.
open_popup() { keys C-b g; sleep 0.4; }

# Each row starts from a known, DIFFERENT pane, so "it ended on X" is evidence
# that the popup moved the client rather than that it was already there.
tmux -L $V select-pane -t %0; sleep 0.3
open_popup; keys gamma; keys Enter; sleep 0.5
check "prefix g, 'gamma', Enter -> gamma-notes" "%2" "$(active)"

tmux -L $V select-pane -t %1; sleep 0.3
open_popup; keys alpha; keys Enter; sleep 0.5
check "prefix g, 'alpha', Enter -> alpha-build" "%0" "$(active)"

# Enter with nothing typed takes the FIRST row (R3: the cursor opens there).
tmux -L $V select-pane -t %2; sleep 0.3
open_popup; keys Enter; sleep 0.5
check "prefix g, Enter with no typing -> the first row" "%0" "$(active)"

echo "== 7. a miss keeps the popup open and says so (R4) =="
tmux -L $V select-pane -t %1; sleep 0.3
open_popup; keys zzz; keys Enter
miss=$(screen)
contains "the header reports the miss" "no pane title matches" "$miss"
contains "the popup is still open" "pane name: zzz" "$miss"
check "nothing moved" "%1" "$(active)"
keys Escape; sleep 0.4
check "Escape then closes it, still nothing moved" "%1" "$(active)"
case "$(screen)" in *"pane name:"*) bad "Escape closed the popup" "no popup" "popup still drawn";;
                    *) ok "Escape closed the popup";; esac

echo "== 8. cancel is silent (R5) =="
# `show-messages` on 3.5a logs every command the server ran, so its LINE COUNT
# grows whether or not anything was printed — pane-jump's own `list-panes` and
# this harness's probes are commands too. What a printed message looks like in
# that log is a ` message: ` line (measured 2026-09-03: a `display-message -l`
# from the tool logs one `command:` line AND one `message:` line), so the
# message lines are the oracle and the command lines are noise.
said() { tmux -L $V show-messages | grep -c ' message: '; }
before=$(said)
open_popup; keys beta
contains "the query filtered to one row" "1/3" "$(screen)"
keys Escape; sleep 0.4
check "Escape moved nothing" "%1" "$(active)"
check "Escape printed no message" "$before" "$(said)"

# The control for that assertion: a message the tool DOES print has to move the
# same counter, or "no new message" would be a checker that cannot fail.
pj jump '' 2>/dev/null
sleep 0.3
check "control: a printed message moves the counter" "$((before + 1))" "$(said)"

echo
printf 'passed %d, failed %d\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
