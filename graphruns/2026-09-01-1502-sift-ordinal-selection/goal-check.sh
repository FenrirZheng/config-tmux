#!/usr/bin/env bash
# goal-check.sh — the §9 goal condition of graphruns/2026-09-01-1502-sift-ordinal-selection,
# mechanised. Run once PRE-WORK as the negative control: checks 1-2 must PASS (proving the
# check is aimed at a working target, not crashing or mis-pathed) and checks 3-6 must FAIL
# on the missing ordinal work.
#
# Never touches $TMUX or the user's real server: the suites it calls use throwaway -L sockets
# and carry their own control case.
set -u
R=$HOME/.tmux
SCRIPTS=$R/records/2026-08-27-2240-tmux-sift/assets/scripts
MAP=$R/records/2026-09-01-1330-sift-ordinal-selection/sift-ordinal-selection.org
pass=0; fail=0
ok()  { pass=$((pass+1)); printf '  PASS  %s\n' "$1"; }
bad() { fail=$((fail+1)); printf '  FAIL  %s\n        want: %s\n        got : %s\n' "$1" "$2" "$3"; }

echo "== 1. cmake --preset release builds with zero warnings =="
blog=$(cd "$R/tools/sift" && cmake --preset release >/dev/null 2>&1 && \
       cmake --build --preset release --clean-first 2>&1)
brc=$?
warn=$(printf '%s' "$blog" | grep -c 'warning:')
if [ $brc -eq 0 ] && [ "$warn" -eq 0 ]; then ok "build clean, 0 warnings"
else bad "build clean, 0 warnings" "rc=0 warnings=0" "rc=$brc warnings=$warn"; fi

echo "== 2. verify-sift-jump.sh: exit 0, >=13 passed, 0 failed =="
jlog=$(bash "$SCRIPTS/verify-sift-jump.sh" 2>&1); jrc=$?
jp=$(printf '%s' "$jlog" | sed -n 's/^passed \([0-9]*\), failed \([0-9]*\)$/\1/p' | tail -1)
jf=$(printf '%s' "$jlog" | sed -n 's/^passed \([0-9]*\), failed \([0-9]*\)$/\2/p' | tail -1)
if [ "$jrc" -eq 0 ] && [ "${jp:-0}" -ge 13 ] && [ "${jf:-1}" -eq 0 ]; then ok "jump $jp/$jf"
else bad "jump suite" "rc=0 passed>=13 failed=0" "rc=$jrc passed=${jp:-?} failed=${jf:-?}"; fi

echo "== 3. verify-sift-live.sh: exit 0, >=17 passed, 0 failed =="
llog=$(bash "$SCRIPTS/verify-sift-live.sh" 2>&1); lrc=$?
lp=$(printf '%s' "$llog" | sed -n 's/^passed \([0-9]*\), failed \([0-9]*\)$/\1/p' | tail -1)
lf=$(printf '%s' "$llog" | sed -n 's/^passed \([0-9]*\), failed \([0-9]*\)$/\2/p' | tail -1)
if [ "$lrc" -eq 0 ] && [ "${lp:-0}" -ge 17 ] && [ "${lf:-1}" -eq 0 ]; then ok "live $lp/$lf"
else bad "live suite" "rc=0 passed>=17 failed=0" "rc=$lrc passed=${lp:-?} failed=${lf:-?}"; fi

echo "== 4. >=6 assertions in the suites name ordinal mode =="
na=$(cat "$SCRIPTS/verify-sift-live.sh" "$SCRIPTS/verify-sift-jump.sh" \
     | grep -cE '^[[:space:]]*(ok|bad|check)[[:space:]]*.*[Oo]rdinal|ordinal' )
if [ "$na" -ge 6 ]; then ok "$na ordinal references in the suites"
else bad "ordinal assertions in the suites" ">=6" "$na"; fi

echo "== 5. runbooks/sift.md documents goto> and the left-Alt restriction =="
g=0; grep -q 'goto>' "$R/runbooks/sift.md" && g=$((g+1))
grep -qiE 'left[ -]?alt' "$R/runbooks/sift.md" && g=$((g+1))
if [ "$g" -eq 2 ]; then ok "runbook documents goto> and left Alt"
else bad "runbook documents goto> and left Alt" "both present" "$g/2 present"; fi

echo "== 6. t2, t3, t4 are DONE in the map =="
d=$(grep -cE '^\*\* DONE (Build ordinal mode|Assert ordinal mode|Document ordinal mode)' "$MAP")
if [ "$d" -eq 3 ]; then ok "t2/t3/t4 DONE"
else bad "t2/t3/t4 DONE in the map" "3" "$d"; fi

echo
printf 'goal-check: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
