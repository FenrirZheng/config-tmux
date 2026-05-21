#!/usr/bin/env bash
# tmux-ace-window — orchestrator.
#
# Usage: ace-window.sh <select|swap>
#
# Labels every pane in the current window with a single key, paints those
# labels into the pane borders, then opens a small popup (reader.sh) that
# captures one keypress and performs the action.
set -u

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=helpers.sh
source "$CURRENT_DIR/helpers.sh"

action="${1:-select}"

# --- options --------------------------------------------------------------
keys="$(get_tmux_option "@ace-window-keys" "a s d f g h j k l q w e r t y u i o p z x c v b n m")"
label_bg="$(get_tmux_option "@ace-window-label-bg" "red")"
label_fg="$(get_tmux_option "@ace-window-label-fg" "white")"
shortcut_2="$(get_tmux_option "@ace-window-jump-on-2" "on")"

# Clear any state a previous (interrupted) run might have left behind.
ace_restore

window_id="$(tmux display-message -p '#{window_id}')"
active_pane="$(tmux display-message -p '#{pane_id}')"

mapfile -t panes < <(tmux list-panes -t "$window_id" -F '#{pane_id}')
count="${#panes[@]}"

# 0 or 1 pane: nothing to choose.
if [ "$count" -le 1 ]; then
	tmux display-message "ace-window: only one pane in this window"
	exit 0
fi

# Exactly 2 panes: skip the UI and act on "the other" pane directly.
if [ "$count" -eq 2 ] && [ "$shortcut_2" = "on" ]; then
	for p in "${panes[@]}"; do
		[ "$p" = "$active_pane" ] && continue
		if [ "$action" = "swap" ]; then
			tmux swap-pane -s "$active_pane" -t "$p"
		else
			tmux select-pane -t "$p"
		fi
		exit 0
	done
fi

# --- 3+ panes (or shortcut disabled): label panes and capture a key -------

# Stash state so reader.sh can act, and ace_restore can undo, after the popup.
tmux set-option "@ace_saved_pbs" "$(tmux show-option -wqv pane-border-status)"
tmux set-option "@ace_saved_pbf" "$(tmux show-option -wqv pane-border-format)"
tmux set-option "@ace_window_id" "$window_id"
tmux set-option "@ace_active_pane" "$active_pane"
tmux set-option "@ace_action" "$action"

# Assign one label per pane, in pane order. Panes past the end of the key
# list stay unlabelled and simply are not selectable this round.
read -ra key_list <<< "$keys"
idx=0
for p in "${panes[@]}"; do
	label="${key_list[$idx]:-}"
	[ -z "$label" ] && break
	tmux set-option -p -t "$p" "@ace_label" "$label"
	idx=$((idx + 1))
done

# Paint each label as a centred badge in that pane's top border. The #[...]
# style directives are kept comma-free (#[bg=x]#[fg=y]#[bold], NOT the form
# #[bg=x,fg=y,bold]) on purpose: a comma inside #[...] is otherwise swallowed
# by the #{?...,...} argument separator and the whole conditional mis-parses.
label_style="#[bg=${label_bg}]#[fg=${label_fg}]#[bold]"
tmux set-option -w -t "$window_id" pane-border-status top
tmux set-option -w -t "$window_id" pane-border-format \
	"#{?@ace_label,#[align=centre]${label_style}  #{@ace_label}  #[default],}"

# Capture one keypress in a popup. reader.sh also draws a legend of every
# label inside it, so size the popup to the pane count and park it low so it
# does not cover the top-border badges.
popup_h=$(( count + 8 ))
popup_w=48
height="$(tmux display-message -p '#{client_height}')"
y=$(( ${height:-0} - popup_h - 1 ))
if [ "$y" -ge 1 ]; then
	tmux display-popup -E -w "$popup_w" -h "$popup_h" -x C -y "$y" "$CURRENT_DIR/reader.sh"
else
	tmux display-popup -E -w "$popup_w" -h "$popup_h" "$CURRENT_DIR/reader.sh"
fi
