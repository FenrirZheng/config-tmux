#!/usr/bin/env bash
# tmux-ace-window — jump to (or swap) tmux panes by single-key labels, in the
# spirit of Emacs ace-window.
#
# Install (plain): add to ~/.tmux.conf
#   run-shell ~/.tmux/plugins/tmux-ace-window/ace-window.tmux
# Install (TPM):   set -g @plugin 'fenrir/tmux-ace-window'
#
# See README.md for the configurable @ace-window-* options.

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPTS_DIR="$CURRENT_DIR/scripts"
# shellcheck source=scripts/helpers.sh
source "$SCRIPTS_DIR/helpers.sh"

# display-popup (the keypress capture) needs tmux >= 3.2.
version="$(tmux -V | sed -E 's/^tmux //; s/[^0-9.].*$//')"
if ! awk -v v="$version" 'BEGIN { exit !(v + 0 >= 3.2) }'; then
	tmux display-message "tmux-ace-window: needs tmux >= 3.2 (found ${version})"
	exit 0
fi

jump_key="$(get_tmux_option "@ace-window-key" "a")"
swap_key="$(get_tmux_option "@ace-window-swap-key" "A")"

# prefix + <key>  -> jump to a pane;  prefix + <swap_key> -> swap with a pane.
tmux bind-key "$jump_key" run-shell -b "$SCRIPTS_DIR/ace-window.sh select"
tmux bind-key "$swap_key" run-shell -b "$SCRIPTS_DIR/ace-window.sh swap"
