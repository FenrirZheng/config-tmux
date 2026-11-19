#include "model.hpp"

#include <algorithm>
#include <cctype>
#include <charconv>
#include <unordered_map>

namespace win_pick {

namespace {

std::string to_lower_ascii(std::string_view s) {
  std::string out;
  out.reserve(s.size());
  for (char raw_c : s) {
    const auto c = static_cast<unsigned char>(raw_c);
    out.push_back(static_cast<char>(std::tolower(c)));
  }
  return out;
}

bool is_pure_digits(std::string_view s) {
  if (s.empty()) return false;
  for (char raw_c : s) {
    const auto c = static_cast<unsigned char>(raw_c);
    if (c < '0' || c > '9') return false;
  }
  return true;
}

std::string_view trim_ascii(std::string_view s) {
  while (!s.empty() && (s.front() == ' ' || s.front() == '\t')) s.remove_prefix(1);
  while (!s.empty() && (s.back() == ' ' || s.back() == '\t' || s.back() == '\r')) s.remove_suffix(1);
  return s;
}

int parse_int_or(std::string_view s, int fallback) {
  int val = fallback;
  const auto [ptr, ec] = std::from_chars(s.data(), s.data() + s.size(), val);
  if (ec != std::errc{} || ptr != s.data() + s.size()) return fallback;
  return val;
}

bool parse_bool_flag(std::string_view s) {
  return s == "1" || s == "on" || s == "true";
}

AgentState parse_agent_state(std::string_view state,
                             std::string_view agent,
                             std::string_view command) {
  // Stale-agy liveness guard matching tmux-bar and tmuxlib:
  // agy has no SessionEnd hook, so ignore @claude_state when the pane no longer runs agy.
  if (agent == "agy" && command != "agy") {
    return AgentState::None;
  }
  if (state == "needs-input") return AgentState::NeedsInput;
  if (state == "busy") return AgentState::Busy;
  if (state == "idle") return AgentState::Idle;
  return AgentState::None;
}

void append_corpus(std::string& corpus, std::string_view piece) {
  if (piece.empty()) return;
  if (!corpus.empty()) corpus.push_back(' ');
  corpus.append(to_lower_ascii(piece));
}

void split_tabs(std::string_view line, std::vector<std::string_view>& fields) {
  fields.clear();
  std::size_t start = 0;
  while (start <= line.size()) {
    const std::size_t pos = line.find('\t', start);
    if (pos == std::string_view::npos) {
      fields.push_back(line.substr(start));
      break;
    }
    fields.push_back(line.substr(start, pos - start));
    start = pos + 1;
  }
}

void apply_sgr_params(const std::vector<int>& params, AnsiStyle& style) {
  if (params.empty()) {
    style = AnsiStyle{};
    return;
  }
  for (std::size_t i = 0; i < params.size(); ++i) {
    const int p = params[i];
    switch (p) {
      case 0:
        style = AnsiStyle{};
        break;
      case 1:
        style.bold = true;
        break;
      case 2:
        style.dim = true;
        break;
      case 3:
        style.italic = true;
        break;
      case 4:
        style.underlined = true;
        break;
      case 7:
        style.inverted = true;
        break;
      case 22:
        style.bold = false;
        style.dim = false;
        break;
      case 23:
        style.italic = false;
        break;
      case 24:
        style.underlined = false;
        break;
      case 27:
        style.inverted = false;
        break;
      case 39:
        style.fg = AnsiColor{};
        break;
      case 49:
        style.bg = AnsiColor{};
        break;
      case 38:
      case 48: {
        AnsiColor& target = (p == 38) ? style.fg : style.bg;
        if (i + 2 < params.size() && params[i + 1] == 5) {
          const int idx = std::clamp(params[i + 2], 0, 255);
          target = AnsiColor{AnsiColor::Kind::Palette256, static_cast<std::uint8_t>(idx), 0, 0, 0};
          i += 2;
        } else if (i + 4 < params.size() && params[i + 1] == 2) {
          target = AnsiColor{
              AnsiColor::Kind::Rgb,
              0,
              static_cast<std::uint8_t>(std::clamp(params[i + 2], 0, 255)),
              static_cast<std::uint8_t>(std::clamp(params[i + 3], 0, 255)),
              static_cast<std::uint8_t>(std::clamp(params[i + 4], 0, 255)),
          };
          i += 4;
        }
        break;
      }
      default:
        if (p >= 30 && p <= 37) {
          style.fg = AnsiColor{AnsiColor::Kind::Palette16, static_cast<std::uint8_t>(p - 30), 0, 0, 0};
        } else if (p >= 90 && p <= 97) {
          style.fg = AnsiColor{AnsiColor::Kind::Palette16, static_cast<std::uint8_t>(p - 90 + 8), 0, 0, 0};
        } else if (p >= 40 && p <= 47) {
          style.bg = AnsiColor{AnsiColor::Kind::Palette16, static_cast<std::uint8_t>(p - 40), 0, 0, 0};
        } else if (p >= 100 && p <= 107) {
          style.bg = AnsiColor{AnsiColor::Kind::Palette16, static_cast<std::uint8_t>(p - 100 + 8), 0, 0, 0};
        }
        break;
    }
  }
}

void push_span_text(std::vector<AnsiSpan>& spans, std::string_view text, const AnsiStyle& style) {
  if (text.empty()) return;
  if (!spans.empty() && spans.back().style == style) {
    spans.back().text.append(text);
  } else {
    spans.push_back(AnsiSpan{std::string(text), style});
  }
}

}  // namespace

std::vector<WindowEntry> parse_panes_output(std::string_view raw,
                                            std::string_view current_session_id) {
  std::vector<WindowEntry> windows;
  std::unordered_map<std::string, std::size_t> by_window_id;
  std::vector<std::string_view> f;
  f.reserve(16);

  std::size_t pos = 0;
  while (pos < raw.size()) {
    std::size_t nl = raw.find('\n', pos);
    if (nl == std::string_view::npos) nl = raw.size();
    std::string_view line = raw.substr(pos, nl - pos);
    if (!line.empty() && line.back() == '\r') line.remove_suffix(1);
    pos = (nl < raw.size()) ? nl + 1 : raw.size();

    if (line.empty()) continue;
    split_tabs(line, f);
    if (f.size() < 16) continue;

    const std::string_view session_id = f[0];
    const std::string_view session_name = f[1];
    const std::string_view window_id = f[2];
    const int window_index = parse_int_or(f[3], 0);
    const std::string_view window_name = f[4];
    const bool auto_rename = parse_bool_flag(f[5]);
    const bool window_active = parse_bool_flag(f[6]);
    const bool window_bell = parse_bool_flag(f[7]);
    const std::string_view pane_id = f[8];
    const bool pane_active = parse_bool_flag(f[9]);
    const std::string_view command = f[10];
    const std::string_view path = f[11];
    const std::string_view raw_state = f[12];
    const std::string_view raw_agent = f[13];
    const std::string_view activity = f[14];
    const std::string_view task = f[15];

    const AgentState pane_state = parse_agent_state(raw_state, raw_agent, command);
    const std::string_view hint = !activity.empty() ? activity : task;

    auto [it, inserted] = by_window_id.emplace(std::string(window_id), windows.size());
    if (inserted) {
      WindowEntry entry;
      entry.session_id = std::string(session_id);
      entry.session_name = std::string(session_name);
      entry.window_id = std::string(window_id);
      entry.window_index = window_index;
      entry.window_name = std::string(window_name);
      entry.auto_rename = auto_rename;
      entry.window_active = window_active;
      entry.bell = window_bell;
      entry.pane_count = 1;
      entry.active_pane_id = std::string(pane_id);
      entry.command = std::string(command);
      entry.path = std::string(path);
      entry.agent_state = pane_state;
      if (pane_state != AgentState::None) {
        entry.task_hint = std::string(hint);
      }
      append_corpus(entry.search_corpus, window_name);
      append_corpus(entry.search_corpus, command);
      append_corpus(entry.search_corpus, path);
      if (pane_state != AgentState::None) {
        append_corpus(entry.search_corpus, agent_state_name(pane_state));
        append_corpus(entry.search_corpus, hint);
      }
      windows.push_back(std::move(entry));
    } else {
      WindowEntry& entry = windows[it->second];
      entry.pane_count += 1;
      entry.bell = entry.bell || window_bell;
      if (pane_active) {
        entry.active_pane_id = std::string(pane_id);
        entry.command = std::string(command);
        entry.path = std::string(path);
      }
      if (static_cast<int>(pane_state) > static_cast<int>(entry.agent_state)) {
        entry.agent_state = pane_state;
        entry.task_hint = std::string(hint);
      } else if (pane_state == entry.agent_state && pane_state != AgentState::None &&
                 entry.task_hint.empty() && !hint.empty()) {
        entry.task_hint = std::string(hint);
      }
      append_corpus(entry.search_corpus, command);
      append_corpus(entry.search_corpus, path);
      if (pane_state != AgentState::None) {
        append_corpus(entry.search_corpus, agent_state_name(pane_state));
        append_corpus(entry.search_corpus, hint);
      }
    }
  }

  std::sort(windows.begin(), windows.end(), [&](const WindowEntry& a, const WindowEntry& b) {
    const bool a_cur = (!current_session_id.empty() && a.session_id == current_session_id);
    const bool b_cur = (!current_session_id.empty() && b.session_id == current_session_id);
    if (a_cur != b_cur) return a_cur > b_cur;
    if (a.session_name != b.session_name) return a.session_name < b.session_name;
    if (a.window_index != b.window_index) return a.window_index < b.window_index;
    return a.window_id < b.window_id;
  });

  return windows;
}

std::vector<std::size_t> filter_windows(const std::vector<WindowEntry>& windows,
                                        std::string_view query,
                                        bool all_sessions,
                                        std::string_view current_session_id) {
  const std::string_view trimmed = trim_ascii(query);

  struct Ranked {
    int tier;
    std::size_t idx;
  };
  std::vector<Ranked> matches;
  matches.reserve(windows.size());

  const bool digits = is_pure_digits(trimmed);
  const std::string q_lower = to_lower_ascii(trimmed);

  for (std::size_t i = 0; i < windows.size(); ++i) {
    const WindowEntry& w = windows[i];
    if (!all_sessions && !current_session_id.empty() && w.session_id != current_session_id) {
      continue;
    }

    if (trimmed.empty()) {
      matches.push_back({0, i});
      continue;
    }

    const std::string idx_str = std::to_string(w.window_index);
    if (digits) {
      if (idx_str == trimmed) {
        matches.push_back({0, i});
      } else if (idx_str.starts_with(trimmed)) {
        matches.push_back({1, i});
      } else if (w.search_corpus.find(q_lower) != std::string::npos) {
        matches.push_back({2, i});
      }
      continue;
    }

    bool hit = false;
    if (idx_str.find(q_lower) != std::string::npos) {
      hit = true;
    } else if (w.search_corpus.find(q_lower) != std::string::npos) {
      hit = true;
    } else if (all_sessions &&
               to_lower_ascii(w.session_name).find(q_lower) != std::string::npos) {
      hit = true;
    }
    if (hit) {
      matches.push_back({0, i});
    }
  }

  std::stable_sort(matches.begin(), matches.end(), [](const Ranked& a, const Ranked& b) {
    return a.tier < b.tier;
  });

  std::vector<std::size_t> out;
  out.reserve(matches.size());
  for (const Ranked& r : matches) {
    out.push_back(r.idx);
  }
  return out;
}

std::string display_label(const WindowEntry& w) {
  const bool manual = !w.auto_rename && !w.window_name.empty() &&
                      !w.window_name.starts_with("cc:");
  if (manual) return w.window_name;
  if (!w.command.empty()) return w.command;
  if (!w.window_name.empty()) return w.window_name;
  return path_basename(w.path);
}

std::string shorten_path(std::string_view path, std::string_view home) {
  if (!home.empty()) {
    while (home.size() > 1 && home.back() == '/') home.remove_suffix(1);
    if (path == home) return "~";
    if (path.size() > home.size() && path.starts_with(home) && path[home.size()] == '/') {
      std::string out = "~";
      out.append(path.substr(home.size()));
      return out;
    }
  }
  return std::string(path);
}

std::string path_basename(std::string_view path) {
  while (path.size() > 1 && path.back() == '/') path.remove_suffix(1);
  if (path.empty()) return "";
  if (path == "/") return "/";
  const std::size_t slash = path.rfind('/');
  if (slash == std::string_view::npos) return std::string(path);
  return std::string(path.substr(slash + 1));
}

std::string_view agent_state_name(AgentState state) noexcept {
  switch (state) {
    case AgentState::NeedsInput:
      return "needs-input";
    case AgentState::Busy:
      return "busy";
    case AgentState::Idle:
      return "idle";
    case AgentState::None:
      return "";
  }
  return "";
}

std::vector<AnsiSpan> parse_ansi_line(std::string_view line) {
  std::vector<AnsiSpan> spans;
  AnsiStyle style;
  std::string buf;
  std::size_t col = 0;

  auto flush = [&]() {
    if (!buf.empty()) {
      push_span_text(spans, buf, style);
      buf.clear();
    }
  };

  std::size_t i = 0;
  while (i < line.size()) {
    const unsigned char c = static_cast<unsigned char>(line[i]);

    if (c == '\x1b') {
      flush();
      if (i + 1 >= line.size()) break;
      const char next = line[i + 1];
      if (next == '[') {
        // CSI sequence: ESC [ <params> <final 0x40..0x7E>
        std::size_t j = i + 2;
        while (j < line.size()) {
          const unsigned char ch = static_cast<unsigned char>(line[j]);
          if (ch >= 0x40 && ch <= 0x7E) break;
          ++j;
        }
        if (j < line.size()) {
          const char final_byte = line[j];
          if (final_byte == 'm') {
            std::vector<int> params;
            const std::string_view body = line.substr(i + 2, j - (i + 2));
            if (!body.empty()) {
              std::size_t p_start = 0;
              while (p_start <= body.size()) {
                std::size_t sep = body.find_first_of(";:", p_start);
                if (sep == std::string_view::npos) sep = body.size();
                const std::string_view tok = body.substr(p_start, sep - p_start);
                params.push_back(tok.empty() ? 0 : parse_int_or(tok, 0));
                if (sep == body.size()) break;
                p_start = sep + 1;
              }
            }
            apply_sgr_params(params, style);
          }
          i = j + 1;
          continue;
        }
        break;
      } else if (next == ']') {
        // OSC sequence: ESC ] ... (BEL or ESC \)
        std::size_t j = i + 2;
        while (j < line.size()) {
          if (line[j] == '\x07') {
            ++j;
            break;
          }
          if (line[j] == '\x1b' && j + 1 < line.size() && line[j + 1] == '\\') {
            j += 2;
            break;
          }
          ++j;
        }
        i = j;
        continue;
      } else {
        // Other 2-byte ESC sequence.
        i += 2;
        continue;
      }
    }

    if (c == '\t') {
      const std::size_t spaces = 4 - (col % 4);
      buf.append(spaces, ' ');
      col += spaces;
      ++i;
      continue;
    }

    if (c < 0x20 || c == 0x7f) {
      // Strip other control bytes (CR, BEL, etc.).
      ++i;
      continue;
    }

    buf.push_back(static_cast<char>(c));
    // Approximate column count for tab stops (count UTF-8 leading bytes).
    if ((c & 0xC0) != 0x80) ++col;
    ++i;
  }

  flush();
  return spans;
}

std::vector<std::vector<AnsiSpan>> parse_ansi_capture(std::string_view raw) {
  std::vector<std::vector<AnsiSpan>> lines;
  std::size_t pos = 0;
  while (pos <= raw.size()) {
    std::size_t nl = raw.find('\n', pos);
    if (nl == std::string_view::npos) {
      if (pos < raw.size()) {
        lines.push_back(parse_ansi_line(raw.substr(pos)));
      }
      break;
    }
    lines.push_back(parse_ansi_line(raw.substr(pos, nl - pos)));
    pos = nl + 1;
  }

  // Trim trailing empty/whitespace-only lines so the preview doesn't look sparse.
  auto is_blank = [](const std::vector<AnsiSpan>& spans) {
    for (const auto& s : spans) {
      for (char ch : s.text) {
        if (ch != ' ') return false;
      }
    }
    return true;
  };
  while (!lines.empty() && is_blank(lines.back())) {
    lines.pop_back();
  }
  return lines;
}

}  // namespace win_pick
