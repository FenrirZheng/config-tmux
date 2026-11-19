#pragma once

#include <cstdint>
#include <string>
#include <string_view>
#include <vector>

namespace win_pick {

enum class AgentState : int {
  None = 0,
  Idle = 1,
  Busy = 2,
  NeedsInput = 3,
};

struct WindowEntry {
  std::string session_id;
  std::string session_name;
  std::string window_id;
  int window_index = 0;
  std::string window_name;
  bool auto_rename = true;
  bool window_active = false;
  bool bell = false;
  int pane_count = 0;
  std::string active_pane_id;
  std::string command;
  std::string path;
  AgentState agent_state = AgentState::None;
  std::string task_hint;
  // Concatenated lowercase search corpus across all panes in this window.
  std::string search_corpus;
};

struct AnsiColor {
  enum class Kind : std::uint8_t {
    Default = 0,
    Palette16,
    Palette256,
    Rgb,
  };
  Kind kind = Kind::Default;
  std::uint8_t index = 0;
  std::uint8_t r = 0;
  std::uint8_t g = 0;
  std::uint8_t b = 0;

  [[nodiscard]] bool operator==(const AnsiColor&) const = default;
};

struct AnsiStyle {
  AnsiColor fg;
  AnsiColor bg;
  bool bold = false;
  bool dim = false;
  bool italic = false;
  bool underlined = false;
  bool inverted = false;

  [[nodiscard]] bool operator==(const AnsiStyle&) const = default;
};

struct AnsiSpan {
  std::string text;
  AnsiStyle style;
};

// Format string passed to `tmux list-panes -a -F ...`.
// 16 tab-separated fields; see parse_panes_output().
inline constexpr std::string_view kPaneListFormat =
    "#{session_id}\t#{session_name}\t#{window_id}\t#{window_index}\t"
    "#{window_name}\t#{automatic-rename}\t#{window_active}\t#{window_bell_flag}\t"
    "#{pane_id}\t#{pane_active}\t#{pane_current_command}\t#{pane_current_path}\t"
    "#{@claude_state}\t#{@claude_agent}\t#{@claude_activity}\t#{@claude_task}";

// Parse the tab-separated output of `tmux list-panes -a -F kPaneListFormat`
// into window-aggregated entries sorted by (current_session first, session_name, window_index).
[[nodiscard]] std::vector<WindowEntry> parse_panes_output(
    std::string_view raw,
    std::string_view current_session_id);

// Filter and rank windows for `query`.
// - If `all_sessions` is false, only windows in `current_session_id` are included.
// - If `query` is pure ASCII digits, exact `window_index` matches rank first,
//   then `window_index` prefix matches, then substring matches in labels/tasks.
// - Otherwise, case-insensitive substring match over window metadata.
[[nodiscard]] std::vector<std::size_t> filter_windows(
    const std::vector<WindowEntry>& windows,
    std::string_view query,
    bool all_sessions,
    std::string_view current_session_id);

// Human-readable display label for a window row:
// manual rename-window name (when auto_rename is off and not prefixed with `cc:`),
// otherwise the active pane's command (or window_name fallback).
[[nodiscard]] std::string display_label(const WindowEntry& w);

// Shorten a filesystem path by replacing a leading `home` prefix with `~`.
[[nodiscard]] std::string shorten_path(std::string_view path, std::string_view home);

// Basename of a path (returns "/" for "/", or the component after the last '/').
[[nodiscard]] std::string path_basename(std::string_view path);

// Human-readable name for AgentState (empty string for None).
[[nodiscard]] std::string_view agent_state_name(AgentState state) noexcept;

// Parse a single line from `tmux capture-pane -e -p` into styled spans.
// Preserves SGR 16/256/TrueColor and text attributes, strips non-SGR escapes
// (OSC/CSI cursor control), and expands tabs to 4-column stops.
[[nodiscard]] std::vector<AnsiSpan> parse_ansi_line(std::string_view line);

// Split raw multi-line capture output and parse each line with parse_ansi_line(),
// trimming trailing blank lines so the preview stays dense.
[[nodiscard]] std::vector<std::vector<AnsiSpan>> parse_ansi_capture(std::string_view raw);

}  // namespace win_pick
