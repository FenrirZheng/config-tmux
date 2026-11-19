#include <cstdlib>
#include <iostream>
#include <string>
#include <string_view>
#include <vector>

#include "model.hpp"
#include "tmux_io.hpp"
#include "ui.hpp"

namespace {

void print_rows(const win_pick::Inventory& inv,
                std::string_view query,
                bool all_sessions) {
  const auto indices =
      win_pick::filter_windows(inv.windows, query, all_sessions, inv.current_session_id);
  for (std::size_t idx : indices) {
    const auto& w = inv.windows[idx];
    std::cout << w.window_id << '\t'
              << w.session_name << ':' << w.window_index << '\t'
              << win_pick::display_label(w) << '\t'
              << win_pick::agent_state_name(w.agent_state) << '\t'
              << w.pane_count << '\n';
  }
}

}  // namespace

int main(int argc, char* argv[]) {
  std::string socket;
  std::vector<std::string_view> positional;

  for (int i = 1; i < argc; ++i) {
    const std::string_view arg(argv[i]);
    if (arg == "-L" && i + 1 < argc) {
      socket = argv[++i];
    } else {
      positional.push_back(arg);
    }
  }

  // Headless subcommand: `win-pick rows [-L sock] [--all] [query]`
  if (!positional.empty() && positional[0] == "rows") {
    bool all_sessions = false;
    std::string query;
    for (std::size_t i = 1; i < positional.size(); ++i) {
      if (positional[i] == "--all") {
        all_sessions = true;
      } else if (query.empty()) {
        query = std::string(positional[i]);
      } else {
        query.push_back(' ');
        query.append(positional[i]);
      }
    }
    auto inv = win_pick::fetch_inventory(socket);
    if (!inv) return 0;
    print_rows(*inv, query, all_sessions);
    return 0;
  }

  // Headless subcommand: `win-pick jump [-L sock] <window-id>`
  if (!positional.empty() && positional[0] == "jump") {
    if (positional.size() < 2) return 0;
    auto inv = win_pick::fetch_inventory(socket);
    bool same_session = true;
    if (inv) {
      for (const auto& w : inv->windows) {
        if (w.window_id == positional[1]) {
          same_session = (w.session_id == inv->current_session_id);
          break;
        }
      }
    }
    (void)win_pick::jump_to_window(positional[1], same_session, socket);
    return 0;
  }

  // Interactive FTXUI popup mode (always exit 0 to avoid tmux error popups).
  auto inv = win_pick::fetch_inventory(socket);
  if (!inv || inv->windows.empty()) return 0;

  const char* home_env = std::getenv("HOME");
  const std::string_view home = home_env ? std::string_view(home_env) : std::string_view{};

  const win_pick::UiChoice choice = win_pick::run_ui(std::move(*inv), home, socket);
  if (choice.confirmed) {
    (void)win_pick::jump_to_window(choice.window_id, choice.same_session, socket);
  }
  return 0;
}
