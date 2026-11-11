#include "tmux_client.hpp"

#include <fcntl.h>
#include <poll.h>
#include <spawn.h>
#include <sys/wait.h>
#include <unistd.h>

#include <algorithm>
#include <array>
#include <cctype>
#include <cerrno>
#include <cstring>
#include <set>
#include <utility>

extern char** environ;

namespace ccmm {

namespace {

// ── small string helpers ────────────────────────────────────────────────────
bool is_space(char c) { return c == ' ' || c == '\t'; }

std::string_view trim(std::string_view s) {
  while (!s.empty() && (is_space(s.front()) || s.front() == '\r')) s.remove_prefix(1);
  while (!s.empty() && (is_space(s.back()) || s.back() == '\r' || s.back() == '\n'))
    s.remove_suffix(1);
  return s;
}

// Split on '\n', dropping one trailing empty line.
std::vector<std::string_view> lines_of(std::string_view text) {
  std::vector<std::string_view> out;
  while (!text.empty()) {
    const auto nl = text.find('\n');
    out.push_back(text.substr(0, nl));
    if (nl == std::string_view::npos) break;
    text.remove_prefix(nl + 1);
  }
  return out;
}

// Pops the next whitespace-delimited word off `s`.
std::string_view next_word(std::string_view& s) {
  while (!s.empty() && is_space(s.front())) s.remove_prefix(1);
  std::size_t n = 0;
  while (n < s.size() && !is_space(s[n])) ++n;
  const auto w = s.substr(0, n);
  s.remove_prefix(n);
  return w;
}

bool valid_command_name(std::string_view name) {
  if (name.empty() || name.front() < 'a' || name.front() > 'z') return false;
  return std::all_of(name.begin(), name.end(),
                     [](char c) { return (c >= 'a' && c <= 'z') || c == '-'; });
}

// ── RAII for the pipe ends ──────────────────────────────────────────────────
class Fd {
 public:
  Fd() = default;
  explicit Fd(int fd) : fd_(fd) {}
  Fd(const Fd&) = delete;
  Fd& operator=(const Fd&) = delete;
  Fd(Fd&& o) noexcept : fd_(std::exchange(o.fd_, -1)) {}
  Fd& operator=(Fd&& o) noexcept {
    if (this != &o) {
      reset();
      fd_ = std::exchange(o.fd_, -1);
    }
    return *this;
  }
  ~Fd() { reset(); }
  int get() const { return fd_; }
  void reset() {
    if (fd_ >= 0) ::close(fd_);
    fd_ = -1;
  }

 private:
  int fd_ = -1;
};

struct Pipe {
  Fd read;
  Fd write;
  bool open() {
    std::array<int, 2> fds{};
    if (::pipe2(fds.data(), O_CLOEXEC) != 0) return false;
    read = Fd(fds[0]);
    write = Fd(fds[1]);
    return true;
  }
};

class FileActions {
 public:
  FileActions() { ok_ = ::posix_spawn_file_actions_init(&fa_) == 0; }
  FileActions(const FileActions&) = delete;
  FileActions& operator=(const FileActions&) = delete;
  ~FileActions() {
    if (ok_) ::posix_spawn_file_actions_destroy(&fa_);
  }
  bool ok() const { return ok_; }
  posix_spawn_file_actions_t* get() { return &fa_; }

 private:
  posix_spawn_file_actions_t fa_{};
  bool ok_ = false;
};

// Commands whose quoted arguments are themselves the command the key runs.
const std::set<std::string, std::less<>> kWrappers = {
    "if-shell", "confirm-before", "command-prompt",
};

// A menu's items are choices, not what the key does: `prefix >` would otherwise
// badge split-window, kill-pane, swap-pane… Its { } blocks are skipped whole.
constexpr std::string_view kMenu = "display-menu";

}  // namespace

// ── process ─────────────────────────────────────────────────────────────────
RunResult run_tmux(std::span<const std::string> args) {
  RunResult r;
  Pipe out;
  Pipe err;
  if (!out.open() || !err.open()) {
    r.err = std::string("pipe: ") + std::strerror(errno);
    return r;
  }
  FileActions fa;
  if (!fa.ok()) {
    r.err = "posix_spawn_file_actions_init failed";
    return r;
  }
  ::posix_spawn_file_actions_adddup2(fa.get(), out.write.get(), STDOUT_FILENO);
  ::posix_spawn_file_actions_adddup2(fa.get(), err.write.get(), STDERR_FILENO);
  ::posix_spawn_file_actions_addopen(fa.get(), STDIN_FILENO, "/dev/null", O_RDONLY, 0);

  std::vector<std::string> owned;
  owned.reserve(args.size() + 1);
  owned.emplace_back("tmux");
  owned.insert(owned.end(), args.begin(), args.end());
  std::vector<char*> argv;
  argv.reserve(owned.size() + 1);
  for (auto& a : owned) argv.push_back(a.data());
  argv.push_back(nullptr);

  pid_t pid = 0;
  const int rc = ::posix_spawnp(&pid, "tmux", fa.get(), nullptr, argv.data(), environ);
  out.write.reset();
  err.write.reset();
  if (rc != 0) {
    r.err = std::string("cannot run tmux: ") + std::strerror(rc);
    return r;
  }

  std::array<pollfd, 2> pfd{{{out.read.get(), POLLIN, 0}, {err.read.get(), POLLIN, 0}}};
  std::array<std::string*, 2> sink{&r.out, &r.err};
  std::array<char, 8192> buf{};
  int open_count = 2;
  while (open_count > 0) {
    if (::poll(pfd.data(), pfd.size(), -1) < 0) {
      if (errno == EINTR) continue;
      break;
    }
    for (std::size_t i = 0; i < pfd.size(); ++i) {
      if (pfd[i].fd < 0 || pfd[i].revents == 0) continue;
      const ssize_t n = ::read(pfd[i].fd, buf.data(), buf.size());
      if (n > 0) {
        sink[i]->append(buf.data(), static_cast<std::size_t>(n));
      } else if (n == 0 || errno != EINTR) {
        pfd[i].fd = -1;
        --open_count;
      }
    }
  }

  int wstatus = 0;
  while (::waitpid(pid, &wstatus, 0) < 0 && errno == EINTR) {
  }
  r.status = WIFEXITED(wstatus) ? WEXITSTATUS(wstatus) : -1;
  return r;
}

// ── list-commands ───────────────────────────────────────────────────────────
std::vector<CommandInfo> parse_list_commands_formatted(std::string_view text) {
  std::vector<CommandInfo> out;
  for (auto line : lines_of(text)) {
    const auto t1 = line.find('\t');
    if (t1 == std::string_view::npos) continue;
    const auto t2 = line.find('\t', t1 + 1);
    if (t2 == std::string_view::npos) continue;
    CommandInfo c;
    c.name = std::string(trim(line.substr(0, t1)));
    c.alias = std::string(trim(line.substr(t1 + 1, t2 - t1 - 1)));
    c.usage = std::string(trim(line.substr(t2 + 1)));  // usage is last: may hold anything
    if (!valid_command_name(c.name)) continue;
    out.push_back(std::move(c));
  }
  return out;
}

std::vector<CommandInfo> parse_list_commands_plain(std::string_view text) {
  std::vector<CommandInfo> out;
  for (auto line : lines_of(text)) {
    std::string_view rest = line;
    const auto name = next_word(rest);
    if (!valid_command_name(name)) continue;
    CommandInfo c;
    c.name = std::string(name);
    std::string_view probe = rest;
    const auto second = next_word(probe);
    if (second.size() > 2 && second.front() == '(' && second.back() == ')') {
      c.alias = std::string(second.substr(1, second.size() - 2));
      rest = probe;
    }
    c.usage = std::string(trim(rest));
    out.push_back(std::move(c));
  }
  return out;
}

// ── list-keys ───────────────────────────────────────────────────────────────
std::string unescape_key(std::string_view token) {
  if (token.size() == 2 && token.front() == '\\') return std::string(1, token[1]);
  return std::string(token);
}

std::optional<KeyBinding> parse_list_keys_line(std::string_view line) {
  std::string_view rest = line;
  if (next_word(rest) != "bind-key") return std::nullopt;
  KeyBinding kb;
  for (;;) {
    const auto w = next_word(rest);
    if (w.empty()) return std::nullopt;
    if (w == "-r") {
      kb.repeat = true;
    } else if (w == "-T") {
      kb.table = std::string(next_word(rest));
    } else if (w == "-N") {
      next_word(rest);  // not printed by 3.5a, tolerated if a later tmux does
    } else if (w.front() == '-' && w.size() > 1 && kb.table.empty()) {
      continue;  // unknown flag before the key
    } else {
      kb.key = unescape_key(w);
      break;
    }
  }
  if (kb.table.empty() || kb.key.empty()) return std::nullopt;
  kb.command = std::string(trim(rest));
  return kb;
}

std::vector<KeyBinding> parse_list_keys(std::string_view text) {
  std::vector<KeyBinding> out;
  for (auto line : lines_of(text))
    if (auto kb = parse_list_keys_line(line)) out.push_back(std::move(*kb));
  return out;
}

std::vector<KeyNote> parse_list_keys_notes(std::string_view text) {
  std::vector<KeyNote> out;
  for (auto line : lines_of(text)) {
    std::string_view rest = line;
    const auto key = next_word(rest);
    const auto note = trim(rest);
    if (key.empty() || note.empty()) continue;
    out.push_back({std::string(key), std::string(note)});
  }
  return out;
}

// ── tokenizer ───────────────────────────────────────────────────────────────
std::vector<std::string> tokenize(std::string_view s) {
  std::vector<std::string> out;
  std::string cur;
  bool have = false;  // distinguishes an empty quoted word from no word
  bool quoted_any = false;
  auto flush = [&] {
    if (have) {
      if (!quoted_any && (cur == "{" || cur == "}")) {
        out.push_back(cur);
      } else if (!quoted_any && cur.size() > 1 && cur.back() == ';' &&
                 cur[cur.size() - 2] != '\\') {
        cur.pop_back();
        out.push_back(cur);
        out.emplace_back(";");
      } else if (cur == ";" || (!quoted_any && cur == "\\;")) {
        out.emplace_back(";");
      } else {
        out.push_back(cur);
      }
    }
    cur.clear();
    have = false;
    quoted_any = false;
  };

  std::size_t i = 0;
  while (i < s.size()) {
    const char c = s[i];
    if (is_space(c) || c == '\n') {
      flush();
      ++i;
    } else if (c == '\'') {
      have = quoted_any = true;
      const auto end = s.find('\'', i + 1);
      cur.append(s.substr(i + 1, end == std::string_view::npos ? std::string_view::npos
                                                                : end - i - 1));
      i = end == std::string_view::npos ? s.size() : end + 1;
    } else if (c == '"') {
      have = quoted_any = true;
      ++i;
      while (i < s.size() && s[i] != '"') {
        if (s[i] == '\\' && i + 1 < s.size()) ++i;
        cur.push_back(s[i]);
        ++i;
      }
      ++i;  // closing quote
    } else if (c == '\\' && i + 1 < s.size()) {
      have = true;
      if (s[i + 1] == ';' && (i + 2 == s.size() || is_space(s[i + 2])) && cur.empty()) {
        cur = "\\;";  // the separator; flush() turns it into ";"
      } else {
        cur.push_back(s[i + 1]);
      }
      i += 2;
    } else if (c == '#' && i + 1 < s.size() && s[i + 1] == '{') {
      // A format: keep it whole, spaces and nested braces included.
      have = true;
      int depth = 0;
      while (i < s.size()) {
        if (s[i] == '{') ++depth;
        if (s[i] == '}' && --depth == 0) {
          cur.push_back(s[i++]);
          break;
        }
        cur.push_back(s[i++]);
      }
    } else {
      have = true;
      cur.push_back(c);
      ++i;
    }
  }
  flush();
  return out;
}

namespace {

void collect_commands(std::span<const std::string> toks,
                      const std::map<std::string, std::string, std::less<>>& aliases,
                      std::vector<std::string>& found, int depth) {
  if (depth > 4) return;
  bool at_start = true;
  std::string current;
  for (std::size_t i = 0; i < toks.size(); ++i) {
    const auto& t = toks[i];
    if (t == "{" && current == kMenu) {
      for (int level = 0; i < toks.size(); ++i) {
        if (toks[i] == "{") ++level;
        if (toks[i] == "}" && --level == 0) break;
      }
      continue;
    }
    if (t == ";" || t == "{" || t == "}") {
      at_start = true;
      continue;
    }
    if (at_start) {
      at_start = false;
      const auto it = aliases.find(t);
      current = it == aliases.end() ? std::string() : it->second;
      if (!current.empty() &&
          std::find(found.begin(), found.end(), current) == found.end())
        found.push_back(current);
      continue;
    }
    // A wrapper's non-flag argument that itself starts with a command word.
    if (kWrappers.count(current) != 0 && !t.empty() && t.front() != '-') {
      const auto inner = tokenize(t);
      if (!inner.empty() && aliases.count(inner.front()) != 0)
        collect_commands(inner, aliases, found, depth + 1);
    }
  }
}

}  // namespace

std::vector<std::string> commands_in(
    std::string_view command_text,
    const std::map<std::string, std::string, std::less<>>& alias_to_name) {
  std::vector<std::string> found;
  const auto toks = tokenize(command_text);
  collect_commands(toks, alias_to_name, found, 0);
  return found;
}

// ── catalog ─────────────────────────────────────────────────────────────────
bool badge_table(std::string_view table) {
  return table != "copy-mode" && table != "copy-mode-vi";
}

std::string badge_text(std::string_view table, std::string_view key) {
  if (table == "prefix") return "prefix " + std::string(key);
  if (table == "root") return std::string(key);
  return std::string(table) + ":" + std::string(key);
}

std::string printable(std::string_view s) {
  std::string out;
  out.reserve(s.size());
  for (const char c : s) {
    const auto u = static_cast<unsigned char>(c);
    if (u < 0x20 || u == 0x7f)
      out += "\xEF\xBF\xBD";  // U+FFFD
    else
      out.push_back(c);
  }
  return out;
}

Catalog build_catalog(std::vector<CommandInfo> commands,
                      const std::vector<KeyBinding>& bindings,
                      const std::map<std::string, std::vector<KeyNote>>& notes_by_table,
                      std::string prefix) {
  Catalog cat;
  cat.prefix = std::move(prefix);

  std::sort(commands.begin(), commands.end(),
            [](const CommandInfo& a, const CommandInfo& b) { return a.name < b.name; });
  for (const auto& c : commands) {
    if (cat.alias_to_name.count(c.name) != 0) {
      cat.warnings.push_back("duplicate command dropped: " + c.name);
      continue;
    }
    cat.alias_to_name[c.name] = c.name;
    if (!c.alias.empty()) cat.alias_to_name.emplace(c.alias, c.name);
  }

  // Noted keys first: they are the "menu" part of the palette.
  std::map<std::pair<std::string, std::string>, const KeyBinding*> by_key;
  for (const auto& b : bindings) by_key.emplace(std::pair{b.table, b.key}, &b);
  for (const auto& [table, notes] : notes_by_table) {
    for (const auto& n : notes) {
      Action a;
      a.kind = Kind::Binding;
      a.table = table;
      a.key = n.key;
      a.note = printable(n.note);
      const auto it = by_key.find({table, n.key});
      if (it != by_key.end()) a.bound_command = printable(it->second->command);
      a.badges.push_back(printable(badge_text(table, n.key)));
      cat.actions.push_back(std::move(a));
    }
  }

  std::map<std::string, std::vector<std::string>, std::less<>> badges;
  for (const auto& b : bindings) {
    if (!badge_table(b.table)) continue;
    // Mouse events are not shortcuts anyone types.
    if (b.key.find("Mouse") != std::string::npos || b.key.find("Wheel") != std::string::npos ||
        b.key.find("Click") != std::string::npos || b.key.find("Drag") != std::string::npos)
      continue;
    for (const auto& name : commands_in(b.command, cat.alias_to_name)) {
      auto& v = badges[name];
      auto text = printable(badge_text(b.table, b.key));
      if (std::find(v.begin(), v.end(), text) == v.end()) v.push_back(std::move(text));
    }
  }

  for (auto& c : commands) {
    if (cat.command_index.count(c.name) != 0) continue;
    Action a;
    a.kind = Kind::Command;
    a.name = std::move(c.name);
    a.alias = std::move(c.alias);
    a.usage = std::move(c.usage);
    if (const auto it = badges.find(a.name); it != badges.end()) a.badges = it->second;
    cat.command_index[a.name] = cat.actions.size();
    cat.actions.push_back(std::move(a));
  }
  return cat;
}

Catalog load_catalog(std::string& error) {
  const std::array<std::string, 3> lc_args{"list-commands", "-F",
                                           std::string(kListCommandsFormat)};
  auto lc = run_tmux(lc_args);
  std::vector<CommandInfo> commands;
  std::vector<std::string> warnings;
  if (lc.status == 0) commands = parse_list_commands_formatted(lc.out);
  if (commands.empty()) {
    const std::array<std::string, 1> plain_args{"list-commands"};
    lc = run_tmux(plain_args);
    if (lc.status != 0) {
      error = "tmux list-commands failed: " + std::string(trim(lc.err.empty() ? lc.out : lc.err));
      return {};
    }
    commands = parse_list_commands_plain(lc.out);
    warnings.emplace_back("list-commands -F unsupported; parsed plain output");
  }
  if (commands.empty()) {
    error = "tmux list-commands returned no commands";
    return {};
  }

  const std::array<std::string, 1> lk_args{"list-keys"};
  const auto lk = run_tmux(lk_args);
  std::vector<KeyBinding> bindings;
  if (lk.status == 0)
    bindings = parse_list_keys(lk.out);
  else
    warnings.push_back("list-keys failed: " + std::string(trim(lk.err)));

  std::set<std::string> tables;
  for (const auto& b : bindings)
    if (badge_table(b.table)) tables.insert(b.table);
  std::map<std::string, std::vector<KeyNote>> notes;
  // std::map orders tables by name, so prefix comes before root.
  for (const auto& t : tables) {
    const std::array<std::string, 4> args{"list-keys", "-N", "-T", t};
    const auto r = run_tmux(args);
    if (r.status == 0) {
      auto parsed = parse_list_keys_notes(r.out);
      if (!parsed.empty()) notes[t] = std::move(parsed);
    }
  }

  const std::array<std::string, 3> px{"show-options", "-gv", "prefix"};
  const auto pr = run_tmux(px);
  std::string prefix = pr.status == 0 ? std::string(trim(pr.out)) : std::string("None");

  auto cat = build_catalog(std::move(commands), bindings, notes, std::move(prefix));
  cat.warnings.insert(cat.warnings.begin(), warnings.begin(), warnings.end());
  return cat;
}

// ── resolution ──────────────────────────────────────────────────────────────
namespace {

// True when `flag` (e.g. 't') is already present among the leading option
// words of `args`, either alone ("-t") or as the last letter of a cluster
// ("-ht") or with its value attached ("-t%3").
bool has_flag(std::span<const std::string> args, char flag) {
  for (const auto& a : args) {
    if (a == ";") break;
    if (a.size() < 2 || a.front() != '-' || a == "--") continue;
    if (a.find(flag) != std::string::npos) return true;
  }
  return false;
}

bool usage_has(std::string_view usage, std::string_view needle) {
  return usage.find(needle) != std::string_view::npos;
}

// Quote one word for tmux's command parser (inverse of tokenize for a word).
std::string tmux_quote(const std::string& w) {
  if (w == ";" || w == "{" || w == "}") return w;
  const bool plain = !w.empty() && w.find_first_of(" \t'\"\\;{}#$~") == std::string::npos;
  if (plain) return w;
  if (w.find('\'') == std::string::npos) return "'" + w + "'";
  std::string out = "\"";
  for (const char c : w) {
    if (c == '"' || c == '\\' || c == '$') out.push_back('\\');
    out.push_back(c);
  }
  out.push_back('"');
  return out;
}

// `{ … }` is config-file syntax: on the command line tmux reads each brace as a
// literal argument ("too many arguments", measured 3.5a). Fold every top-level
// group back into ONE argument holding the inner commands as a string, which is
// what command-prompt / if-shell / confirm-before accept there.
std::vector<std::string> fold_brace_groups(std::span<const std::string> toks) {
  std::vector<std::string> out;
  for (std::size_t i = 0; i < toks.size(); ++i) {
    if (toks[i] != "{") {
      out.push_back(toks[i]);
      continue;
    }
    std::string inner;
    int level = 1;
    for (++i; i < toks.size(); ++i) {
      if (toks[i] == "{") ++level;
      if (toks[i] == "}" && --level == 0) break;
      if (!inner.empty()) inner.push_back(' ');
      inner += tmux_quote(toks[i]);  // a bare ";" separates commands in the string
    }
    out.push_back(std::move(inner));
  }
  return out;
}

}  // namespace

std::vector<std::string> derived_target_flags(const CommandInfo& cmd,
                                              std::span<const std::string> user_args,
                                              const Context& ctx) {
  std::vector<std::string> out;
  const auto& u = cmd.usage;
  if (!has_flag(user_args, 't')) {
    std::string value;
    if (usage_has(u, "-t target-pane"))
      value = ctx.pane;
    else if (usage_has(u, "-t target-window"))
      // new-window -t @id means "create AT that index" and fails with
      // "index N in use"; `$session:` picks the next free index like prefix c.
      value = cmd.name == "new-window" ? (ctx.session.empty() ? "" : ctx.session + ":")
                                       : ctx.window;
    else if (usage_has(u, "-t target-session"))
      value = ctx.session;
    else if (usage_has(u, "-t target-client"))
      value = ctx.client;
    if (!value.empty()) {
      out.emplace_back("-t");
      out.push_back(std::move(value));
    }
  }
  if (!has_flag(user_args, 'c') && usage_has(u, "-c target-client") && !ctx.client.empty()) {
    out.emplace_back("-c");
    out.push_back(ctx.client);
  }
  return out;
}

std::optional<std::vector<std::string>> resolve_line(std::string_view line,
                                                     const Catalog& catalog,
                                                     const Context& ctx,
                                                     std::string& error) {
  auto toks = fold_brace_groups(tokenize(line));
  if (toks.empty()) {
    error = "empty command line";
    return std::nullopt;
  }
  const auto it = catalog.alias_to_name.find(toks.front());
  if (it == catalog.alias_to_name.end()) {
    error = "unknown command: " + printable(toks.front());
    return std::nullopt;
  }
  const auto& action = catalog.actions.at(catalog.command_index.at(it->second));
  const CommandInfo info{action.name, action.alias, action.usage};
  const std::span<const std::string> user_args(toks.begin() + 1, toks.end());

  std::vector<std::string> argv;
  argv.push_back(action.name);
  // Derived flags go right after the name: tmux stops option parsing at the
  // first positional argument, so a trailing -t would be read as an argument.
  for (auto& f : derived_target_flags(info, user_args, ctx)) argv.push_back(std::move(f));
  argv.insert(argv.end(), user_args.begin(), user_args.end());
  return argv;
}

std::optional<std::vector<std::string>> resolve_action(const Action& action,
                                                       const Catalog& catalog,
                                                       const Context& ctx,
                                                       std::string& error) {
  if (action.kind == Kind::Command) return resolve_line(action.name, catalog, ctx, error);

  if (ctx.client.empty()) {
    error = "no client: a key can only be replayed on the client that opened the menu";
    return std::nullopt;
  }
  // A lone ";" argument separates commands on the tmux command line.
  const std::string key = action.key == ";" ? "\\;" : action.key;
  std::vector<std::string> argv;
  if (action.table == "prefix") {
    if (catalog.prefix.empty() || catalog.prefix == "None") {
      error = "the prefix option is None; cannot replay a prefix key";
      return std::nullopt;
    }
    argv = {"send-keys", "-K", "-c", ctx.client, catalog.prefix, key};
  } else if (action.table == "root") {
    argv = {"send-keys", "-K", "-c", ctx.client, key};
  } else {
    argv = {"switch-client", "-c", ctx.client, "-T", action.table, ";",
            "send-keys",     "-K", "-c",       ctx.client, key};
  }
  return argv;
}

std::string edit_line_for(const Action& action) {
  if (action.kind == Kind::Command) return action.name + " ";
  return action.bound_command;
}

std::string quote_argv(std::span<const std::string> argv) {
  std::string out;
  for (const auto& a : argv) {
    if (!out.empty()) out.push_back(' ');
    const bool plain =
        !a.empty() && std::all_of(a.begin(), a.end(), [](char c) {
          return std::isalnum(static_cast<unsigned char>(c)) != 0 ||
                 std::strchr("-_./:%@=,+", c) != nullptr;
        });
    if (plain) {
      out += a;
      continue;
    }
    out.push_back('\'');
    for (const char c : a) {
      if (c == '\'')
        out += "'\\''";
      else
        out.push_back(c);
    }
    out.push_back('\'');
  }
  return out;
}

}  // namespace ccmm
