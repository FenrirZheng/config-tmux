#include "ui.hpp"

#include <termios.h>
#include <unistd.h>

#include <algorithm>
#include <array>
#include <ftxui/component/component.hpp>
#include <ftxui/component/component_options.hpp>
#include <ftxui/component/event.hpp>
#include <ftxui/component/screen_interactive.hpp>
#include <ftxui/dom/elements.hpp>
#include <ftxui/screen/color.hpp>
#include <ftxui/screen/terminal.hpp>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

namespace win_pick {

namespace {

using namespace ftxui;

// Tokyo Night palette aligned with tools/tmux-bar/src/bar_spec.hpp.
// Background stays Color::Default so tmux-bar's `popup-style bg=#16161e` shows through.
const Color kSelection = Color::RGB(0x28, 0x34, 0x57);
const Color kBorder = Color::RGB(0x29, 0x2e, 0x42);
const Color kMuted = Color::RGB(0x56, 0x5f, 0x89);
const Color kComment = Color::RGB(0x73, 0x7a, 0xa2);
const Color kSubtext = Color::RGB(0xa9, 0xb1, 0xd6);
const Color kText = Color::RGB(0xc0, 0xca, 0xf5);
const Color kBlue = Color::RGB(0x7a, 0xa2, 0xf7);
const Color kMagenta = Color::RGB(0xbb, 0x9a, 0xf7);
const Color kRed = Color::RGB(0xf7, 0x76, 0x8e);
const Color kYellow = Color::RGB(0xe0, 0xaf, 0x68);
const Color kGreen = Color::RGB(0x9e, 0xce, 0x6a);

// Raw control bytes (FTXUI 5.x does not expose named Ctrl-letter events).
const Event kCtrlC = Event::Special("\x03");
const Event kCtrlG = Event::Special("\x07");
const Event kCtrlN = Event::Special("\x0e");
const Event kCtrlP = Event::Special("\x10");
const Event kCtrlR = Event::Special("\x12");
const Event kCtrlU = Event::Special("\x15");
const Event kCtrlW = Event::Special("\x17");

// Disable ISIG on first render so Ctrl-C arrives as \x03 in CatchEvent rather
// than raising SIGINT against the popup wrapper.
void disable_signal_keys() {
  termios t{};
  if (::tcgetattr(STDIN_FILENO, &t) == 0) {
    t.c_lflag &= ~static_cast<tcflag_t>(ISIG);
    ::tcsetattr(STDIN_FILENO, TCSANOW, &t);
  }
}

Color to_ftxui_color(const AnsiColor& c) {
  static constexpr std::array<Color::Palette16, 16> kPal16 = {
      Color::Black,        Color::Red,         Color::Green,       Color::Yellow,
      Color::Blue,         Color::Magenta,     Color::Cyan,        Color::GrayLight,
      Color::GrayDark,     Color::RedLight,    Color::GreenLight,  Color::YellowLight,
      Color::BlueLight,    Color::MagentaLight, Color::CyanLight,  Color::White,
  };
  switch (c.kind) {
    case AnsiColor::Kind::Default:
      return Color::Default;
    case AnsiColor::Kind::Palette16:
      return Color(kPal16[c.index & 0x0fU]);
    case AnsiColor::Kind::Palette256:
      return Color(static_cast<Color::Palette256>(c.index));
    case AnsiColor::Kind::Rgb:
      return Color::RGB(c.r, c.g, c.b);
  }
  return Color::Default;
}

Element render_ansi_line(const std::vector<AnsiSpan>& spans) {
  if (spans.empty()) return text(" ");
  Elements parts;
  parts.reserve(spans.size());
  for (const auto& sp : spans) {
    Element el = text(sp.text);
    if (sp.style.fg.kind != AnsiColor::Kind::Default) {
      el = el | color(to_ftxui_color(sp.style.fg));
    }
    if (sp.style.bg.kind != AnsiColor::Kind::Default) {
      el = el | bgcolor(to_ftxui_color(sp.style.bg));
    }
    if (sp.style.bold) el = el | bold;
    if (sp.style.dim) el = el | dim;
    if (sp.style.underlined) el = el | underlined;
    if (sp.style.inverted) el = el | inverted;
    parts.push_back(std::move(el));
  }
  return hbox(std::move(parts));
}

Element agent_dot(AgentState state, bool bell) {
  if (state == AgentState::NeedsInput || bell) {
    return text("● ") | color(kRed);
  }
  if (state == AgentState::Busy) {
    return text("● ") | color(kYellow);
  }
  if (state == AgentState::Idle) {
    return text("● ") | color(kGreen);
  }
  return text("  ");
}

std::string pad_index(int idx, int width) {
  std::string s = std::to_string(idx);
  if (static_cast<int>(s.size()) < width) {
    return std::string(static_cast<std::size_t>(width - static_cast<int>(s.size())), ' ') + s;
  }
  return s;
}

}  // namespace

UiChoice run_ui(Inventory inventory,
                std::string_view home_dir,
                std::string_view socket) {
  UiChoice choice;
  auto screen = ScreenInteractive::Fullscreen();

  bool all_sessions = false;
  std::string query;
  std::vector<std::size_t> shown =
      filter_windows(inventory.windows, query, all_sessions, inventory.current_session_id);

  // Start cursor on the currently active window when present.
  int selected = 0;
  for (std::size_t i = 0; i < shown.size(); ++i) {
    if (inventory.windows[shown[i]].window_id == inventory.current_window_id) {
      selected = static_cast<int>(i);
      break;
    }
  }

  using ParsedCapture = std::vector<std::vector<AnsiSpan>>;
  std::unordered_map<std::string, ParsedCapture> preview_cache;

  auto current_window = [&]() -> const WindowEntry* {
    if (shown.empty()) return nullptr;
    if (selected < 0 || selected >= static_cast<int>(shown.size())) return nullptr;
    return &inventory.windows[shown[static_cast<std::size_t>(selected)]];
  };

  auto get_preview = [&](const WindowEntry& w) -> const ParsedCapture& {
    auto it = preview_cache.find(w.active_pane_id);
    if (it != preview_cache.end()) return it->second;
    const std::string raw = capture_pane_preview(w.active_pane_id, socket);
    auto [ins, _] = preview_cache.emplace(w.active_pane_id, parse_ansi_capture(raw));
    return ins->second;
  };

  auto refilter = [&](bool preserve_current_window) {
    std::string keep_id;
    if (preserve_current_window) {
      if (const WindowEntry* cur = current_window()) {
        keep_id = cur->window_id;
      }
    }
    shown = filter_windows(inventory.windows, query, all_sessions, inventory.current_session_id);
    if (shown.empty()) {
      selected = 0;
      return;
    }
    if (!keep_id.empty()) {
      for (std::size_t i = 0; i < shown.size(); ++i) {
        if (inventory.windows[shown[i]].window_id == keep_id) {
          selected = static_cast<int>(i);
          return;
        }
      }
    }
    selected = 0;
  };

  InputOption input_opt;
  input_opt.multiline = false;
  input_opt.placeholder = "number or filter...";
  input_opt.transform = [](InputState s) {
    if (s.is_placeholder) return s.element | color(kMuted) | dim;
    return s.element | color(kText);
  };
  input_opt.on_change = [&]() {
    // When clearing back to empty query, stay on the window we were just previewing.
    refilter(query.empty());
  };

  Component input_comp = Input(&query, input_opt);

  bool signal_disabled = false;

  auto renderer = Renderer(input_comp, [&]() -> Element {
    if (!signal_disabled) {
      disable_signal_keys();
      signal_disabled = true;
    }

    const int term_w = std::max(60, Terminal::Size().dimx);
    const int left_w = std::clamp(term_w * 36 / 100, 28, 52);

    // Determine max index width for neat right-alignment.
    int max_idx_w = 1;
    for (std::size_t idx : shown) {
      const int w_len = static_cast<int>(std::to_string(inventory.windows[idx].window_index).size());
      if (w_len > max_idx_w) max_idx_w = w_len;
    }

    // ── Left pane: search prompt + window list ──────────────────────────
    Element scope_badge = all_sessions
                              ? text(" all ") | color(kMagenta) | bold
                              : text(" session ") | color(kComment);

    Element search_bar = hbox({
        text(" ❯ ") | color(kBlue) | bold,
        input_comp->Render() | flex,
        scope_badge,
        text(" "),
    });

    Elements list_rows;
    if (shown.empty()) {
      list_rows.push_back(text("  (no matching windows)") | color(kMuted) | dim);
    } else {
      list_rows.reserve(shown.size());
      for (std::size_t i = 0; i < shown.size(); ++i) {
        const WindowEntry& w = inventory.windows[shown[i]];
        const bool is_sel = (static_cast<int>(i) == selected);
        const bool is_cur = (w.window_id == inventory.current_window_id);

        Elements cols;
        cols.push_back(text(is_sel ? "▌" : " ") | color(kBlue));
        cols.push_back(text(" "));

        if (all_sessions) {
          cols.push_back(text(w.session_name + ":") | color(is_sel ? kSubtext : kMuted));
        }

        Color idx_color = is_sel ? kBlue : (w.agent_state == AgentState::NeedsInput || w.bell ? kText : kComment);
        Element idx_el = text(pad_index(w.window_index, max_idx_w)) | color(idx_color);
        if (is_sel || w.agent_state == AgentState::NeedsInput || w.bell) {
          idx_el = idx_el | bold;
        }
        cols.push_back(std::move(idx_el));
        cols.push_back(text(" "));
        cols.push_back(agent_dot(w.agent_state, w.bell));

        const std::string label = display_label(w);
        Element label_el = text(label) | color(is_sel ? kText : kSubtext);
        if (is_sel) label_el = label_el | bold;
        cols.push_back(std::move(label_el) | xflex_shrink);

        cols.push_back(filler());

        if (w.pane_count > 1) {
          cols.push_back(text(" ×" + std::to_string(w.pane_count)) | color(kMuted));
        }
        if (is_cur) {
          cols.push_back(text(" *") | color(kBlue));
        }
        cols.push_back(text(" "));

        Element row = hbox(std::move(cols));
        if (is_sel) {
          row = row | bgcolor(kSelection) | focus;
        }
        list_rows.push_back(std::move(row));
      }
    }

    Element left_pane = vbox({
                            search_bar,
                            separatorLight() | color(kBorder),
                            vbox(std::move(list_rows)) | yframe | flex,
                        }) |
                        size(WIDTH, EQUAL, left_w);

    // ── Right pane: preview header + live ANSI capture ──────────────────
    Element right_header;
    Element preview_body;
    if (const WindowEntry* w = current_window()) {
      Elements hdr;
      hdr.push_back(text(" #" + std::to_string(w->window_index) + " ") | color(kBlue) | bold);
      hdr.push_back(text(display_label(*w)) | color(kText) | bold);
      const std::string spath = shorten_path(w->path, home_dir);
      if (!spath.empty()) {
        hdr.push_back(text(" · ") | color(kMuted));
        hdr.push_back(text(spath) | color(kComment));
      }
      if (w->pane_count > 1) {
        hdr.push_back(text(" · " + std::to_string(w->pane_count) + " panes") | color(kMuted));
      }
      if (w->agent_state != AgentState::None) {
        hdr.push_back(text(" · ") | color(kMuted));
        hdr.push_back(agent_dot(w->agent_state, w->bell));
        const std::string& info = !w->task_hint.empty()
                                      ? w->task_hint
                                      : std::string(agent_state_name(w->agent_state));
        hdr.push_back(text(info) | color(kSubtext));
      }
      right_header = hbox(std::move(hdr)) | xflex_shrink;

      const ParsedCapture& cap = get_preview(*w);
      if (cap.empty()) {
        preview_body = vbox({text("  (empty pane)") | color(kMuted) | dim}) | flex;
      } else {
        Elements plines;
        plines.reserve(cap.size());
        for (const auto& line : cap) {
          plines.push_back(hbox({text(" "), render_ansi_line(line)}));
        }
        preview_body = vbox(std::move(plines)) | flex;
      }
    } else {
      right_header = text(" preview") | color(kMuted);
      preview_body = filler();
    }

    Element right_pane = vbox({
                             right_header,
                             separatorLight() | color(kBorder),
                             preview_body,
                         }) |
                         flex;

    // ── Footer: key hints + match counter ───────────────────────────────
    std::size_t scoped_total = 0;
    for (const auto& w : inventory.windows) {
      if (all_sessions || inventory.current_session_id.empty() ||
          w.session_id == inventory.current_session_id) {
        ++scoped_total;
      }
    }

    Element footer = hbox({
        text(" C-n/C-p ") | color(kBlue),
        text("select  ") | color(kComment),
        text("0-9/text ") | color(kBlue),
        text("preview  ") | color(kComment),
        text("Tab ") | color(kBlue),
        text("scope  ") | color(kComment),
        text("C-r ") | color(kBlue),
        text("refresh  ") | color(kComment),
        text("Enter ") | color(kBlue),
        text("jump  ") | color(kComment),
        text("Esc/C-g ") | color(kBlue),
        text("quit") | color(kComment),
        filler(),
        text(std::to_string(shown.size()) + "/" + std::to_string(scoped_total) + " ") |
            color(kMuted),
    });

    return vbox({
        hbox({
            left_pane,
            separatorLight() | color(kBorder),
            right_pane,
        }) | flex,
        separatorLight() | color(kBorder),
        footer,
    });
  });

  auto root = CatchEvent(renderer, [&](Event event) -> bool {
    if (event == Event::Escape || event == kCtrlG || event == kCtrlC) {
      choice.confirmed = false;
      screen.Exit();
      return true;
    }

    if (event == Event::Return) {
      if (const WindowEntry* w = current_window()) {
        choice.confirmed = true;
        choice.window_id = w->window_id;
        choice.same_session = (w->session_id == inventory.current_session_id);
        screen.Exit();
      }
      return true;
    }

    if (event == kCtrlN || event == Event::ArrowDown) {
      if (!shown.empty()) {
        selected = (selected + 1) % static_cast<int>(shown.size());
      }
      return true;
    }

    if (event == kCtrlP || event == Event::ArrowUp) {
      if (!shown.empty()) {
        const int n = static_cast<int>(shown.size());
        selected = (selected - 1 + n) % n;
      }
      return true;
    }

    if (event == Event::Tab) {
      all_sessions = !all_sessions;
      refilter(true);
      return true;
    }

    if (event == kCtrlU || event == kCtrlW) {
      if (!query.empty()) {
        query.clear();
        refilter(true);
      }
      return true;
    }

    if (event == kCtrlR) {
      preview_cache.clear();
      if (auto fresh = fetch_inventory(socket)) {
        inventory = std::move(*fresh);
        refilter(true);
      }
      return true;
    }

    return false;
  });

  screen.Loop(root);
  return choice;
}

}  // namespace win_pick
