// tmux_client — everything that talks to tmux or parses what it prints.
//
// Split into two layers so the parsers can be tested without a server:
//   * pure functions over strings (parse_*, tokenize, commands_in, build_catalog,
//     resolve_*), and
//   * load_catalog() / run_tmux(), the only code that forks tmux.
// Edge cases and the measurements behind them: docs/parser-design.org.
#pragma once

#include <map>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace ccmm {

// The pane/window/session/client the popup was opened from. Passed in as argv by
// scripts/launch.sh, because display-popup does not expand formats.
struct Context {
  std::string client;
  std::string session;
  std::string window;
  std::string pane;
};

struct RunResult {
  int status = -1;  // exit code; -1 when tmux could not be started or was killed
  std::string out;
  std::string err;
};

// fork/exec `tmux args...` (PATH lookup, no shell). The tmux server is the one
// $TMUX names, exactly as for any tmux call made from inside tmux.
RunResult run_tmux(std::span<const std::string> args);

// ── list-commands ───────────────────────────────────────────────────────────
struct CommandInfo {
  std::string name;
  std::string alias;  // "" when the command has none
  std::string usage;  // "" for e.g. kill-server
};

// The -F format load_catalog() asks for; fields separated by a tab.
inline constexpr std::string_view kListCommandsFormat =
    "#{command_list_name}\t#{command_list_alias}\t#{command_list_usage}";

std::vector<CommandInfo> parse_list_commands_formatted(std::string_view text);
std::vector<CommandInfo> parse_list_commands_plain(std::string_view text);

// ── list-keys ───────────────────────────────────────────────────────────────
struct KeyBinding {
  std::string table;
  std::string key;      // unescaped: `"` not `\"`
  bool repeat = false;  // bound with -r
  std::string command;  // the command text exactly as list-keys printed it
};

std::optional<KeyBinding> parse_list_keys_line(std::string_view line);
std::vector<KeyBinding> parse_list_keys(std::string_view text);

// `list-keys -N -T TABLE` lines: "KEY<spaces>NOTE", key raw (not escaped).
struct KeyNote {
  std::string key;
  std::string note;
};
std::vector<KeyNote> parse_list_keys_notes(std::string_view text);

// Undo list-keys' escaping of a key token: `\"` → `"`, `\\` → `\`.
std::string unescape_key(std::string_view token);

// ── tmux command-line syntax ────────────────────────────────────────────────
// Split tmux command text into words: '…' literal, "…" with backslash escapes,
// a bare backslash escapes the next char, `#{…}` kept whole. A command
// separator (`;`, `\;` or a word ending in an unquoted `;`) becomes the token ";",
// and a bare `{` / `}` become their own tokens.
std::vector<std::string> tokenize(std::string_view text);

// Names of the tmux commands that `command_text` runs: the first word of each
// `;`-separated link, words inside `{ … }` blocks, and the quoted command
// arguments of wrappers (if-shell, confirm-before, command-prompt,
// display-menu). Aliases are resolved through `alias_to_name`; a word that is
// neither a name nor an alias is dropped.
std::vector<std::string> commands_in(
    std::string_view command_text,
    const std::map<std::string, std::string, std::less<>>& alias_to_name);

// ── the palette's model ─────────────────────────────────────────────────────
enum class Kind { Binding, Command };

struct Action {
  Kind kind = Kind::Command;
  // Command
  std::string name;
  std::string alias;
  std::string usage;
  // Binding (a key bound with -N "note")
  std::string table;
  std::string key;
  std::string note;
  std::string bound_command;
  // Both: "prefix \"", "M-Up", "copy-mode:q". For a Binding, its own key.
  std::vector<std::string> badges;
};

struct Catalog {
  std::vector<Action> actions;  // Bindings first (in list-keys order), then Commands A–Z
  std::string prefix;           // value of the `prefix` option; "None" when unset
  std::vector<std::string> warnings;  // dropped rows etc. — shown, never fatal
  std::map<std::string, std::string, std::less<>> alias_to_name;  // also name→name
  std::map<std::string, std::size_t, std::less<>> command_index;  // name → actions index
};

// Tables whose keys become badges. copy-mode / copy-mode-vi are left out: 165
// `send-keys -X` rows would bury every other badge.
bool badge_table(std::string_view table);

// "prefix X" for the prefix table, "X" for root, "TABLE:X" otherwise.
std::string badge_text(std::string_view table, std::string_view key);

Catalog build_catalog(std::vector<CommandInfo> commands,
                      const std::vector<KeyBinding>& bindings,
                      const std::map<std::string, std::vector<KeyNote>>& notes_by_table,
                      std::string prefix);

// Runs tmux (list-commands, list-keys, list-keys -N per table, show prefix).
// On failure the catalog is empty and `error` says why.
Catalog load_catalog(std::string& error);

// ── turning a choice into tmux argv ─────────────────────────────────────────
// Flags to add so the command acts on the popup's origin rather than on
// whatever tmux would guess. Derived from the usage string; skipped for a flag
// the user already typed. Empty when the usage names no target.
std::vector<std::string> derived_target_flags(const CommandInfo& cmd,
                                              std::span<const std::string> user_args,
                                              const Context& ctx);

// argv (without the leading "tmux") for running `line`, a full command line as
// typed in the argument bar. The first word must be a command or alias.
std::optional<std::vector<std::string>> resolve_line(std::string_view line,
                                                     const Catalog& catalog,
                                                     const Context& ctx,
                                                     std::string& error);

// argv for Enter on an action: a Command runs bare (plus derived flags); a
// Binding is replayed as a keystroke on the origin client (send-keys -K), so its
// formats expand exactly as when the key is pressed.
std::optional<std::vector<std::string>> resolve_action(const Action& action,
                                                       const Catalog& catalog,
                                                       const Context& ctx,
                                                       std::string& error);

// What the argument bar is prefilled with on Tab.
std::string edit_line_for(const Action& action);

// Shell-quoted, space-joined — for `rows` output and messages only, never executed.
std::string quote_argv(std::span<const std::string> argv);

// Replace C0 controls and DEL with U+FFFD so key notes and bound commands (user
// text) can't emit terminal escapes when drawn.
std::string printable(std::string_view s);

}  // namespace ccmm
