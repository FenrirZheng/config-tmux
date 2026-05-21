# tmux-ace-window — shared helpers, sourced by the other scripts.
# Not meant to be executed directly.

# get_tmux_option <option-name> <default-value>
# Echoes the global value of a tmux option, or the default when unset/empty.
get_tmux_option() {
	local option="$1" default="$2" value
	value="$(tmux show-option -gqv "$option")"
	if [ -n "$value" ]; then
		printf '%s' "$value"
	else
		printf '%s' "$default"
	fi
}

# ace_restore — undo any in-progress ace-window UI state. Idempotent: a no-op
# when nothing is in progress, so it doubles as a stale-state cleaner. Both
# reader.sh (on exit) and ace-window.sh (on entry) call this.
ace_restore() {
	local win saved_pbs saved_pbf opt pane
	win="$(tmux show-option -qv "@ace_window_id")"
	[ -z "$win" ] && return 0   # nothing in progress

	saved_pbs="$(tmux show-option -qv "@ace_saved_pbs")"
	saved_pbf="$(tmux show-option -qv "@ace_saved_pbf")"

	# Restore the two window-scoped border options. An empty saved value means
	# "was not set window-locally" -> unset so it inherits the global again.
	if [ -n "$saved_pbs" ]; then
		tmux set-option -w -t "$win" pane-border-status "$saved_pbs" 2>/dev/null
	else
		tmux set-option -wu -t "$win" pane-border-status 2>/dev/null
	fi
	if [ -n "$saved_pbf" ]; then
		tmux set-option -w -t "$win" pane-border-format "$saved_pbf" 2>/dev/null
	else
		tmux set-option -wu -t "$win" pane-border-format 2>/dev/null
	fi

	# Drop the per-pane label options.
	while read -r pane; do
		tmux set-option -pu -t "$pane" "@ace_label" 2>/dev/null
	done < <(tmux list-panes -t "$win" -F '#{pane_id}' 2>/dev/null)

	# Drop the stashed session state.
	for opt in @ace_saved_pbs @ace_saved_pbf @ace_window_id @ace_active_pane @ace_action; do
		tmux set-option -u "$opt" 2>/dev/null
	done
}
