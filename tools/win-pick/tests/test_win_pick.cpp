#include "model.hpp"

#include <cstdlib>
#include <iostream>
#include <string>
#include <string_view>

namespace {

int g_failures = 0;

void check(bool cond, std::string_view msg, int line) {
  if (!cond) {
    std::cerr << "FAIL (line " << line << "): " << msg << "\n";
    ++g_failures;
  }
}

#define CHECK(expr) check((expr), #expr, __LINE__)

void test_parse_panes_and_agy_guard() {
  // 16 tab-separated fields per pane.
  const std::string raw =
      "$1\tmain\t@1\t0\tzsh\t1\t0\t0\t%1\t1\tzsh\t/home/u\t\t\t\t\n"
      "$1\tmain\t@2\t1\tclaude\t0\t0\t0\t%2\t1\tclaude\t/home/u/proj\tbusy\tclaude\tediting\tfix bug\n"
      "$1\tmain\t@2\t1\tclaude\t0\t0\t0\t%3\t0\tzsh\t/home/u/proj\tneeds-input\tclaude\t\tapprove?\n"
      "$1\tmain\t@3\t5\tagy\t1\t1\t0\t%4\t1\tagy\t/home/u/.tmux\tbusy\tagy\t\tworking\n"
      "$1\tmain\t@4\t6\tzsh\t1\t0\t0\t%5\t1\tzsh\t/home/u\tbusy\tagy\t\tstale task\n"
      "$1\tmain\t@5\t12\tnotes\t0\t0\t0\t%6\t1\tvim\t/home/u/docs\t\t\t\t\n"
      "$2\tother\t@9\t1\thtop\t1\t1\t0\t%9\t1\thtop\t/tmp\tidle\tclaude\t\tdone\n";

  const auto wins = win_pick::parse_panes_output(raw, "$1");
  CHECK(wins.size() == 6);

  // Current session ($1) sorted first by window_index: 0, 1, 5, 6, 12, then $2:1.
  CHECK(wins[0].window_index == 0);
  CHECK(wins[1].window_index == 1);
  CHECK(wins[2].window_index == 5);
  CHECK(wins[3].window_index == 6);
  CHECK(wins[4].window_index == 12);
  CHECK(wins[5].session_id == "$2");

  // Window @2 has 2 panes; active pane is %2, worst state is NeedsInput (from %3).
  CHECK(wins[1].pane_count == 2);
  CHECK(wins[1].active_pane_id == "%2");
  CHECK(wins[1].agent_state == win_pick::AgentState::NeedsInput);
  CHECK(wins[1].task_hint == "approve?");

  // Window @3 runs agy -> busy kept.
  CHECK(wins[2].agent_state == win_pick::AgentState::Busy);

  // Window @4 has @claude_agent=agy but command=zsh -> stale guard clears state.
  CHECK(wins[3].agent_state == win_pick::AgentState::None);
  CHECK(wins[3].task_hint.empty());

  // Display label: manual rename vs automatic.
  CHECK(win_pick::display_label(wins[4]) == "notes");
  CHECK(win_pick::display_label(wins[2]) == "agy");
}

void test_filter_digits_and_scope() {
  const std::string raw =
      "$1\tmain\t@1\t0\tzsh\t1\t0\t0\t%1\t1\tzsh\t/home/u\t\t\t\t\n"
      "$1\tmain\t@2\t1\tclaude\t1\t0\t0\t%2\t1\tclaude\t/home/u\tbusy\tclaude\t\t\n"
      "$1\tmain\t@3\t5\tagy\t1\t1\t0\t%4\t1\tagy\t/home/u/.tmux\t\t\t\t\n"
      "$1\tmain\t@5\t12\tnotes\t0\t0\t0\t%6\t1\tvim\t/home/u/docs\t\t\t\t\n"
      "$2\tother\t@9\t1\thtop\t1\t1\t0\t%9\t1\thtop\t/tmp\t\t\t\t\n";

  const auto wins = win_pick::parse_panes_output(raw, "$1");

  // Default scope: current session only (4 windows).
  const auto scoped = win_pick::filter_windows(wins, "", false, "$1");
  CHECK(scoped.size() == 4);

  // All sessions scope: 5 windows.
  const auto all = win_pick::filter_windows(wins, "", true, "$1");
  CHECK(all.size() == 5);

  // Typing "1" in current session -> exact index 1 first, prefix 12 second.
  const auto q1 = win_pick::filter_windows(wins, "1", false, "$1");
  CHECK(q1.size() == 2);
  CHECK(wins[q1[0]].window_index == 1);
  CHECK(wins[q1[1]].window_index == 12);

  // Typing "12" -> exact index 12 only.
  const auto q12 = win_pick::filter_windows(wins, "12", false, "$1");
  CHECK(q12.size() == 1);
  CHECK(wins[q12[0]].window_index == 12);

  // Typing "5" -> exact index 5.
  const auto q5 = win_pick::filter_windows(wins, "5", false, "$1");
  CHECK(q5.size() == 1);
  CHECK(wins[q5[0]].window_index == 5);

  // Text query: case-insensitive substring on path/command.
  const auto qtxt = win_pick::filter_windows(wins, "TMUX", false, "$1");
  CHECK(qtxt.size() == 1);
  CHECK(wins[qtxt[0]].window_index == 5);

  // Text query across all sessions matches session_name.
  const auto qsess = win_pick::filter_windows(wins, "other", true, "$1");
  CHECK(qsess.size() == 1);
  CHECK(wins[qsess[0]].session_name == "other");
}

void test_ansi_parser() {
  // 16-color + bold + RGB + reset + OSC stripped + tab expanded.
  const std::string line =
      "\x1b]0;window title\x07"
      "\x1b[1;31mERR\x1b[0m\t\x1b[38;2;122;162;247mblue\x1b[38;5;208m256\x1b[0m";
  const auto spans = win_pick::parse_ansi_line(line);
  CHECK(spans.size() == 4);
  CHECK(spans[0].text == "ERR");
  CHECK(spans[0].style.bold);
  CHECK(spans[0].style.fg.kind == win_pick::AnsiColor::Kind::Palette16);
  CHECK(spans[0].style.fg.index == 1);

  CHECK(spans[1].text == " ");  // tab from col 3 -> col 4 (1 space)
  CHECK(!spans[1].style.bold);

  CHECK(spans[2].text == "blue");
  CHECK(spans[2].style.fg.kind == win_pick::AnsiColor::Kind::Rgb);
  CHECK(spans[2].style.fg.r == 122 && spans[2].style.fg.g == 162 && spans[2].style.fg.b == 247);

  CHECK(spans[3].text == "256");
  CHECK(spans[3].style.fg.kind == win_pick::AnsiColor::Kind::Palette256);
  CHECK(spans[3].style.fg.index == 208);

  // Trailing blank lines trimmed.
  const auto cap = win_pick::parse_ansi_capture("hello\nworld\n   \n\n");
  CHECK(cap.size() == 2);
}

void test_shorten_path() {
  CHECK(win_pick::shorten_path("/home/fenrir", "/home/fenrir") == "~");
  CHECK(win_pick::shorten_path("/home/fenrir/.tmux", "/home/fenrir") == "~/.tmux");
  CHECK(win_pick::shorten_path("/home/fenrir2/x", "/home/fenrir") == "/home/fenrir2/x");
}

}  // namespace

int main() {
  test_parse_panes_and_agy_guard();
  test_filter_digits_and_scope();
  test_ansi_parser();
  test_shorten_path();
  if (g_failures > 0) {
    std::cerr << g_failures << " test(s) failed\n";
    return EXIT_FAILURE;
  }
  std::cout << "All win-pick unit tests passed.\n";
  return EXIT_SUCCESS;
}
