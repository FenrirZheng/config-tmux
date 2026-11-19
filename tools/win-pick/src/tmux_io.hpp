#pragma once

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "model.hpp"

namespace win_pick {

struct Inventory {
  std::string current_session_id;
  std::string current_window_id;
  std::vector<WindowEntry> windows;
};

// Run tmux with `args` (prepends `-L socket` when `socket` is non-empty) and
// return its stdout, or std::nullopt on non-zero exit.
[[nodiscard]] std::optional<std::string> tmux_out(const std::vector<std::string>& args,
                                                  std::string_view socket = {});

// Run tmux with `args` and return true iff exit status is 0.
[[nodiscard]] bool tmux_run(const std::vector<std::string>& args,
                            std::string_view socket = {});

// Fetch current session/window ids and all windows across the tmux server.
[[nodiscard]] std::optional<Inventory> fetch_inventory(std::string_view socket = {});

// Capture the visible screen of `pane_id` with ANSI escape sequences preserved.
[[nodiscard]] std::string capture_pane_preview(std::string_view pane_id,
                                               std::string_view socket = {});

// Switch tmux to `window_id` (uses `select-window` when `same_session`, else `switch-client`).
[[nodiscard]] bool jump_to_window(std::string_view window_id,
                                  bool same_session,
                                  std::string_view socket = {});

}  // namespace win_pick
