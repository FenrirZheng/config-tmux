#!/usr/bin/env bash
# verify.sh — load-bearing assertions for cc-main-menu, on throwaway servers.
#
#   groups 1-5   headless: the `rows` / `resolve` seams (parsers, badge join,
#                fuzzy order, -t derivation, key replay argv). No TUI.
#   groups 6-9   end to end: prefix m on a real client opens the popup, keys are
#                typed into it, and the effect is read back from tmux.
#
# Two servers, never the user's:
#   -L ccmm_verify   server under test, `-f /dev/null`, entry script sourced.
#   -L ccmm_outer    one pane running `tmux -L ccmm_verify attach`: the terminal
#                    of the client under test. Keys for a PREFIX binding must
#                    arrive at a client's terminal; send-keys into a ccmm_verify
#                    pane would bypass the key table.
# $TMUX is pointed at the throwaway socket for the headless calls, otherwise
# `rows` would quietly read the user's server (group 1 is the control).
#
#   bash scripts/verify.sh                                  exit 0 = all passed
#   CC_MAIN_MENU=…/cc-main-menu-asan CCMM_SAN_LOG=DIR bash scripts/verify.sh
#        sanitizer twin; reports from inside popups land in DIR (must be empty
#        afterwards). Headless calls report on stderr as usual.
set -u
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
BIN=${CC_MAIN_MENU:-$here/../target/release/cc-main-menu}
V=ccmm_verify
O=ccmm_outer
pass=0 fail=0
ok()   { pass=$((pass + 1)); printf '  ok   %s\n' "$1"; }
bad()  { fail=$((fail + 1)); printf '  FAIL %s\n     want: %s\n     got : %s\n' "$1" "$2" "$3"; }
check(){ [ "$2" = "$3" ] && ok "$1" || bad "$1" "$2" "$3"; }
contains()    { case "$3" in *"$2"*) ok "$1" ;; *) bad "$1" "*$2*" "$3" ;; esac; }
not_contains(){ case "$3" in *"$2"*) bad "$1" "no $2" "$3" ;; *) ok "$1" ;; esac; }

[ -x "$BIN" ] || { echo "cc-main-menu not built at $BIN"; exit 1; }

tmux -L $V kill-server 2>/dev/null
tmux -L $O kill-server 2>/dev/null
trap 'tmux -L $V kill-server 2>/dev/null; tmux -L $O kill-server 2>/dev/null' EXIT

# ── fixture ────────────────────────────────────────────────────────────────
tmux -L $V -f /dev/null new-session -d -s t -x 120 -y 34
tmux -L $V bind -N 'zz-verify-note' -T prefix F11 'display-message zz'
tmux -L $V bind -N 'zz-verify-hit' -T prefix F9 'set -g @hit yes'
tmux -L $V bind -N 'zz-nested-popup' -T prefix F8 display-popup -E -T ' zz nested ' 'read x'
tmux -L $V bind -N 'zz-menu' -T prefix F7 display-menu -T zzmenu one 1 'set -g @menu 1'
SOCK=$(tmux -L $V display-message -p '#{socket_path}')
S=$(tmux -L $V display-message -p '#{session_id}')
P0=$(tmux -L $V display-message -p '#{pane_id}')
mm() { TMUX="$SOCK,0,0" "$BIN" "$@"; }
vt() { tmux -L $V "$@"; }
field() { awk -F'\t' -v l="$1" -v f="$2" '$2==l {print $f; exit}'; }

echo "1. control: the seam reads the throwaway server"
rows=$(mm rows)
contains "noted key from ccmm_verify is listed" $'key\tzz-verify-note\tprefix F11' "$rows"

echo "2. list-commands parsing"
check "one cmd row per tmux command" "$(vt list-commands | wc -l)" \
  "$(printf '%s\n' "$rows" | grep -c $'^cmd\t')"
check "alias parsed" "splitw" "$(printf '%s\n' "$rows" | field split-window 3)"
check "command without alias" "" "$(printf '%s\n' "$rows" | field choose-buffer 3)"
contains "command with empty usage kept" $'cmd\tkill-server' "$rows"

echo "3. badge join"
b=$(printf '%s\n' "$rows" | field split-window 4)
contains "prefix \" badges split-window" 'prefix "' "$b"
contains "prefix % badges split-window" 'prefix %' "$b"
not_contains "menu items (prefix >) are not badges" 'prefix >' "$b"
not_contains "mouse keys are not badges" 'Mouse' "$b"
contains "wrapper argument badged (prefix x: confirm-before … kill-pane)" "prefix x" \
  "$(printf '%s\n' "$rows" | field kill-pane 4)"

echo "4. fuzzy order"
check "splitw → split-window first" "split-window" "$(mm rows splitw | head -1 | cut -f2)"
check "two terms narrow to the note" "zz-verify-note" "$(mm rows zz verify note | head -1 | cut -f2)"
check "no match → no rows" "0" "$(mm rows qqxqzz | wc -l)"
check "key badge is searchable" "zz-verify-note" "$(mm rows 'prefix F11' | head -1 | cut -f2)"

echo "5. -t derivation and replay argv"
check "new-window → \$session:" "new-window -t $S:" "$(mm resolve new-window | paste -sd' ')"
check "kill-pane → origin pane" "kill-pane -t $P0" "$(mm resolve kill-pane | paste -sd' ')"
check "alias + flags, -t inserted after the name" "split-window -t $P0 -h" \
  "$(mm resolve 'splitw -h' | paste -sd' ')"
check "user -t wins" "split-window -t %9 -h" "$(mm resolve 'split-window -t %9 -h' | paste -sd' ')"
check "no target in usage → nothing added" "list-commands" "$(mm resolve list-commands | paste -sd' ')"
check "-c target-client filled from --client" "display-message -t $P0 -c /dev/pts/X hi" \
  "$(mm --client /dev/pts/X resolve 'display-message hi' | paste -sd' ')"
check "quoted arg kept whole" "#{pane_current_path}" \
  "$(mm resolve 'split-window -c "#{pane_current_path}"' | tail -1)"
mm resolve bogus-cmd >/dev/null 2>&1; check "unknown command → exit 1" "1" "$?"
check "{ } group folded into one argument" "rename-session %%" \
  "$(mm resolve 'command-prompt -I x { rename-session "%%" }' | tail -1)"
check "noted key → send-keys -K through the prefix" \
  "send-keys -K -c /dev/pts/X C-b F11" \
  "$(mm --client /dev/pts/X rows zz-verify-note | head -1 | cut -f5)"
check "key ; is escaped for the command line" "send-keys -K -c /dev/pts/X C-b '\\;'" \
  "$(mm --client /dev/pts/X rows | awk -F'\t' '$3=="prefix ;" {print $5; exit}')"

# ── end to end ─────────────────────────────────────────────────────────────
vt set -g @cc-main-menu-key m
# A wrapper records each popup run's exit status: with -E the popup closes on
# any exit, so "it closed" says nothing about Ctrl-C exiting 0.
WRAP=$(mktemp -d)
trap 'tmux -L $V kill-server 2>/dev/null; tmux -L $O kill-server 2>/dev/null; rm -rf "$WRAP"' EXIT
printf '#!/bin/sh\n"%s" "$@"\nrc=$?\necho $rc >> "%s/rc"\nexit $rc\n' "$BIN" "$WRAP" > "$WRAP/bin"
chmod +x "$WRAP/bin"
last_rc() { tail -1 "$WRAP/rc" 2>/dev/null; }
vt set-environment -g CC_MAIN_MENU_BIN "$WRAP/bin"
# Sanitizer reports from inside the popup would vanish with it; CCMM_SAN_LOG
# (a directory) makes them land on disk, one file per process.
if [ -n "${CCMM_SAN_LOG:-}" ]; then
  vt set-environment -g ASAN_OPTIONS "log_path=$CCMM_SAN_LOG/asan"
  vt set-environment -g UBSAN_OPTIONS "log_path=$CCMM_SAN_LOG/ubsan:print_stacktrace=1"
fi
TMUX="$SOCK,0,0" bash "$here/cc-main-menu.tmux"
tmux -L $O -f /dev/null new-session -d -s o -x 130 -y 40 "TMUX= tmux -L $V attach -t t"
sleep 0.5
CL=$(vt list-clients -F '#{client_name}' | head -1)

screen() { tmux -L $O capture-pane -p -t o; }
wait_screen() { # wait_screen TEXT [present|absent]
  for _ in $(seq 1 30); do
    case "$(screen)" in *"$1"*) [ "${2:-present}" = present ] && return 0 ;;
                         *) [ "${2:-present}" = absent ] && return 0 ;; esac
    sleep 0.1
  done
  return 1
}
# Wait for the UI's own footer, not the popup title: tmux draws the title before
# the binary runs, and a key sent then (C-c especially) reaches a terminal that
# still has ISIG on.
open_menu() { tmux -L $O send-keys -t o C-b m; wait_screen 'Tab edit args'; }
type_q()    { tmux -L $O send-keys -t o -l -- "$1"; }
key()       { tmux -L $O send-keys -t o "$@"; }
closed()    { wait_screen ' main menu ' absent; }
wait_eq() { # wait_eq WANT CMD... — poll until CMD prints WANT (≤3 s), then print it
  local got
  for _ in $(seq 1 30); do got=$("${@:2}" 2>/dev/null); [ "$got" = "$1" ] && break; sleep 0.1; done
  printf '%s' "$got"
}
nwin()      { vt list-windows -t t | wc -l; }
npane()     { vt list-panes -t t | wc -l; }

echo "6. prefix m opens the popup; Esc / C-c run nothing"
open_menu; check "popup opened on prefix m" 0 $?
w0=$(nwin); key Escape; closed; check "Esc closes the popup" 0 $?
check "Esc ran nothing" "$w0" "$(nwin)"
check "Esc exits 0" 0 "$(wait_eq 0 last_rc)"
: > "$WRAP/rc"
open_menu; key C-c; closed; check "C-c closes the popup" 0 $?
check "C-c ran nothing" "$w0" "$(nwin)"
check "C-c exits 0 (ISIG off, not SIGINT's 130)" 0 "$(wait_eq 0 last_rc)"
not_contains "C-c leaves no error message" "cc-main-menu:" "$(screen)"

echo "7. Enter runs after the popup has closed"
open_menu; type_q new-window; key Enter; closed
check "new-window created one window" "$((w0 + 1))" "$(wait_eq "$((w0 + 1))" nwin)"
open_menu; type_q copy-mode; key Enter; closed
check "copy-mode entered (needs the client free)" 1 \
  "$(wait_eq 1 vt display -p -t t '#{pane_in_mode}')"
vt send-keys -t t -X cancel

echo "8. Tab → argument bar; errors reach the status line"
p0=$(npane)
open_menu; type_q split-window; key Tab; type_q '-h'; key Enter; closed
check "split-window -h via the argument bar" "$((p0 + 1))" "$(wait_eq "$((p0 + 1))" npane)"
left=$(vt display -p -t t '#{pane_left}')
[ "$left" -gt 0 ] && ok "-h reached tmux: the new pane sits right (pane_left $left)" \
  || bad "-h reached tmux: the new pane sits right" "pane_left > 0" "$left"
open_menu; type_q select-window; key Tab; type_q '-t @999'; key Enter; closed
wait_screen 'cc-main-menu: select-window:'; check "tmux stderr shown via display-message" 0 $?

echo "9. noted keys replay through the key table"
vt set -gu @hit
open_menu; type_q zz-verify-hit; key Enter; closed
check "noted prefix F9 ran its binding" yes "$(wait_eq yes vt show -gv @hit)"
before=$(vt display -p -t t '#{pane_id}')
open_menu; type_q 'previously active pane'; key Enter; closed
for _ in $(seq 1 30); do after=$(vt display -p -t t '#{pane_id}'); [ "$after" != "$before" ] && break; sleep 0.1; done
[ "$before" != "$after" ] && ok "prefix ; (escaped \\;) replayed last-pane" \
  || bad "prefix ; (escaped \\;) replayed last-pane" "pane != $before" "$after"

echo "10. output, nested popups and menus work once the popup is gone"
open_menu; type_q list-commands; key Enter; closed
check "list-commands output shown in view mode" 1 "$(wait_eq 1 vt display -p -t t '#{pane_in_mode}')"
contains "its output is on screen" "attach-session" "$(screen)"
vt send-keys -t t -X cancel
open_menu; type_q zz-nested-popup; key Enter; closed
wait_screen ' zz nested '; check "a noted display-popup key opens its popup" 0 $?
key Enter; wait_screen ' zz nested ' absent
open_menu; type_q zz-menu; key Enter; closed
wait_screen 'zzmenu'; check "a noted display-menu key opens its menu" 0 $?
key 1; check "the menu item ran" 1 "$(wait_eq 1 vt show -gv @menu)"

echo "11. Tab on a stock binding with a { } block (prefix \$)"
open_menu; type_q 'Rename current session'; key Tab; key Enter; closed
wait_screen '(rename-session)'; check "command-prompt opened from the edited line" 0 $?
key BSpace; type_q zzname; key Enter
check "and its { rename-session } ran" zzname "$(wait_eq zzname vt display -p -t "$S" '#{session_name}')"

echo
echo "client $CL — passed $pass, failed $fail"
[ "$fail" -eq 0 ]
