#pragma once

#include <string>
#include <string_view>
#include <vector>

namespace tmux_bar {

// Tokyo Night palette aligned with ~/.config/alacritty/themes/tokyo-night.toml.
namespace palette {
inline constexpr std::string_view kBlack     = "#15161e";
inline constexpr std::string_view kBgBar     = "#16161e";
inline constexpr std::string_view kSurface   = "#1f2335";
inline constexpr std::string_view kSelection = "#283457";
inline constexpr std::string_view kBorder    = "#292e42";
inline constexpr std::string_view kMuted     = "#565f89";
inline constexpr std::string_view kComment   = "#737aa2";
inline constexpr std::string_view kSubtext   = "#a9b1d6";
inline constexpr std::string_view kText      = "#c0caf5";
inline constexpr std::string_view kBlue      = "#7aa2f7";
inline constexpr std::string_view kMagenta   = "#bb9af7";
inline constexpr std::string_view kRed       = "#f7768e";
inline constexpr std::string_view kOrange    = "#ff9e64";
inline constexpr std::string_view kYellow    = "#e0af68";
inline constexpr std::string_view kGreen     = "#9ece6a";
}  // namespace palette

// Minimal 1-cell status dot for tabs/fleet + Nerd Font git icon for status-right.
namespace glyph {
inline constexpr std::string_view kDot = "\u25cf";      // ● (strictly 1-cell)
inline constexpr std::string_view kGit = "\U000f02a2";  // git branch
}  // namespace glyph

enum class Scope {
  Global,        // tmux set-option -g
  GlobalWindow,  // tmux set-option -gw
};

struct TmuxOption {
  Scope scope;
  std::string name;
  std::string value;
};

// Format expression checking whether any pane in the window holds `state`.
// Passes the format twice to #{P:A,B} because A covers the active pane and B
// covers inactive panes.
std::string any_pane_state(std::string_view state);

// Coloured Claude state glyph for inactive window tabs (@claude_tab_glyph).
std::string window_glyph();

// Monochrome Claude state glyph for the current tab's filled mauve block
// (@claude_tab_glyph_mono).
std::string window_glyph_mono();

// Fleet "who needs me" counter appended to status-right.
std::string fleet_segment();

// Window tab formats: number only (#I), plus manual rename-window name (when
// automatic-rename is off and name does not start with `cc:`), plus Claude glyph.
std::string window_status_format();
std::string window_status_current_format();

// Status-left and status-right formats.
std::string status_left_format();
std::string status_right_format(std::string_view home_dir);

// Complete ordered set of tmux options owned by tmux-bar.
std::vector<TmuxOption> build_options(std::string_view home_dir);

// Format rule applied to existing windows so single-pane windows hide the
// pane-border row unless marked or recording.
std::string_view existing_window_border_rule();

// Escape a string as a tmux config double-quoted token (for `dump` output).
std::string tmux_quote(std::string_view text);

// Render `options` as a human-readable tmux config script.
std::string render_tmux_script(const std::vector<TmuxOption>& options);

}  // namespace tmux_bar
