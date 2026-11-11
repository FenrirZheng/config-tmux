#include "ui.hpp"

#include <termios.h>
#include <unistd.h>

#include <algorithm>
#include <ftxui/component/component.hpp>
#include <ftxui/component/component_options.hpp>
#include <ftxui/component/event.hpp>
#include <ftxui/component/mouse.hpp>
#include <ftxui/component/screen_interactive.hpp>
#include <ftxui/dom/elements.hpp>
#include <ftxui/screen/color.hpp>
#include <string>
#include <utility>

#include "fuzzy_filter.hpp"

namespace ccmm {

namespace {

using namespace ftxui;

// Catppuccin Mocha — the palette ~/.tmux migrated to. The background is left
// as Color::Default so claude.conf's `popup-style bg=#181825` shows through.
const Color kText = Color::RGB(0xcd, 0xd6, 0xf4);
const Color kSubtext = Color::RGB(0xa6, 0xad, 0xc8);
const Color kOverlay = Color::RGB(0x6c, 0x70, 0x86);
const Color kBlue = Color::RGB(0x89, 0xb4, 0xfa);
const Color kMauve = Color::RGB(0xcb, 0xa6, 0xf7);
const Color kRed = Color::RGB(0xf3, 0x8b, 0xa8);
const Color kSurface = Color::RGB(0x31, 0x32, 0x44);  // selected-row band

constexpr std::size_t kMaxBadges = 3;

// Raw control bytes: FTXUI 5.0 has no named Ctrl events.
const Event kCtrlC = Event::Special("\x03");
const Event kCtrlG = Event::Special("\x07");
const Event kCtrlN = Event::Special("\x0e");
const Event kCtrlP = Event::Special("\x10");

Element badge_row(const std::vector<std::string>& badges) {
  Elements out;
  for (std::size_t i = 0; i < badges.size() && i < kMaxBadges; ++i) {
    out.push_back(text(" "));
    out.push_back(text(" " + badges[i] + " ") | color(kBlue) | bold);
  }
  if (badges.size() > kMaxBadges)
    out.push_back(text(" +" + std::to_string(badges.size() - kMaxBadges)) | color(kSubtext));
  return hbox(std::move(out));
}

Element row_element(const Action& a, bool selected) {
  Element label;
  if (a.kind == Kind::Command) {
    Elements parts{text(a.name) | color(selected ? kMauve : kText)};
    if (!a.alias.empty()) parts.push_back(text(" (" + a.alias + ")") | color(kSubtext));
    label = hbox(std::move(parts));
  } else {
    label = hbox({text("★ ") | color(kMauve), text(a.note) | color(selected ? kMauve : kText)});
  }
  auto row = hbox({text(selected ? "▌" : " ") | color(kMauve), label | xflex_shrink,
                   filler(), badge_row(a.badges), text(" ")});
  if (selected) row = row | bold | bgcolor(kSurface) | focus;
  return row;
}

Element field(const std::string& name, Element value) {
  return hbox({text(name) | color(kSubtext) | size(WIDTH, EQUAL, 8), std::move(value)});
}

std::string context_line(const Context& c) {
  auto show = [](const std::string& v) { return v.empty() ? std::string("?") : v; };
  return "session " + show(c.session) + " · window " + show(c.window) + " · pane " +
         show(c.pane) + " · client " + show(c.client);
}

InputOption themed_input(std::string placeholder) {
  InputOption opt;
  opt.multiline = false;
  opt.placeholder = std::move(placeholder);
  opt.transform = [](InputState s) {
    if (s.is_placeholder) return s.element | color(kOverlay) | dim;
    return s.element | color(kText);
  };
  return opt;
}

// FTXUI 5 leaves ISIG on, so Ctrl-C raises SIGINT for the whole foreground
// process group; FTXUI turns it into a quiet exit, but any wrapper shell in the
// group dies with it (verify.sh's exit-status wrapper did). Turning ISIG off
// makes Ctrl-C an ordinary \x03 event handled by CatchEvent.
// Called from the first render, not via screen.Post(): Post() before Loop()
// is dropped (no task sender yet), and a render runs only after FTXUI's
// Install() has set its own termios. FTXUI restores the attributes it saved
// at start-up on exit, so ISIG comes back afterwards.
void disable_signal_keys() {
  termios t{};
  if (::tcgetattr(STDIN_FILENO, &t) == 0) {
    t.c_lflag &= ~static_cast<tcflag_t>(ISIG);
    ::tcsetattr(STDIN_FILENO, TCSANOW, &t);
  }
}

}  // namespace

UiResult run_ui(const Catalog& catalog, const Context& ctx, const std::string& load_error) {
  UiResult result;
  auto screen = ScreenInteractive::Fullscreen();

  std::string query;
  std::string line;
  int line_cursor = 0;
  int mode = 0;  // 0 = search, 1 = argument bar
  int sel = 0;
  std::string status;
  bool status_is_error = false;
  std::vector<std::size_t> shown = filter("", catalog.actions);

  auto selected = [&]() -> const Action* {
    if (shown.empty()) return nullptr;
    return &catalog.actions[shown[static_cast<std::size_t>(sel)]];
  };
  auto move = [&](int delta) {
    if (shown.empty()) return;
    sel = std::clamp(sel + delta, 0, static_cast<int>(shown.size()) - 1);
  };
  auto finish = [&](std::vector<std::string> argv) {
    result.argv = std::move(argv);
    screen.Exit();
  };
  auto cancel = [&] {
    result.argv.clear();
    screen.Exit();
  };

  auto qopt = themed_input("type to filter commands and keys…");
  qopt.on_change = [&] {
    shown = filter(query, catalog.actions);
    sel = 0;
    status.clear();
  };
  auto query_input = Input(&query, qopt);

  auto lopt = themed_input("command [flags] [args]");
  lopt.cursor_position = &line_cursor;
  lopt.on_change = [&] { status.clear(); };
  auto line_input = Input(&line, lopt);

  auto inputs = Container::Tab({query_input, line_input}, &mode);

  bool signal_keys_off = false;
  auto renderer = Renderer(inputs, [&] {
    if (!signal_keys_off) {
      disable_signal_keys();
      signal_keys_off = true;
    }
    const auto* a = selected();
    const std::string count =
        std::to_string(shown.size()) + "/" + std::to_string(catalog.actions.size());

    Element top;
    if (mode == 0) {
      top = hbox({text(" ❯ ") | color(kMauve) | bold, query_input->Render() | flex,
                  text(" " + count + " ") | color(kSubtext)});
    } else {
      top = hbox({text(" ⌨ ") | color(kMauve) | bold, line_input->Render() | flex,
                  text(" edit · Enter run · Esc back ") | color(kSubtext)});
    }

    Element list;
    if (!load_error.empty()) {
      list = paragraph(load_error) | color(kRed);
    } else if (shown.empty()) {
      list = text(" no match") | color(kSubtext);
    } else {
      Elements rows;
      rows.reserve(shown.size());
      for (std::size_t i = 0; i < shown.size(); ++i)
        rows.push_back(row_element(catalog.actions[shown[i]], static_cast<int>(i) == sel));
      list = vbox(std::move(rows)) | vscroll_indicator | yframe;
    }

    Elements preview;
    if (a != nullptr) {
      if (a->kind == Kind::Command) {
        preview.push_back(
            field("usage", paragraph(a->name + (a->usage.empty() ? "" : " " + a->usage)) |
                               color(kText)));
      } else {
        preview.push_back(field("runs", paragraph(a->bound_command) | color(kText)));
        preview.push_back(field("key", text(a->badges.empty() ? a->key : a->badges.front()) |
                                           color(kBlue)));
      }
    }
    preview.push_back(field("target", text(context_line(ctx)) | color(kSubtext)));
    if (!status.empty()) {
      preview.push_back(paragraph(status) | color(status_is_error ? kRed : kSubtext));
    } else if (mode == 0) {
      preview.push_back(text("Enter run · Tab edit args · ↑↓ C-n C-p wheel move · Esc quit") |
                        color(kOverlay));
    }

    return vbox({
               top,
               separator() | color(kBlue),
               list | flex,
               separator() | color(kBlue),
               vbox(std::move(preview)),
           }) |
           color(kText);
  });

  auto root = CatchEvent(renderer, [&](Event e) {
    if (e == kCtrlC) {
      cancel();
      return true;
    }
    if (!load_error.empty()) {
      if (e.is_character() || e == Event::Escape || e == Event::Return) cancel();
      return true;
    }
    if (e == Event::Escape || e == kCtrlG) {
      if (mode == 1) {
        mode = 0;
        status.clear();
      } else {
        cancel();
      }
      return true;
    }
    if (e.is_mouse()) {
      const auto b = e.mouse().button;
      if (b == Mouse::WheelUp) move(-1);
      if (b == Mouse::WheelDown) move(+1);
      return b == Mouse::WheelUp || b == Mouse::WheelDown;
    }
    if (mode == 0) {
      if (e == Event::ArrowUp || e == kCtrlP) return move(-1), true;
      if (e == Event::ArrowDown || e == kCtrlN) return move(+1), true;
      if (e == Event::PageUp) return move(-10), true;
      if (e == Event::PageDown) return move(+10), true;
      if (e == Event::Tab) {
        if (const auto* a = selected()) {
          line = edit_line_for(*a);
          line_cursor = static_cast<int>(line.size());
          mode = 1;
          status.clear();
        }
        return true;
      }
      if (e == Event::Return) {
        const auto* a = selected();
        if (a == nullptr) return true;
        if (needs_destination(*a)) {
          // Run bare, source == destination and tmux silently does nothing.
          line = destination_line_for(*a);
          line_cursor = static_cast<int>(line.size());
          mode = 1;
          status = a->name + " needs a destination: type it after -t (e.g. :2, @5), Enter runs";
          status_is_error = false;
          return true;
        }
        if (const auto need = required_args(*a); !need.empty()) {
          // Bare, tmux rejects it: "too few arguments" (e.g. rename-window).
          line = edit_line_for(*a);
          line_cursor = static_cast<int>(line.size());
          mode = 1;
          status = a->name + " needs " + need + ": type it, Enter runs";
          status_is_error = false;
          return true;
        }
        std::string err;
        if (auto argv = resolve_action(*a, catalog, ctx, err)) {
          finish(std::move(*argv));
        } else {
          status = err;
          status_is_error = true;
        }
        return true;
      }
      return false;
    }
    // mode == 1: argument bar
    if (e == Event::Return) {
      std::string err;
      if (auto argv = resolve_line(line, catalog, ctx, err)) {
        finish(std::move(*argv));
      } else {
        status = err;
        status_is_error = true;
      }
      return true;
    }
    if (e == Event::Tab) return true;  // keep focus in the bar
    return false;
  });

  screen.Loop(root);
  return result;
}

}  // namespace ccmm
