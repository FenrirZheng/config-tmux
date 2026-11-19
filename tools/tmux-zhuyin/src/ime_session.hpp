#pragma once

#include <string>
#include <string_view>

#include "chewing_engine.hpp"

namespace zhuyin {

enum class KeyKind {
  Char,
  Space,
  Enter,
  Escape,
  Backspace,
  Delete,
  Left,
  Right,
  Up,
  Down,
  Home,
  End,
  PageUp,
  PageDown,
  Tab,
  ToggleChinese,
  ToggleFullshape,
  SubmitNow,
  CancelNow,
};

struct KeyEvent {
  KeyKind kind = KeyKind::Char;
  int ch = 0;  // ASCII byte when kind == KeyKind::Char
};

enum class SessionAction {
  Continue,
  Submit,
  Cancel,
};

class ImeSession {
 public:
  explicit ImeSession(ChewingEngine engine);

  SessionAction handle_key(const KeyEvent& ev);

  [[nodiscard]] const std::string& committed_text() const { return committed_text_; }
  [[nodiscard]] const EngineSnapshot& state() const { return state_; }
  [[nodiscard]] KeyboardLayout layout() const { return engine_.layout(); }
  [[nodiscard]] bool has_any_text() const {
    return !committed_text_.empty() || state_.has_composition();
  }

  // Helper exposed for unit tests and UTF-8 editing.
  static void pop_last_utf8_char(std::string& s);

 private:
  void apply_snapshot(EngineSnapshot next);

  ChewingEngine engine_;
  EngineSnapshot state_;
  std::string committed_text_;
};

}  // namespace zhuyin
