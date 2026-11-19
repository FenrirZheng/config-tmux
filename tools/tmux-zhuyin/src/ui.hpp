#pragma once

#include <optional>
#include <string>

#include "ime_session.hpp"

namespace zhuyin {

struct UiOptions {
  std::string target_pane;
};

// Runs the interactive FTXUI popup loop.
// Returns the submitted UTF-8 text, or std::nullopt if cancelled.
std::optional<std::string> run_popup_ui(ImeSession& session, const UiOptions& opts);

}  // namespace zhuyin
