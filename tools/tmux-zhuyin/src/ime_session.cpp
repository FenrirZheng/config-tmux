#include "ime_session.hpp"

#include <utility>

namespace zhuyin {

ImeSession::ImeSession(ChewingEngine engine)
    : engine_(std::move(engine)), state_(engine_.snapshot()) {}

void ImeSession::pop_last_utf8_char(std::string& s) {
  if (s.empty()) return;
  std::size_t i = s.size() - 1;
  while (i > 0 && (static_cast<unsigned char>(s[i]) & 0xC0u) == 0x80u) {
    --i;
  }
  s.erase(i);
}

void ImeSession::apply_snapshot(EngineSnapshot next) {
  if (!next.newly_committed.empty()) {
    committed_text_ += next.newly_committed;
    next.newly_committed.clear();
  }
  state_ = std::move(next);
}

SessionAction ImeSession::handle_key(const KeyEvent& ev) {
  switch (ev.kind) {
    case KeyKind::CancelNow:
      return SessionAction::Cancel;

    case KeyKind::SubmitNow:
      apply_snapshot(engine_.flush_preedit());
      return committed_text_.empty() ? SessionAction::Cancel : SessionAction::Submit;

    case KeyKind::ToggleChinese:
      apply_snapshot(engine_.toggle_chinese_mode());
      return SessionAction::Continue;

    case KeyKind::ToggleFullshape:
      apply_snapshot(engine_.toggle_fullshape());
      return SessionAction::Continue;

    case KeyKind::Tab:
      if (state_.cand_open) {
        apply_snapshot(engine_.handle_tab());
      } else {
        apply_snapshot(engine_.toggle_chinese_mode());
      }
      return SessionAction::Continue;

    case KeyKind::Escape:
      if (state_.cand_open || !state_.bopomofo.empty()) {
        apply_snapshot(engine_.handle_esc());
        return SessionAction::Continue;
      }
      return SessionAction::Cancel;

    case KeyKind::Enter:
      if (state_.has_composition()) {
        apply_snapshot(engine_.handle_enter());
        return SessionAction::Continue;
      }
      if (!committed_text_.empty()) {
        return SessionAction::Submit;
      }
      return SessionAction::Continue;

    case KeyKind::Backspace:
      if (state_.has_composition()) {
        apply_snapshot(engine_.handle_backspace());
      } else {
        pop_last_utf8_char(committed_text_);
      }
      return SessionAction::Continue;

    case KeyKind::Delete:
      if (state_.has_composition()) {
        apply_snapshot(engine_.handle_del());
      }
      return SessionAction::Continue;

    case KeyKind::Space:
      apply_snapshot(engine_.handle_space());
      return SessionAction::Continue;

    case KeyKind::Left:
      apply_snapshot(engine_.handle_left());
      return SessionAction::Continue;

    case KeyKind::Right:
      apply_snapshot(engine_.handle_right());
      return SessionAction::Continue;

    case KeyKind::Up:
      apply_snapshot(engine_.handle_up());
      return SessionAction::Continue;

    case KeyKind::Down:
      apply_snapshot(engine_.handle_down());
      return SessionAction::Continue;

    case KeyKind::Home:
      apply_snapshot(engine_.handle_home());
      return SessionAction::Continue;

    case KeyKind::End:
      apply_snapshot(engine_.handle_end());
      return SessionAction::Continue;

    case KeyKind::PageUp:
      apply_snapshot(engine_.handle_page_up());
      return SessionAction::Continue;

    case KeyKind::PageDown:
      apply_snapshot(engine_.handle_page_down());
      return SessionAction::Continue;

    case KeyKind::Char:
      apply_snapshot(engine_.handle_default(ev.ch));
      return SessionAction::Continue;
  }

  return SessionAction::Continue;
}

}  // namespace zhuyin
