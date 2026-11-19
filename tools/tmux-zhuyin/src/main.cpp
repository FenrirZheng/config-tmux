#include <fstream>
#include <iostream>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "chewing_engine.hpp"
#include "ime_session.hpp"
#include "ui.hpp"

namespace {

using namespace zhuyin;

void print_usage(std::ostream& os) {
  os << "usage: tmux-zhuyin [--out FILE] [--pane PANE_ID] [--kb LAYOUT] [--eval KEYS]\n"
     << "  --kb LAYOUT   default|hsu|et|et26|dvorak|ginyieh|dachen_cp26\n"
     << "  --eval KEYS   headless mode: feed key sequence (e.g. '5j/ jp6<Enter><Enter>')\n";
}

// Parses a headless key sequence like "5j/ jp6<Enter><Enter>" into KeyEvents.
std::vector<KeyEvent> parse_eval_keys(std::string_view spec) {
  std::vector<KeyEvent> events;
  std::size_t i = 0;
  while (i < spec.size()) {
    if (spec[i] == '<') {
      const auto close = spec.find('>', i + 1);
      if (close != std::string_view::npos) {
        const auto token = spec.substr(i + 1, close - i - 1);
        i = close + 1;
        if (token == "Enter" || token == "CR") {
          events.push_back({KeyKind::Enter, 0});
        } else if (token == "Space") {
          events.push_back({KeyKind::Space, ' '});
        } else if (token == "Esc") {
          events.push_back({KeyKind::Escape, 0});
        } else if (token == "BS" || token == "Backspace") {
          events.push_back({KeyKind::Backspace, 0});
        } else if (token == "Del" || token == "Delete") {
          events.push_back({KeyKind::Delete, 0});
        } else if (token == "Left") {
          events.push_back({KeyKind::Left, 0});
        } else if (token == "Right") {
          events.push_back({KeyKind::Right, 0});
        } else if (token == "Up") {
          events.push_back({KeyKind::Up, 0});
        } else if (token == "Down") {
          events.push_back({KeyKind::Down, 0});
        } else if (token == "Home") {
          events.push_back({KeyKind::Home, 0});
        } else if (token == "End") {
          events.push_back({KeyKind::End, 0});
        } else if (token == "PgUp") {
          events.push_back({KeyKind::PageUp, 0});
        } else if (token == "PgDn") {
          events.push_back({KeyKind::PageDown, 0});
        } else if (token == "Tab") {
          events.push_back({KeyKind::Tab, 0});
        } else if (token == "C-s") {
          events.push_back({KeyKind::SubmitNow, 0});
        } else if (token == "C-g" || token == "C-c") {
          events.push_back({KeyKind::CancelNow, 0});
        } else if (token == "F6" || token == "C-f") {
          events.push_back({KeyKind::ToggleFullshape, 0});
        } else if (token == "C-\\") {
          events.push_back({KeyKind::ToggleChinese, 0});
        }
        continue;
      }
    }
    const char c = spec[i++];
    if (c == ' ') {
      events.push_back({KeyKind::Space, ' '});
    } else {
      events.push_back({KeyKind::Char, static_cast<unsigned char>(c)});
    }
  }
  return events;
}

bool write_output(const std::optional<std::string>& out_path, const std::string& text) {
  if (!out_path) {
    std::cout << text;
    std::cout.flush();
    return static_cast<bool>(std::cout);
  }
  std::ofstream ofs(*out_path, std::ios::binary | std::ios::trunc);
  if (!ofs) return false;
  ofs.write(text.data(), static_cast<std::streamsize>(text.size()));
  return static_cast<bool>(ofs);
}

}  // namespace

int main(int argc, char** argv) {
  std::optional<std::string> out_path;
  std::optional<std::string> eval_keys;
  std::string pane_id;
  KeyboardLayout layout = KeyboardLayout::Default;

  for (int i = 1; i < argc; ++i) {
    const std::string_view arg = argv[i];
    if (arg == "-h" || arg == "--help") {
      print_usage(std::cout);
      return 0;
    }
    if (arg == "--out" && i + 1 < argc) {
      out_path = argv[++i];
    } else if (arg == "--pane" && i + 1 < argc) {
      pane_id = argv[++i];
    } else if (arg == "--kb" && i + 1 < argc) {
      const auto parsed = parse_keyboard_layout(argv[++i]);
      if (!parsed) {
        std::cerr << "tmux-zhuyin: unknown keyboard layout: " << argv[i] << '\n';
        return 2;
      }
      layout = *parsed;
    } else if (arg == "--eval" && i + 1 < argc) {
      eval_keys = argv[++i];
    } else {
      std::cerr << "tmux-zhuyin: unknown argument: " << arg << '\n';
      print_usage(std::cerr);
      return 2;
    }
  }

  try {
    ChewingEngine engine(layout);
    ImeSession session(std::move(engine));

    std::optional<std::string> result;
    if (eval_keys) {
      const auto events = parse_eval_keys(*eval_keys);
      bool submitted = false;
      for (const auto& ev : events) {
        const auto action = session.handle_key(ev);
        if (action == SessionAction::Submit) {
          submitted = true;
          break;
        }
        if (action == SessionAction::Cancel) {
          submitted = false;
          break;
        }
      }
      if (submitted && !session.committed_text().empty()) {
        result = session.committed_text();
      }
    } else {
      result = run_popup_ui(session, UiOptions{.target_pane = pane_id});
    }

    if (result && !result->empty()) {
      if (!write_output(out_path, *result)) {
        std::cerr << "tmux-zhuyin: failed to write output\n";
        return 1;
      }
    }
  } catch (const std::exception& ex) {
    std::cerr << "tmux-zhuyin: " << ex.what() << '\n';
    return 1;
  }

  return 0;
}
