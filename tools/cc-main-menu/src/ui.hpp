// ui — the FTXUI popup: search bar, ranked list, preview / argument bar.
//
// The UI never runs a tmux command. It returns the chosen argv and exits, so
// the popup closes before scripts/launch.sh runs the command (copy-mode,
// choose-tree, command-prompt and nested popups need the client free).
#pragma once

#include <string>
#include <vector>

#include "tmux_client.hpp"

namespace ccmm {

struct UiResult {
  std::vector<std::string> argv;  // empty = cancelled, run nothing
};

// `load_error` non-empty: show it instead of a list; any key exits.
UiResult run_ui(const Catalog& catalog, const Context& ctx, const std::string& load_error);

}  // namespace ccmm
