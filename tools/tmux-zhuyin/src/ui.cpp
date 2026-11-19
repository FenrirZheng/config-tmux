#include "ui.hpp"

#include <ftxui/component/component.hpp>
#include <ftxui/component/event.hpp>
#include <ftxui/component/screen_interactive.hpp>
#include <ftxui/dom/elements.hpp>
#include <ftxui/screen/color.hpp>

#include <string>
#include <string_view>
#include <vector>

namespace zhuyin {

namespace {

using namespace ftxui;

// Catppuccin Mocha palette — matches ~/.tmux/claude.conf & cc-main-menu.
// Background stays Color::Default so tmux popup-style bg=#181825 shows through.
const Color kText = Color::RGB(0xcd, 0xd6, 0xf4);
const Color kSubtext = Color::RGB(0xa6, 0xad, 0xc8);
const Color kOverlay = Color::RGB(0x6c, 0x70, 0x86);
const Color kBlue = Color::RGB(0x89, 0xb4, 0xfa);
const Color kMauve = Color::RGB(0xcb, 0xa6, 0xf7);
const Color kYellow = Color::RGB(0xf9, 0xe2, 0xaf);
const Color kGreen = Color::RGB(0xa6, 0xe3, 0xa1);
const Color kSurface = Color::RGB(0x31, 0x32, 0x44);
const Color kCrust = Color::RGB(0x11, 0x11, 0x1b);

// Raw control bytes (FTXUI 5.0 represents Ctrl+key as raw control bytes).
const Event kCtrlC = Event::Special("\x03");
const Event kCtrlF = Event::Special("\x06");
const Event kCtrlG = Event::Special("\x07");
const Event kCtrlS = Event::Special("\x13");
const Event kCtrlBackslash = Event::Special("\x1c");

// Splits a UTF-8 string into codepoints as individual strings.
std::vector<std::string> utf8_chars(std::string_view s) {
  std::vector<std::string> out;
  std::size_t i = 0;
  while (i < s.size()) {
    const unsigned char c = static_cast<unsigned char>(s[i]);
    std::size_t len = 1;
    if ((c & 0x80u) == 0x00u) {
      len = 1;
    } else if ((c & 0xE0u) == 0xC0u) {
      len = 2;
    } else if ((c & 0xF0u) == 0xE0u) {
      len = 3;
    } else if ((c & 0xF8u) == 0xF0u) {
      len = 4;
    }
    if (i + len > s.size()) len = s.size() - i;
    out.emplace_back(s.substr(i, len));
    i += len;
  }
  return out;
}

std::optional<KeyEvent> translate_event(const Event& ev) {
  if (ev == kCtrlC || ev == kCtrlG) return KeyEvent{KeyKind::CancelNow, 0};
  if (ev == kCtrlS) return KeyEvent{KeyKind::SubmitNow, 0};
  if (ev == kCtrlBackslash) return KeyEvent{KeyKind::ToggleChinese, 0};
  if (ev == kCtrlF || ev == Event::F6) return KeyEvent{KeyKind::ToggleFullshape, 0};

  if (ev == Event::Escape) return KeyEvent{KeyKind::Escape, 0};
  if (ev == Event::Return) return KeyEvent{KeyKind::Enter, 0};
  if (ev == Event::Backspace) return KeyEvent{KeyKind::Backspace, 0};
  if (ev == Event::Delete) return KeyEvent{KeyKind::Delete, 0};
  if (ev == Event::ArrowLeft) return KeyEvent{KeyKind::Left, 0};
  if (ev == Event::ArrowRight) return KeyEvent{KeyKind::Right, 0};
  if (ev == Event::ArrowUp) return KeyEvent{KeyKind::Up, 0};
  if (ev == Event::ArrowDown) return KeyEvent{KeyKind::Down, 0};
  if (ev == Event::Home) return KeyEvent{KeyKind::Home, 0};
  if (ev == Event::End) return KeyEvent{KeyKind::End, 0};
  if (ev == Event::PageUp) return KeyEvent{KeyKind::PageUp, 0};
  if (ev == Event::PageDown) return KeyEvent{KeyKind::PageDown, 0};
  if (ev == Event::Tab) return KeyEvent{KeyKind::Tab, 0};

  if (ev.is_character()) {
    const std::string& s = ev.character();
    if (s == " ") return KeyEvent{KeyKind::Space, ' '};
    if (s.size() == 1) {
      const unsigned char byte = static_cast<unsigned char>(s[0]);
      if (byte >= 0x20 && byte <= 0x7e) {
        return KeyEvent{KeyKind::Char, static_cast<int>(byte)};
      }
    }
  }
  return std::nullopt;
}

Element render_composition(const ImeSession& session) {
  const auto& committed = session.committed_text();
  const auto& st = session.state();

  Elements segs;
  if (!committed.empty()) {
    segs.push_back(text(committed) | color(kText));
  }

  const auto pre_chars = utf8_chars(st.preedit);
  const int cur = std::clamp(st.cursor_pos, 0, static_cast<int>(pre_chars.size()));

  std::string before_cur;
  for (int i = 0; i < cur; ++i) before_cur += pre_chars[static_cast<std::size_t>(i)];

  if (!before_cur.empty()) {
    segs.push_back(text(before_cur) | color(kBlue) | underlined);
  }

  if (!st.bopomofo.empty()) {
    segs.push_back(text(st.bopomofo) | color(kYellow) | bold | underlined);
  }

  if (cur < static_cast<int>(pre_chars.size())) {
    segs.push_back(text(pre_chars[static_cast<std::size_t>(cur)]) | color(kCrust) |
                   bgcolor(kBlue) | bold | focus);
    std::string after_cur;
    for (std::size_t i = static_cast<std::size_t>(cur) + 1; i < pre_chars.size(); ++i) {
      after_cur += pre_chars[i];
    }
    if (!after_cur.empty()) {
      segs.push_back(text(after_cur) | color(kBlue) | underlined);
    }
  } else {
    // End-of-line cursor block. Attaching `focus` tells FTXUI to hide the
    // terminal's own hardware cursor (Screen::Cursor::Hidden) cleanly.
    segs.push_back(text("▏") | color(kMauve) | bold | focus);
  }

  if (committed.empty() && st.preedit.empty() && st.bopomofo.empty()) {
    segs.push_back(text(" 輸入注音開始打字…") | color(kOverlay));
  }

  return hbox(std::move(segs)) | flex;
}

Element render_candidates(const EngineSnapshot& st) {
  if (st.cand_open && !st.candidates.empty()) {
    Elements items;
    for (std::size_t i = 0; i < st.candidates.size(); ++i) {
      if (i > 0) items.push_back(text("  "));
      items.push_back(text(std::to_string(i + 1) + ".") | color(kBlue) | bold);
      items.push_back(text(st.candidates[i]) | color(kText));
    }
    const std::string page_str =
        std::to_string(st.cand_page + 1) + "/" + std::to_string(st.cand_total_pages);
    return hbox({
        hbox(std::move(items)) | xflex_shrink,
        filler(),
        text(" " + page_str + " ") | color(kSubtext),
    });
  }

  if (!st.aux_text.empty()) {
    return text(st.aux_text) | color(kGreen);
  }

  return text(" ") | color(kOverlay);
}

}  // namespace

std::optional<std::string> run_popup_ui(ImeSession& session, const UiOptions& opts) {
  auto screen = ScreenInteractive::Fullscreen();
  bool submitted = false;

  auto renderer = Renderer([&] {
    const auto& st = session.state();

    auto mode_badge = st.chinese_mode
                          ? (text(" 注音 ") | color(kCrust) | bgcolor(kMauve) | bold)
                          : (text(" EN ") | color(kCrust) | bgcolor(kBlue) | bold);
    auto shape_badge = st.fullshape ? (text(" 全形 ") | color(kYellow) | bgcolor(kSurface))
                                    : (text(" 半形 ") | color(kSubtext) | bgcolor(kSurface));
    auto kb_badge =
        text(" " + std::string(keyboard_layout_label(session.layout())) + " ") | color(kSubtext);

    Elements header_parts{std::move(mode_badge), text(" "), std::move(shape_badge),
                          std::move(kb_badge), filler()};
    if (!opts.target_pane.empty()) {
      header_parts.push_back(text("→ " + opts.target_pane) | color(kOverlay));
    }

    auto header = hbox(std::move(header_parts));
    auto comp_box = vbox({
                        text(" "),
                        hbox({text(" "), render_composition(session), text(" ")}),
                        text(" "),
                    }) |
                    borderStyled(ROUNDED, kBlue);

    auto cand_bar = hbox({text(" "), render_candidates(st), text(" ")});
    auto footer = hbox({
        text(" Enter") | color(kBlue) | bold,
        text(" 確認/送出  ") | color(kSubtext),
        text("C-s") | color(kBlue) | bold,
        text(" 立即送出  ") | color(kSubtext),
        text("Tab") | color(kBlue) | bold,
        text(" 中英  ") | color(kSubtext),
        text("↓") | color(kBlue) | bold,
        text(" 選字  ") | color(kSubtext),
        text("`") | color(kBlue) | bold,
        text(" 符號  ") | color(kSubtext),
        text("C-g") | color(kBlue) | bold,
        text(" 取消") | color(kSubtext),
    });

    return vbox({
               header,
               comp_box | flex,
               cand_bar,
               separator() | color(kSurface),
               footer,
           }) |
           xflex;
  });

  auto app = CatchEvent(renderer, [&](Event ev) {
    if (ev.is_mouse()) return false;
    auto key = translate_event(ev);
    if (!key) return false;

    const SessionAction action = session.handle_key(*key);
    if (action == SessionAction::Submit) {
      submitted = true;
      screen.Exit();
    } else if (action == SessionAction::Cancel) {
      submitted = false;
      screen.Exit();
    }
    return true;
  });

  screen.Loop(app);

  if (!submitted || session.committed_text().empty()) {
    return std::nullopt;
  }
  return session.committed_text();
}

}  // namespace zhuyin
