#include "bar_spec.hpp"

#include <cstdlib>
#include <iostream>
#include <optional>
#include <string>
#include <string_view>

namespace {

int g_failures = 0;

void expect(bool cond, std::string_view msg) {
  if (!cond) {
    std::cerr << "FAIL: " << msg << '\n';
    ++g_failures;
  }
}

std::optional<tmux_bar::TmuxOption> find_opt(const std::vector<tmux_bar::TmuxOption>& opts,
                                             std::string_view name) {
  for (const auto& opt : opts) {
    if (opt.name == name) {
      return opt;
    }
  }
  return std::nullopt;
}

void test_window_formats_are_number_only_with_manual_name_and_glyph() {
  const std::string inactive = tmux_bar::window_status_format();
  const std::string active = tmux_bar::window_status_current_format();

  // Must not read pane_title or pane_current_path.
  expect(inactive.find("pane_title") == std::string::npos,
         "inactive tab must not reference pane_title");
  expect(active.find("pane_title") == std::string::npos,
         "active tab must not reference pane_title");
  expect(inactive.find("pane_current_path") == std::string::npos,
         "inactive tab must not reference pane_current_path");

  // Must include #I, manual rename guard (automatic-rename || ^cc:), symmetric padding, and colored dot glyph.
  expect(inactive.find("#I#[nobold]#{?#{||:#{automatic-rename},#{m/r:^cc:,#{window_name}}},, ") !=
             std::string::npos,
         "inactive tab must place leading space inside manual-name branch");
  expect(active.find(" #I#{?#{||:#{automatic-rename},#{m/r:^cc:,#{window_name}}},, #{=14:window_name}}") !=
             std::string::npos,
         "active tab must place leading space inside manual-name branch");
  expect(inactive.find("#{E:@claude_tab_glyph} #[default]") != std::string::npos,
         "inactive tab must have symmetric trailing space");
  expect(active.find("#[nobold]#{E:@claude_tab_glyph} #[default]") != std::string::npos,
         "active tab must embed colored @claude_tab_glyph with symmetric trailing space");
}

void test_claude_glyph_scans_both_active_and_inactive_panes() {
  const std::string cond = tmux_bar::any_pane_state("needs-input");
  const std::string effective =
      "#{?#{&&:#{==:#{@claude_agent},agy},#{!=:#{pane_current_command},agy}},,#{@claude_state}}|";
  expect(cond == "#{m:*needs-input*,#{P:" + effective + "," + effective + "}}",
         "any_pane_state must guard stale agy state and pass format twice to #{P:A,B}");

  const std::string g = tmux_bar::window_glyph();
  expect(g.find(tmux_bar::glyph::kDot) != std::string::npos, "glyph uses minimal dot");
  expect(g.find(tmux_bar::palette::kRed) != std::string::npos, "glyph has Tokyo Night red");
  expect(g.find(tmux_bar::palette::kYellow) != std::string::npos, "glyph has Tokyo Night yellow");
  expect(g.find(tmux_bar::palette::kGreen) != std::string::npos, "glyph has Tokyo Night green");

  const std::string mono = tmux_bar::window_glyph_mono();
  expect(mono.find("#[fg=") == std::string::npos, "mono glyph must not set foreground colour");
}

void test_status_right_uses_home_and_includes_gitmux_and_fleet() {
  const std::string sr = tmux_bar::status_right_format("/home/fenrir/");
  expect(sr.find("gitmux -cfg ~/.gitmux.conf") != std::string::npos, "status-right has gitmux");
  expect(sr.find("#{?#{==:#{pane_current_path},/home/fenrir},~,#{s|^/home/fenrir/|~/|:pane_current_path}}") !=
             std::string::npos,
         "status-right matches exact home or home/ prefix and strips trailing slash");
  expect(sr.find("#{W:#{?#{m:*needs-input*") != std::string::npos,
         "status-right embeds fleet counter directly");

  const std::string sr_meta = tmux_bar::status_right_format("/home/a.b");
  expect(sr_meta.find("#{s|^/home/a\\.b/|~/|:pane_current_path}") != std::string::npos,
         "home regex metacharacters are escaped");

  const std::string sr_empty = tmux_bar::status_right_format("");
  expect(sr_empty.find("#{pane_current_path}") != std::string::npos,
         "empty home falls back to plain pane_current_path");
}

void test_all_option_values_are_well_formed() {
  const auto opts = tmux_bar::build_options("/home/fenrir");
  for (const auto& opt : opts) {
    expect(opt.value.empty() || opt.value.back() != ';',
           "option value must not end with ';' (tmux command separator): " + opt.name);
    int brace_depth = 0;
    int bracket_depth = 0;
    for (char ch : opt.value) {
      if (ch == '{') {
        ++brace_depth;
      } else if (ch == '}') {
        --brace_depth;
      } else if (ch == '[') {
        ++bracket_depth;
      } else if (ch == ']') {
        --bracket_depth;
      }
      expect(brace_depth >= 0, "unmatched '}' in " + opt.name);
      expect(bracket_depth >= 0, "unmatched ']' in " + opt.name);
    }
    expect(brace_depth == 0, "unbalanced '{}' in " + opt.name);
    expect(bracket_depth == 0, "unbalanced '[]' in " + opt.name);
  }
}

void test_build_options_contains_full_theme_and_bar() {
  const auto opts = tmux_bar::build_options("/home/fenrir");
  expect(find_opt(opts, "@claude_tab_glyph").has_value(), "has @claude_tab_glyph");
  expect(find_opt(opts, "@claude_tab_glyph_mono").has_value(), "has @claude_tab_glyph_mono");
  expect(find_opt(opts, "status-style").has_value(), "has status-style");
  expect(find_opt(opts, "status-left").has_value(), "has status-left");
  expect(find_opt(opts, "status-right").has_value(), "has status-right");
  expect(find_opt(opts, "window-status-format").has_value(), "has window-status-format");
  expect(find_opt(opts, "window-status-current-format").has_value(), "has window-status-current-format");
  expect(find_opt(opts, "pane-border-style").has_value(), "has pane-border-style");
  expect(find_opt(opts, "pane-active-border-style").has_value(), "has pane-active-border-style");
  expect(find_opt(opts, "popup-border-style").has_value(), "has popup-border-style");
  const auto bell = find_opt(opts, "window-status-bell-style");
  expect(bell.has_value() && bell->value == "default",
         "window-status-bell-style must be default (no yellow clash block)");
  const auto act = find_opt(opts, "window-status-activity-style");
  expect(act.has_value() && act->value == "default",
         "window-status-activity-style must be default");
  expect(!find_opt(opts, "window-style").has_value(), "must not set window-style (preserves translucency)");
  expect(!find_opt(opts, "window-active-style").has_value(), "must not set window-active-style");
}

void test_tmux_quote_and_render_script() {
  expect(tmux_bar::tmux_quote(R"(a"b$c\d)") == R"("a\"b\$c\\d")",
         "tmux_quote escapes double quote, dollar, and backslash");
  const auto script = tmux_bar::render_tmux_script({
      {tmux_bar::Scope::Global, "status-left-length", "40"},
      {tmux_bar::Scope::GlobalWindow, "window-status-separator", "  "},
  });
  expect(script == "set-option -g status-left-length \"40\"\n"
                   "set-option -gw window-status-separator \"  \"\n",
         "render_tmux_script emits -g and -gw set-option lines");
}

}  // namespace

int main() {
  test_window_formats_are_number_only_with_manual_name_and_glyph();
  test_claude_glyph_scans_both_active_and_inactive_panes();
  test_status_right_uses_home_and_includes_gitmux_and_fleet();
  test_all_option_values_are_well_formed();
  test_build_options_contains_full_theme_and_bar();
  test_tmux_quote_and_render_script();

  if (g_failures != 0) {
    std::cerr << g_failures << " test(s) failed.\n";
    return EXIT_FAILURE;
  }
  std::cout << "All tmux-bar unit tests passed.\n";
  return EXIT_SUCCESS;
}
