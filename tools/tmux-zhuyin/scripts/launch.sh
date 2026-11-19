#!/usr/bin/env bash
# launch.sh CLIENT PANE WIDTH HEIGHT KB_LAYOUT
#
# Two-hop delivery (same pattern as cc-main-menu):
#   1. Open FTXUI popup on calling client; binary writes final UTF-8 text to a
#      temp file and exits 0, closing the popup.
#   2. After popup closes, deliver text to the origin pane via a private named
#      buffer + bracketed paste (`paste-buffer -p -d`), never touching the user's
#      paste buffer and never pressing Enter.
set -uo pipefail

client=$1 pane=$2 width=$3 height=$4 kb=${5:-default}
tools=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
bin=${TMUX_ZHUYIN_BIN:-$tools/target/release/tmux-zhuyin}

out=$(mktemp "${TMPDIR:-/tmp}/tmux-zhuyin.XXXXXX") || exit 0
trap 'rm -f -- "$out"' EXIT

cmd=$(printf '%q ' "$bin" --pane "$pane" --kb "$kb" --out "$out")
tmux display-popup -E -c "$client" -t "$pane" -w "$width" -h "$height" \
  -T ' 注音輸入 ' "$cmd" || true

[[ -s $out ]] || exit 0

buf="zhuyin-$$"
if ! tmux load-buffer -b "$buf" "$out" 2>/dev/null; then
  tmux display-message -c "$client" -l "tmux-zhuyin: failed to load buffer"
  exit 0
fi

if ! tmux paste-buffer -p -d -b "$buf" -t "$pane" 2>/dev/null; then
  tmux delete-buffer -b "$buf" 2>/dev/null || true
  tmux display-message -c "$client" -l "tmux-zhuyin: failed to paste into $pane"
fi
exit 0
