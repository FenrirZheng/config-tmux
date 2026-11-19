#!/usr/bin/env bash
# TPM-style entry point for tmux-zhuyin (pure-terminal Zhuyin IME popup).
#
#   run-shell "~/.tmux/tools/tmux-zhuyin/tmux-zhuyin.tmux"
#
# Options (set before the run-shell line):
#   @tmux-zhuyin-table   key table                 (default: root — no prefix needed)
#   @tmux-zhuyin-key     key in that table         (default: C-\)
#   @tmux-zhuyin-width   popup width               (default: 68)
#   @tmux-zhuyin-height  popup height              (default: 11)
#   @tmux-zhuyin-kb      keyboard layout           (default: default)
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
bin="$here/../target/release/tmux-zhuyin"

opt() { # opt NAME DEFAULT
  local v
  v=$(tmux show-option -gqv "$1")
  printf '%s' "${v:-$2}"
}

table=$(opt @tmux-zhuyin-table root)
key=$(opt @tmux-zhuyin-key 'C-\')
width=$(opt @tmux-zhuyin-width 68)
height=$(opt @tmux-zhuyin-height 11)
kb=$(opt @tmux-zhuyin-kb default)

if [[ -x $bin ]]; then
  # display-popup does NOT expand #{...} formats in its shell-command, so capture
  # client/pane via run-shell -b and pass them as plain argv to launch.sh.
  tmux bind-key -N 'Zhuyin IME: type Traditional Chinese into this pane (tmux-zhuyin)' \
    -T "$table" "$key" run-shell -b \
    "$(printf '%q' "$here/scripts/launch.sh") '#{client_name}' '#{pane_id}' $(printf '%q %q %q' "$width" "$height" "$kb")"
  exit 0
fi

build_cmd="$here/scripts/build.sh"
tmux bind-key -T "$table" "$key" display-message -l \
  "tmux-zhuyin: not built — run $build_cmd && tmux source-file ~/.tmux.conf"
