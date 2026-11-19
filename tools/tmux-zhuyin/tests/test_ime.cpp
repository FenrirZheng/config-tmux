#include <gtest/gtest.h>

#include <filesystem>
#include <string>

#include "chewing_engine.hpp"
#include "ime_session.hpp"

namespace zhuyin {
namespace {

class IsolatedImeTest : public ::testing::Test {
 protected:
  void SetUp() override {
    tmp_dir_ = std::filesystem::temp_directory_path() /
               ("tmux-zhuyin-test-" +
                std::to_string(reinterpret_cast<std::uintptr_t>(this)));
    std::filesystem::create_directories(tmp_dir_);
    user_db_ = (tmp_dir_ / "uhash.sqlite3").string();
  }

  void TearDown() override {
    std::error_code ec;
    std::filesystem::remove_all(tmp_dir_, ec);
  }

  ImeSession make_session(KeyboardLayout layout = KeyboardLayout::Default) {
    return ImeSession(ChewingEngine(layout, user_db_));
  }

  static SessionAction feed_ascii(ImeSession& s, std::string_view keys) {
    SessionAction last = SessionAction::Continue;
    for (char c : keys) {
      if (c == ' ') {
        last = s.handle_key({KeyKind::Space, ' '});
      } else {
        last = s.handle_key({KeyKind::Char, static_cast<unsigned char>(c)});
      }
    }
    return last;
  }

  std::filesystem::path tmp_dir_;
  std::string user_db_;
};

TEST_F(IsolatedImeTest, BasicZhuyinCompositionAndTwoStageEnter) {
  auto session = make_session();

  // Standard DaChen: 5j/ <Space> = 中 (ㄓㄨㄥ), jp6 = 文 (ㄨㄣˊ)
  feed_ascii(session, "5j/ jp6");
  EXPECT_EQ(session.state().preedit, "中文");
  EXPECT_TRUE(session.committed_text().empty());

  // First Enter commits preedit into committed_text_, stays open.
  EXPECT_EQ(session.handle_key({KeyKind::Enter, 0}), SessionAction::Continue);
  EXPECT_EQ(session.committed_text(), "中文");
  EXPECT_TRUE(session.state().preedit.empty());

  // Second Enter submits when preedit is empty and committed_text_ is non-empty.
  EXPECT_EQ(session.handle_key({KeyKind::Enter, 0}), SessionAction::Submit);
  EXPECT_EQ(session.committed_text(), "中文");
}

TEST_F(IsolatedImeTest, SubmitNowFlushesPreeditImmediately) {
  auto session = make_session();
  feed_ascii(session, "5j/ jp6");
  EXPECT_EQ(session.state().preedit, "中文");

  EXPECT_EQ(session.handle_key({KeyKind::SubmitNow, 0}), SessionAction::Submit);
  EXPECT_EQ(session.committed_text(), "中文");
}

TEST_F(IsolatedImeTest, CandidateSelectionAndEscapeClosesCandidatesFirst) {
  auto session = make_session();
  feed_ascii(session, "5j/ ");
  ASSERT_EQ(session.state().preedit, "中");

  // Open candidate list with Down arrow.
  session.handle_key({KeyKind::Down, 0});
  EXPECT_TRUE(session.state().cand_open);
  EXPECT_FALSE(session.state().candidates.empty());

  // Esc closes candidate list without cancelling the popup.
  EXPECT_EQ(session.handle_key({KeyKind::Escape, 0}), SessionAction::Continue);
  EXPECT_FALSE(session.state().cand_open);

  // Open again and select candidate #2.
  session.handle_key({KeyKind::Down, 0});
  ASSERT_TRUE(session.state().cand_open);
  ASSERT_GE(session.state().candidates.size(), 2u);
  const std::string second_cand = session.state().candidates[1];
  session.handle_key({KeyKind::Char, '2'});
  EXPECT_FALSE(session.state().cand_open);
  EXPECT_EQ(session.state().preedit, second_cand);
}

TEST_F(IsolatedImeTest, BackspaceAcrossPreeditIntoCommittedUtf8) {
  auto session = make_session();
  feed_ascii(session, "5j/ jp6");
  session.handle_key({KeyKind::Enter, 0});
  ASSERT_EQ(session.committed_text(), "中文");

  // Preedit is empty: Backspace pops one UTF-8 character ("文"), leaving "中".
  session.handle_key({KeyKind::Backspace, 0});
  EXPECT_EQ(session.committed_text(), "中");

  session.handle_key({KeyKind::Backspace, 0});
  EXPECT_TRUE(session.committed_text().empty());
}

TEST_F(IsolatedImeTest, ModeToggleChineseAndEnglishMix) {
  auto session = make_session();
  feed_ascii(session, "5j/ jp6");
  session.handle_key({KeyKind::Enter, 0});

  // Tab switches to English mode when candidate list is closed.
  session.handle_key({KeyKind::Tab, 0});
  EXPECT_FALSE(session.state().chinese_mode);

  feed_ascii(session, "ABC");
  EXPECT_EQ(session.committed_text(), "中文ABC");

  // Tab back to Chinese mode.
  session.handle_key({KeyKind::Tab, 0});
  EXPECT_TRUE(session.state().chinese_mode);
}

TEST_F(IsolatedImeTest, CancelNowAndEscapeWhenIdle) {
  auto session = make_session();
  EXPECT_EQ(session.handle_key({KeyKind::Escape, 0}), SessionAction::Cancel);

  auto session2 = make_session();
  feed_ascii(session2, "5j/ ");
  EXPECT_EQ(session2.handle_key({KeyKind::CancelNow, 0}), SessionAction::Cancel);
}

TEST_F(IsolatedImeTest, ParseKeyboardLayouts) {
  EXPECT_EQ(parse_keyboard_layout("default"), KeyboardLayout::Default);
  EXPECT_EQ(parse_keyboard_layout("hsu"), KeyboardLayout::Hsu);
  EXPECT_EQ(parse_keyboard_layout("et"), KeyboardLayout::Eten);
  EXPECT_EQ(parse_keyboard_layout("nonexistent"), std::nullopt);
}

}  // namespace
}  // namespace zhuyin
