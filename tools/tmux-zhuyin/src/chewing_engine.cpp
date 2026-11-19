#include "chewing_engine.hpp"

#include <chewing.h>

#include <array>
#include <stdexcept>
#include <string>

namespace zhuyin {

namespace {

const char* layout_to_chewing_name(KeyboardLayout layout) {
  switch (layout) {
    case KeyboardLayout::Default:
      return "KB_DEFAULT";
    case KeyboardLayout::Hsu:
      return "KB_HSU";
    case KeyboardLayout::Eten:
      return "KB_ET";
    case KeyboardLayout::Eten26:
      return "KB_ET26";
    case KeyboardLayout::Dvorak:
      return "KB_DVORAK";
    case KeyboardLayout::GinYieh:
      return "KB_GIN_YIEH";
    case KeyboardLayout::DachenCp26:
      return "KB_DACHEN_CP26";
  }
  return "KB_DEFAULT";
}

}  // namespace

std::optional<KeyboardLayout> parse_keyboard_layout(std::string_view name) {
  if (name == "default" || name == "dachen" || name == "standard") {
    return KeyboardLayout::Default;
  }
  if (name == "hsu") return KeyboardLayout::Hsu;
  if (name == "et" || name == "eten") return KeyboardLayout::Eten;
  if (name == "et26" || name == "eten26") return KeyboardLayout::Eten26;
  if (name == "dvorak") return KeyboardLayout::Dvorak;
  if (name == "ginyieh" || name == "gin_yieh") return KeyboardLayout::GinYieh;
  if (name == "dachen_cp26" || name == "cp26") return KeyboardLayout::DachenCp26;
  return std::nullopt;
}

std::string_view keyboard_layout_label(KeyboardLayout layout) {
  switch (layout) {
    case KeyboardLayout::Default:
      return "大千";
    case KeyboardLayout::Hsu:
      return "許氏";
    case KeyboardLayout::Eten:
      return "倚天";
    case KeyboardLayout::Eten26:
      return "倚天26";
    case KeyboardLayout::Dvorak:
      return "Dvorak";
    case KeyboardLayout::GinYieh:
      return "精業";
    case KeyboardLayout::DachenCp26:
      return "大千26";
  }
  return "大千";
}

void ChewingEngine::Deleter::operator()(ChewingContext* ctx) const noexcept {
  if (ctx != nullptr) {
    chewing_delete(ctx);
  }
}

ChewingEngine::ChewingEngine(KeyboardLayout layout,
                             std::optional<std::string> user_data_path,
                             std::optional<std::string> sys_data_path)
    : layout_(layout) {
  const char* sys_path = sys_data_path ? sys_data_path->c_str() : nullptr;
  const char* usr_path = user_data_path ? user_data_path->c_str() : nullptr;

  ChewingContext* raw = (sys_path || usr_path)
                            ? chewing_new2(sys_path, usr_path, nullptr, nullptr)
                            : chewing_new();
  if (raw == nullptr) {
    throw std::runtime_error("failed to initialize libchewing context");
  }
  ctx_.reset(raw);

  constexpr std::array<int, 9> kSelKeys = {'1', '2', '3', '4', '5', '6', '7', '8', '9'};
  chewing_set_selKey(ctx_.get(), kSelKeys.data(), static_cast<int>(kSelKeys.size()));
  chewing_set_candPerPage(ctx_.get(), static_cast<int>(kSelKeys.size()));
  chewing_set_maxChiSymbolLen(ctx_.get(), 32);
  chewing_set_spaceAsSelection(ctx_.get(), 1);
  chewing_set_autoShiftCur(ctx_.get(), 1);
  chewing_set_addPhraseDirection(ctx_.get(), 1);
  chewing_set_phraseChoiceRearward(ctx_.get(), 1);
  chewing_set_escCleanAllBuf(ctx_.get(), 0);
  chewing_set_ChiEngMode(ctx_.get(), CHINESE_MODE);
  chewing_set_ShapeMode(ctx_.get(), HALFSHAPE_MODE);

  const int kb_num = chewing_KBStr2Num(layout_to_chewing_name(layout_));
  chewing_set_KBType(ctx_.get(), kb_num);
}

ChewingEngine::~ChewingEngine() = default;
ChewingEngine::ChewingEngine(ChewingEngine&&) noexcept = default;
ChewingEngine& ChewingEngine::operator=(ChewingEngine&&) noexcept = default;

EngineSnapshot ChewingEngine::snapshot() const {
  EngineSnapshot s;
  ChewingContext* c = ctx_.get();

  if (chewing_commit_Check(c)) {
    if (const char* str = chewing_commit_String_static(c); str != nullptr) {
      s.newly_committed = str;
    }
  }
  if (chewing_buffer_Check(c)) {
    if (const char* str = chewing_buffer_String_static(c); str != nullptr) {
      s.preedit = str;
    }
  }
  if (chewing_bopomofo_Check(c)) {
    if (const char* str = chewing_bopomofo_String_static(c); str != nullptr) {
      s.bopomofo = str;
    }
  }
  if (chewing_aux_Check(c)) {
    if (const char* str = chewing_aux_String_static(c); str != nullptr) {
      s.aux_text = str;
    }
  }

  s.cursor_pos = chewing_cursor_Current(c);
  s.chinese_mode = (chewing_get_ChiEngMode(c) == CHINESE_MODE);
  s.fullshape = (chewing_get_ShapeMode(c) == FULLSHAPE_MODE);

  const int total_pages = chewing_cand_TotalPage(c);
  if (total_pages > 0 && !chewing_cand_CheckDone(c)) {
    s.cand_open = true;
    s.cand_total_pages = total_pages;
    s.cand_page = chewing_cand_CurrentPage(c);

    const int per_page = chewing_cand_ChoicePerPage(c);
    chewing_cand_Enumerate(c);
    for (int i = 0; i < per_page && chewing_cand_hasNext(c); ++i) {
      if (const char* cand = chewing_cand_String_static(c); cand != nullptr) {
        s.candidates.emplace_back(cand);
      }
    }
  }

  return s;
}

EngineSnapshot ChewingEngine::take_snapshot() {
  EngineSnapshot s = snapshot();
  if (!s.newly_committed.empty()) {
    chewing_ack(ctx_.get());
  }
  return s;
}

EngineSnapshot ChewingEngine::handle_default(int key) {
  chewing_handle_Default(ctx_.get(), key);
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_space() {
  chewing_handle_Space(ctx_.get());
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_enter() {
  chewing_handle_Enter(ctx_.get());
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_esc() {
  chewing_handle_Esc(ctx_.get());
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_backspace() {
  chewing_handle_Backspace(ctx_.get());
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_del() {
  chewing_handle_Del(ctx_.get());
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_left() {
  chewing_handle_Left(ctx_.get());
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_right() {
  chewing_handle_Right(ctx_.get());
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_up() {
  chewing_handle_Up(ctx_.get());
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_down() {
  chewing_handle_Down(ctx_.get());
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_home() {
  chewing_handle_Home(ctx_.get());
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_end() {
  chewing_handle_End(ctx_.get());
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_page_up() {
  chewing_handle_PageUp(ctx_.get());
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_page_down() {
  chewing_handle_PageDown(ctx_.get());
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_tab() {
  chewing_handle_Tab(ctx_.get());
  return take_snapshot();
}

EngineSnapshot ChewingEngine::handle_ctrl_num(int digit_char) {
  chewing_handle_CtrlNum(ctx_.get(), digit_char);
  return take_snapshot();
}

EngineSnapshot ChewingEngine::toggle_chinese_mode() {
  const int cur = chewing_get_ChiEngMode(ctx_.get());
  chewing_set_ChiEngMode(ctx_.get(), cur == CHINESE_MODE ? SYMBOL_MODE : CHINESE_MODE);
  return take_snapshot();
}

EngineSnapshot ChewingEngine::toggle_fullshape() {
  const int cur = chewing_get_ShapeMode(ctx_.get());
  chewing_set_ShapeMode(ctx_.get(), cur == FULLSHAPE_MODE ? HALFSHAPE_MODE : FULLSHAPE_MODE);
  return take_snapshot();
}

EngineSnapshot ChewingEngine::flush_preedit() {
  ChewingContext* c = ctx_.get();
  if (chewing_cand_TotalPage(c) > 0 && !chewing_cand_CheckDone(c)) {
    chewing_cand_close(c);
  }
  if (chewing_bopomofo_Check(c)) {
    chewing_clean_bopomofo_buf(c);
  }
  if (chewing_buffer_Check(c)) {
    chewing_commit_preedit_buf(c);
  }
  return take_snapshot();
}

}  // namespace zhuyin
