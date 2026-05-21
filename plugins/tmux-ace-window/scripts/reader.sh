#!/usr/bin/env bash
# tmux-ace-window — keypress reader, runs inside the capture popup.
#
# Lists every pane's label, reads exactly one key, restores the borders no
# matter what (EXIT trap), and on a hit selects or swaps the chosen pane.
set -u

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=helpers.sh
source "$CURRENT_DIR/helpers.sh"

# Read the stashed state before the trap can clear it.
window_id="$(tmux show-option -qv "@ace_window_id")"
active_pane="$(tmux show-option -qv "@ace_active_pane")"
action="$(tmux show-option -qv "@ace_action")"

# Whatever happens next — match, cancel, Ctrl-C — undo the UI state.
trap ace_restore EXIT

# --- legend: the same labels that sit in the pane borders ------------------
verb="jump to"
[ "$action" = "swap" ] && verb="swap with"
printf '\n   ace-window - %s a pane\n\n' "$verb"
while IFS='|' read -r label pidx pcmd pactive; do
	[ -z "$label" ] && continue
	mark='  '
	[ "$pactive" = "1" ] && mark=' *'
	printf '    [%s]%s  pane %s   %s\n' "$label" "$mark" "$pidx" "$pcmd"
done < <(tmux list-panes -t "$window_id" \
	-F '#{@ace_label}|#{pane_index}|#{pane_current_command}|#{pane_active}')
printf '\n   press a label key   (* = current,  Esc cancels)'

# --- read one key; drain any escape-sequence tail --------------------------
key=""
IFS= read -rsn1 key
IFS= read -rsn16 -t 0.002 _tail 2>/dev/null || true

# --- find the pane whose label matches -------------------------------------
target=""
while IFS='|' read -r pane label; do
	if [ -n "$key" ] && [ -n "$label" ] && [ "$label" = "$key" ]; then
		target="$pane"
		break
	fi
done < <(tmux list-panes -t "$window_id" -F '#{pane_id}|#{@ace_label}')

# No match -> cancelled; the EXIT trap restores everything.
[ -z "$target" ] && exit 0

if [ "$action" = "swap" ]; then
	tmux swap-pane -s "$active_pane" -t "$target"
else
	tmux select-pane -t "$target"
fi
