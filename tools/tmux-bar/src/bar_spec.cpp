#include "bar_spec.hpp"

#include <sstream>
#include <utility>

namespace tmux_bar {

namespace {

std::string escape_tmux_regex(std::string_view raw) {
  std::string out;
  out.reserve(raw.size());
  for (char ch : raw) {
    switch (ch) {
      case '\\':
      case '^':
      case '$':
      case '.':
      case '|':
      case '?':
      case '*':
      case '+':
      case '(':
      case ')':
      case '[':
      case ']':
      case '{':
      case '}':
        out.push_back('\\');
        out.push_back(ch);
        break;
      default:
        out.push_back(ch);
        break;
    }
  }
  return out;
}

std::string format_home_path(std::string_view home_dir) {
  while (home_dir.size() > 1 && home_dir.back() == '/') {
    home_dir.remove_suffix(1);
  }
  if (home_dir.empty() || home_dir == "/") {
    return "#{pane_current_path}";
  }
  // Exact match for $HOME itself, or anchored $HOME/ prefix, so /home/fenrir2
  // is never mangled into ~2.
  std::ostringstream oss;
  oss << "#{?#{==:#{pane_current_path}," << home_dir << "},~,"
      << "#{s|^" << escape_tmux_regex(home_dir) << "/|~/|:pane_current_path}}";
  return oss.str();
}

}  // namespace

std::string any_pane_state(std::string_view state) {
  // Ignore stale @claude_state left behind when agy exits (agy has no SessionEnd hook).
  static constexpr std::string_view kEffectiveState =
      "#{?#{&&:#{==:#{@claude_agent},agy},#{!=:#{pane_current_command},agy}},,#{@claude_state}}|";
  std::ostringstream oss;
  oss << "#{m:*" << state << "*,#{P:" << kEffectiveState << "," << kEffectiveState << "}}";
  return oss.str();
}

std::string window_glyph() {
  std::ostringstream oss;
  oss << "#{?" << any_pane_state("needs-input")
      << ",#[fg=" << palette::kRed << "] " << glyph::kDot
      << ",#{?" << any_pane_state("busy")
      << ",#[fg=" << palette::kYellow << "] " << glyph::kDot
      << ",#{?" << any_pane_state("idle")
      << ",#[fg=" << palette::kGreen << "] " << glyph::kDot
      << ",}}}";
  return oss.str();
}

std::string window_glyph_mono() {
  std::ostringstream oss;
  oss << "#{?" << any_pane_state("needs-input")
      << ", " << glyph::kDot
      << ",#{?" << any_pane_state("busy")
      << ", " << glyph::kDot
      << ",#{?" << any_pane_state("idle")
      << ", " << glyph::kDot
      << ",}}}";
  return oss.str();
}

std::string fleet_segment() {
  const std::string blocked = "#{W:#{?" + any_pane_state("needs-input") + ",x,}}";
  std::ostringstream oss;
  oss << "#{?" << blocked
      << ",#[fg=" << palette::kRed << "]#[bold]" << glyph::kDot
      << " #{n:" << blocked << "} ,}#[default]";
  return oss.str();
}

std::string window_status_format() {
  // Symmetric 1-cell padding (` #I... `) matches the active tab so switching
  // windows never shifts the tab bar horizontally. Needs-input windows gently
  // lift #I to bright bold text + red dot instead of painting a harsh bell box.
  std::ostringstream oss;
  oss << " #{?" << any_pane_state("needs-input")
      << ",#[fg=" << palette::kText << "]#[bold]"
      << ",#[fg=" << palette::kComment << "]}"
      << "#I#[nobold]"
      << "#{?#{||:#{automatic-rename},#{m/r:^cc:,#{window_name}}},, "
      << "#[fg=" << palette::kSubtext << "]#{=14:window_name}}"
      << "#{E:@claude_tab_glyph} #[default]";
  return oss.str();
}

std::string window_status_current_format() {
  // Soft elevated Tokyo Night selection surface (#283457): dark enough that
  // the colored status dot stays legible right inside the active tab.
  std::ostringstream oss;
  oss << "#[bg=" << palette::kSelection << "]#[fg=" << palette::kText << "]#[bold] "
      << "#I"
      << "#{?#{||:#{automatic-rename},#{m/r:^cc:,#{window_name}}},, #{=14:window_name}}"
      << "#[nobold]#{E:@claude_tab_glyph} #[default]";
  return oss.str();
}

std::string status_left_format() {
  std::ostringstream oss;
  oss << "#{?client_prefix,"
      << "#[fg=" << palette::kBlack << "]#[bg=" << palette::kBlue << "]#[bold] #S #[default] ,"
      << "#{?#{m/r:^[0-9]+$,#{session_name}},"
      << "#[fg=" << palette::kMuted << "] ,"
      << "#[fg=" << palette::kMuted << "] #S  }}";
  return oss.str();
}

std::string status_right_format(std::string_view home_dir) {
  std::ostringstream oss;
  oss << "#[fg=" << palette::kMuted << "]" << glyph::kGit << " "
      << "#[fg=" << palette::kText << "]#(cd #{q:pane_current_path} && gitmux -cfg ~/.gitmux.conf)"
      << "  #[fg=" << palette::kBorder << "]│  "
      << "#[fg=" << palette::kSubtext << "]" << format_home_path(home_dir) << "  "
      << fleet_segment();
  return oss.str();
}

std::vector<TmuxOption> build_options(std::string_view home_dir) {
  std::vector<TmuxOption> opts;
  opts.reserve(32);

  auto add_g = [&opts](std::string name, std::string value) {
    opts.push_back(TmuxOption{Scope::Global, std::move(name), std::move(value)});
  };
  auto add_gw = [&opts](std::string name, std::string value) {
    opts.push_back(TmuxOption{Scope::GlobalWindow, std::move(name), std::move(value)});
  };

  // ── Agent tab glyphs (embedded by window-status-* via #{E:...}) ──────────
  add_g("@claude_tab_glyph", window_glyph());
  add_g("@claude_tab_glyph_mono", window_glyph_mono());

  // ── Status bar base & layout (Tokyo Night) ───────────────────────────────
  add_g("status-style",
        std::string("bg=") + std::string(palette::kBgBar) + ",fg=" + std::string(palette::kText));
  add_g("status-left-length", "40");
  add_g("status-right-length", "100");
  add_g("status-left-style", "default");
  add_g("status-right-style", "default");
  add_g("status-left", status_left_format());
  add_g("status-right", status_right_format(home_dir));

  // ── Window tabs (soft one-block, zero-jitter symmetric padding) ──────────
  add_gw("window-status-separator", " ");
  add_gw("window-status-style", "default");
  add_gw("window-status-current-style", "default");
  add_gw("window-status-last-style", "default");
  add_gw("window-status-activity-style", "default");
  add_gw("window-status-bell-style", "default");
  add_gw("window-status-format", window_status_format());
  add_gw("window-status-current-format", window_status_current_format());

  // ── Pane borders, menus, clock & interactive styles (Tokyo Night) ────────
  // window-style / window-active-style stay unset to keep Alacritty translucency.
  add_gw("pane-border-style", std::string("fg=") + std::string(palette::kBorder));
  add_gw("pane-active-border-style",
         std::string("#{?pane_in_mode,fg=") + std::string(palette::kBlue) +
             ",#{?pane_synchronized,fg=" + std::string(palette::kMagenta) +
             ",fg=" + std::string(palette::kBlue) + "}}");
  add_gw("menu-selected-style",
         std::string("fg=") + std::string(palette::kText) + ",bold,bg=" + std::string(palette::kSelection));
  add_gw("clock-mode-colour", std::string(palette::kBlue));
  add_gw("mode-style",
         std::string("fg=") + std::string(palette::kText) + ",bg=" + std::string(palette::kSelection) + ",bold");
  add_g("message-style",
        std::string("fg=") + std::string(palette::kText) + ",bg=" + std::string(palette::kSurface));
  add_g("message-command-style",
        std::string("fg=") + std::string(palette::kBlue) + ",bg=" + std::string(palette::kSurface));
  add_gw("popup-style",
         std::string("fg=") + std::string(palette::kText) + ",bg=" + std::string(palette::kBgBar));
  add_gw("popup-border-style", std::string("fg=") + std::string(palette::kBlue));
  add_gw("copy-mode-match-style",
         std::string("bg=") + std::string(palette::kYellow) + ",fg=" + std::string(palette::kBlack));
  add_gw("copy-mode-current-match-style",
         std::string("bg=") + std::string(palette::kOrange) + ",fg=" + std::string(palette::kBlack));
  add_gw("copy-mode-mark-style",
         std::string("bg=") + std::string(palette::kRed) + ",fg=" + std::string(palette::kBlack));

  return opts;
}

std::string_view existing_window_border_rule() {
  return "#{?#{==:#{window_panes},1},#{?#{||:#{pane_pipe},#{pane_marked}},top,off},top}";
}

std::string tmux_quote(std::string_view text) {
  std::string out;
  out.reserve(text.size() + 2);
  out.push_back('"');
  for (char ch : text) {
    if (ch == '\\' || ch == '"' || ch == '$') {
      out.push_back('\\');
    }
    out.push_back(ch);
  }
  out.push_back('"');
  return out;
}

std::string render_tmux_script(const std::vector<TmuxOption>& options) {
  std::ostringstream oss;
  for (const auto& opt : options) {
    oss << "set-option " << (opt.scope == Scope::GlobalWindow ? "-gw" : "-g")
        << ' ' << opt.name << ' ' << tmux_quote(opt.value) << '\n';
  }
  return oss.str();
}

}  // namespace tmux_bar
