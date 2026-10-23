#!/usr/bin/env bash
# A/B the status-bar background live: default (opaque mantle #181825) or none
# (Alacritty's 0.95 translucency shows through). Usage: bash try-background.sh none|default
# Edits the one option line in ~/.tmux/tmux.conf, then resets cc-beacon's
# status-right cache BEFORE the single reload (reload-then-unset double-appends).
set -euo pipefail
v=${1:?usage: try-background.sh none|default}
case $v in none|default) ;; *) echo "none or default" >&2; exit 2 ;; esac
sed -i -E "s/^(set -g @catppuccin_status_background )\"[a-z]+\"/\1\"$v\"/" ~/.tmux/tmux.conf
grep -n '^set -g @catppuccin_status_background' ~/.tmux/tmux.conf
tmux set -gu @claude_bar_base_sr
tmux source-file ~/.tmux.conf
echo "status bar background: $v"
