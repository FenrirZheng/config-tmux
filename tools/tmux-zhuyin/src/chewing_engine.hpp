#pragma once

#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

struct ChewingContext;

namespace zhuyin {

enum class KeyboardLayout {
  Default,
  Hsu,
  Eten,
  Eten26,
  Dvorak,
  GinYieh,
  DachenCp26,
};

std::optional<KeyboardLayout> parse_keyboard_layout(std::string_view name);
std::string_view keyboard_layout_label(KeyboardLayout layout);

struct EngineSnapshot {
  std::string newly_committed;
  std::string preedit;
  std::string bopomofo;
  std::string aux_text;
  std::vector<std::string> candidates;
  int cursor_pos = 0;
  int cand_page = 0;
  int cand_total_pages = 0;
  bool chinese_mode = true;
  bool fullshape = false;
  bool cand_open = false;

  [[nodiscard]] bool has_composition() const {
    return !preedit.empty() || !bopomofo.empty() || cand_open;
  }
};

class ChewingEngine {
 public:
  explicit ChewingEngine(KeyboardLayout layout = KeyboardLayout::Default,
                         std::optional<std::string> user_data_path = std::nullopt,
                         std::optional<std::string> sys_data_path = std::nullopt);
  ~ChewingEngine();

  ChewingEngine(const ChewingEngine&) = delete;
  ChewingEngine& operator=(const ChewingEngine&) = delete;
  ChewingEngine(ChewingEngine&&) noexcept;
  ChewingEngine& operator=(ChewingEngine&&) noexcept;

  [[nodiscard]] KeyboardLayout layout() const { return layout_; }
  [[nodiscard]] EngineSnapshot snapshot() const;

  EngineSnapshot handle_default(int key);
  EngineSnapshot handle_space();
  EngineSnapshot handle_enter();
  EngineSnapshot handle_esc();
  EngineSnapshot handle_backspace();
  EngineSnapshot handle_del();
  EngineSnapshot handle_left();
  EngineSnapshot handle_right();
  EngineSnapshot handle_up();
  EngineSnapshot handle_down();
  EngineSnapshot handle_home();
  EngineSnapshot handle_end();
  EngineSnapshot handle_page_up();
  EngineSnapshot handle_page_down();
  EngineSnapshot handle_tab();
  EngineSnapshot handle_ctrl_num(int digit_char);

  EngineSnapshot toggle_chinese_mode();
  EngineSnapshot toggle_fullshape();

  // Commits any active preedit buffer into newly_committed and clears incomplete bopomofo.
  EngineSnapshot flush_preedit();

 private:
  struct Deleter {
    void operator()(ChewingContext* ctx) const noexcept;
  };

  EngineSnapshot take_snapshot();

  std::unique_ptr<ChewingContext, Deleter> ctx_;
  KeyboardLayout layout_;
};

}  // namespace zhuyin
