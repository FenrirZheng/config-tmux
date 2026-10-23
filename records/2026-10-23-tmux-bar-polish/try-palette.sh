#!/usr/bin/env bash
# A/B the status-bar palette and tab information live (round four).
# Usage: bash try-palette.sh original|neutral|neutral-info|neutral-short|neutral-bright|neutral-brighter|oneblock
#
#   original       round-three state: per-module hues, grey number cell on
#                  every tab, 20-cell "…" titles
#   neutral        one accent (active tab mauve) + every module icon cell
#                  surface2 #585b70; inactive tabs lose their number cell, the
#                  index moves into the title cell, dimmed (overlay2 #9399b2, 4.45:1)
#   neutral-info   neutral + hide status-left while the session name is
#                  purely numeric + a shell tab whose title is the hostname
#                  shows its directory basename instead (~ at home)
#   neutral-short  neutral-info + titles cut at 14 cells, no ellipsis
#   neutral-bright   round five (「色調很暗」): neutral with the blocks lifted
#                  off the mantle bar — icon cells overlay1 #7f849c with base
#                  #1e1e2e glyphs, module text cells and inactive tabs surface1
#                  #45475a, inactive index subtext0 #a6adc8
#   neutral-brighter one step more: icon cells overlay2 #9399b2, inactive tabs
#                  surface2 #585b70
#   oneblock       round six (「大刀闊斧」): the current tab is the ONLY filled
#                  block (mauve #cba6f7, crust text, bold); every other element
#                  is text on the mantle bar — inactive tabs index overlay2
#                  #9399b2 + title subtext0 #a6adc8, git and directory as
#                  subtext0 icon + text #cdd6f4, roster ` N` red bold text,
#                  session text (hidden when numeric) that becomes a mauve
#                  block while prefix is armed. Smart titles, 14 cells, no
#                  ellipsis. catppuccin stays loaded for its palette only.
#
# Rewrites the block between the "# >>> palette" / "# <<< palette" markers in
# ~/.tmux/tmux.conf (above `run tpm`, where catppuccin can see it) AND the
# "# >>> bar-format" / "# <<< bar-format" block in ~/.tmux/claude.conf (after
# the theme, so `oneblock` wins over catppuccin's `set -gF` formats; empty for
# every other variant), unsets the options catppuccin only fills when unset,
# then resets cc-beacon's status-right cache BEFORE the single reload
# (reload-then-unset double-appends).
set -euo pipefail
v=${1:?usage: try-palette.sh original|neutral|neutral-info|neutral-short|neutral-bright|neutral-brighter|oneblock}
conf=~/.tmux/tmux.conf
fconf=~/.tmux/claude.conf
block=$(mktemp); fblock=$(mktemp)
trap 'rm -f "$block" "$fblock"' EXIT

# Title expressions. The hostname fallback: a plain shell leaves pane_title at
# the hostname, so two shell tabs both read "fenrir".
plain='#{s|^✳ ||:pane_title}'
# (#{b:} only takes a variable name, so home is a separate test, not #{b:#{s…}}.)
smart='#{?#{==:#{pane_title},#{host_short}},#{?#{==:#{pane_current_path},#{HOME}},~,#{b:pane_current_path}},#{s|^✳ ||:pane_title}}'

case $v in
original)
  cat >"$block" <<'B'
set -g @bar_hide_numeric_session 0
set -g @catppuccin_session_color "#{?client_prefix,#{E:@thm_red},#{E:@thm_green}}"
set -g @catppuccin_gitmux_color "#94e2d5"
set -g @catppuccin_directory_color "#f5e0dc"
set -g @catppuccin_status_session_icon_bg "#{?client_prefix,#{E:@thm_red},#{E:@thm_green}}"
set -g @catppuccin_status_gitmux_icon_bg "#94e2d5"
set -g @catppuccin_status_directory_icon_bg "#f5e0dc"
set -g @catppuccin_status_session_icon_fg "#11111b"
set -g @catppuccin_status_gitmux_icon_fg "#11111b"
set -g @catppuccin_status_directory_icon_fg "#11111b"
set -g @catppuccin_window_number "#I"
set -g @catppuccin_window_number_color "#{@thm_overlay_2}"
set -g @catppuccin_status_module_text_bg "#313244"
set -g @catppuccin_window_text_color "#313244"
set -g @catppuccin_window_current_text_color "#45475a"
set -g @catppuccin_window_text " #{=/20/…;s|^✳ ||:pane_title}#{E:@claude_tab_glyph}"
set -g @catppuccin_window_current_text " #{=/20/…;s|^✳ ||:pane_title}#{E:@claude_tab_glyph}"
B
  ;;
neutral|neutral-info|neutral-short|neutral-bright|neutral-brighter|oneblock)
  # Defaults = round four's neutral: dark blocks on the mantle bar.
  # (oneblock keeps these palette lines as inert defaults: its formats below
  # replace every catppuccin-composed string they feed.)
  hide=0; title="#{=/20/…:$plain}"
  icon_bg="#585b70"; icon_fg="#cdd6f4"; text_bg="#313244"
  tab_bg="#313244"; idx_fg="#9399b2"
  case $v in
    neutral-info)     hide=1; title="#{=/20/…:$smart}" ;;
    neutral-short)    hide=1; title="#{=14:$smart}" ;;
    neutral-bright)   icon_bg="#7f849c"; icon_fg="#1e1e2e"; text_bg="#45475a"
                      tab_bg="#45475a"; idx_fg="#a6adc8" ;;
    neutral-brighter) icon_bg="#9399b2"; icon_fg="#1e1e2e"; text_bg="#45475a"
                      tab_bg="#585b70"; idx_fg="#a6adc8" ;;
  esac
  # Session icon: neutral, but mauve (the one accent) while prefix is armed.
  # Current tab: mauve number cell + surface1 title cell in every variant.
  cat >"$block" <<B
set -g @bar_hide_numeric_session $hide
set -g @catppuccin_session_color "#{?client_prefix,#cba6f7,$icon_bg}"
set -g @catppuccin_gitmux_color "$icon_bg"
set -g @catppuccin_directory_color "$icon_bg"
set -g @catppuccin_status_session_icon_bg "#{?client_prefix,#cba6f7,$icon_bg}"
set -g @catppuccin_status_gitmux_icon_bg "$icon_bg"
set -g @catppuccin_status_directory_icon_bg "$icon_bg"
set -g @catppuccin_status_session_icon_fg "#{?client_prefix,#11111b,$icon_fg}"
set -g @catppuccin_status_gitmux_icon_fg "$icon_fg"
set -g @catppuccin_status_directory_icon_fg "$icon_fg"
set -g @catppuccin_status_module_text_bg "$text_bg"
set -g @catppuccin_window_number ""
set -g @catppuccin_window_number_color "$tab_bg"
set -g @catppuccin_window_text_color "$tab_bg"
set -g @catppuccin_window_current_text_color "#45475a"
set -g @catppuccin_window_text "#[fg=$idx_fg]#I#[fg=#cdd6f4] $title#{E:@claude_tab_glyph}"
set -g @catppuccin_window_current_text " $title#{E:@claude_tab_glyph}"
B
  ;;
*) echo "original, neutral, neutral-info, neutral-short, neutral-bright, neutral-brighter or oneblock" >&2; exit 2 ;;
esac

# claude.conf bar-format block: formats only for oneblock. The older variants
# write nothing and restore tmux's default window-status-separator; catppuccin
# re-sets window-status-format / -current-format itself on every reload
# (`set -gF`, unconditional), and claude.conf's own composition lines above
# the markers re-set status-left / status-right.
if [ "$v" = oneblock ]; then
  title="#{=14:$smart}"
  cat >"$fblock" <<B
set -g window-status-separator "  "
set -g window-status-format "#[fg=#9399b2]#I #[fg=#a6adc8]$title#{E:@claude_tab_glyph}#[default]"
set -g window-status-current-format "#[fg=#11111b]#[bg=#cba6f7]#[bold] #I $title#{E:@claude_tab_glyph_mono} #[default]"
set -g status-left "#{?client_prefix,#[fg=#11111b]#[bg=#cba6f7]#[bold] #S #[default] ,#{?#{m/r:^[0-9]+\$,#{session_name}},#[fg=#9399b2] ,#[fg=#9399b2] #S  }}"
set -g status-right "#[fg=#a6adc8]󰊢 #[fg=#cdd6f4]#(gitmux -cfg ~/.gitmux.conf '#{pane_current_path}')   #[fg=#a6adc8] #[fg=#cdd6f4]#{s|$HOME|~|:pane_current_path}  "
B
else
  echo 'set -gu window-status-separator' >"$fblock"
fi

grep -q '^# >>> palette' "$conf" && grep -q '^# <<< palette' "$conf" ||
  { echo "palette markers missing in $conf" >&2; exit 1; }
awk -v f="$block" '
  /^# >>> palette/ { print; while ((getline l < f) > 0) print l; skip=1; next }
  /^# <<< palette/ { skip=0 }
  !skip
' "$conf" >"$conf.tmp" && mv "$conf.tmp" "$conf"

grep -q '^# >>> bar-format' "$fconf" && grep -q '^# <<< bar-format' "$fconf" ||
  { echo "bar-format markers missing in $fconf" >&2; exit 1; }
awk -v f="$fblock" '
  /^# >>> bar-format/ { print; while ((getline l < f) > 0) print l; skip=1; next }
  /^# <<< bar-format/ { skip=0 }
  !skip
' "$fconf" >"$fconf.tmp" && mv "$fconf.tmp" "$fconf"

tmux set -gu @claude_bar_base_sr
tmux source-file ~/.tmux.conf
echo "status-bar palette: $v"
