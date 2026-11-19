#pragma once

#include <string>
#include <string_view>

#include "tmux_io.hpp"

namespace win_pick {

struct UiChoice {
  bool confirmed = false;
  std::string window_id;
  bool same_session = true;
};

// Launch the interactive FTXUI window picker popup.
[[nodiscard]] UiChoice run_ui(Inventory inventory,
                              std::string_view home_dir,
                              std::string_view socket = {});

}  // namespace win_pick
