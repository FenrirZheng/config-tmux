#!/usr/bin/env bash
# launch.sh CLIENT SESSION WINDOW PANE WIDTH HEIGHT
#
# Called by the key binding through `run-shell -b`, which has already expanded
# the #{…} formats (display-popup would not). Two steps, in this order:
#
#   1. Open the popup on the calling client. cc-main-menu writes the chosen tmux
#      argv to a temp file, NUL-delimited, and exits 0 — the popup closes.
#      `tmux display-popup` returns only when the popup has closed (measured on
#      3.5a), so nothing below runs while the popup still holds the client.
#   2. Run that argv as `tmux "${argv[@]}"` — no shell, no eval. A non-zero exit
#      is shown on the client with display-message -l (literal: tmux's error
#      text never reaches a format context).
#
# CC_MAIN_MENU_BIN overrides the binary (the verify harness points it at the
# ASan twin).
set -uo pipefail

client=$1 session=$2 window=$3 pane=$4 width=$5 height=$6
tools=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
bin=${CC_MAIN_MENU_BIN:-$tools/target/release/cc-main-menu}

out=$(mktemp "${TMPDIR:-/tmp}/cc-main-menu.XXXXXX") || exit 0
errf=$(mktemp "${TMPDIR:-/tmp}/cc-main-menu-err.XXXXXX") || { rm -f -- "$out"; exit 0; }
trap 'rm -f -- "$out" "$errf"' EXIT

cmd=$(printf '%q ' "$bin" --client "$client" --session "$session" \
  --window "$window" --pane "$pane" --out "$out")
tmux display-popup -E -c "$client" -t "$pane" -w "$width" -h "$height" \
  -T ' main menu ' "$cmd" || true

[[ -s $out ]] || exit 0            # cancelled
mapfile -d '' -t argv < "$out"
(( ${#argv[@]} )) || exit 0

if ! tmux "${argv[@]}" 2>"$errf"; then
  msg=$(tr '\n' ' ' < "$errf" | head -c 300)
  tmux display-message -c "$client" -l "cc-main-menu: ${argv[0]}: ${msg:-failed}"
fi
exit 0
