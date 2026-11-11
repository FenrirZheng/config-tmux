// cc-main-menu — tmux command palette.
//
//   cc-main-menu [CONTEXT] --out FILE   the popup (run by scripts/launch.sh).
//                                       On Enter, writes the chosen tmux argv to
//                                       FILE, NUL-delimited, and exits 0; the
//                                       popup closes and launch.sh runs it.
//                                       Cancel (Esc / C-g / C-c) leaves FILE
//                                       empty. Without --out the argv is printed
//                                       shell-quoted on stdout instead.
//   cc-main-menu rows [QUERY]           headless seam, no TUI: one TSV line per
//                                       action the popup would list for QUERY,
//                                       in the popup's order. Fields:
//                                       kind  label  alias|key  badges  argv
//   cc-main-menu resolve LINE           headless seam: the argv the argument bar
//                                       would run for LINE, one word per line.
//
// CONTEXT: --client NAME --session ID --window ID --pane ID. Any of them left
// out is asked of tmux (display-message -p), which is what the headless seams
// rely on.
#include <cstdio>
#include <fstream>
#include <iostream>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "fuzzy_filter.hpp"
#include "tmux_client.hpp"
#include "ui.hpp"

namespace {

constexpr std::string_view kVersion = "cc-main-menu 0.2.0";

constexpr std::string_view kUsage =
    "usage: cc-main-menu [--client C --session S --window W --pane P] [--out FILE]\n"
    "       cc-main-menu rows [QUERY]\n"
    "       cc-main-menu resolve LINE\n";

struct Cli {
  ccmm::Context ctx;
  std::string out;
  std::string sub;  // "", "rows", "resolve"
  std::vector<std::string> positional;
};

bool parse_cli(std::span<char*> args, Cli& cli, std::string& error) {
  for (std::size_t i = 0; i < args.size(); ++i) {
    const std::string_view a = args[i];
    auto value = [&](std::string& dst) {
      if (i + 1 >= args.size()) {
        error = std::string(a) + " needs a value";
        return false;
      }
      dst = args[++i];
      return true;
    };
    if (a == "--client") {
      if (!value(cli.ctx.client)) return false;
    } else if (a == "--session") {
      if (!value(cli.ctx.session)) return false;
    } else if (a == "--window") {
      if (!value(cli.ctx.window)) return false;
    } else if (a == "--pane") {
      if (!value(cli.ctx.pane)) return false;
    } else if (a == "--out") {
      if (!value(cli.out)) return false;
    } else if (cli.sub.empty() && cli.positional.empty() && (a == "rows" || a == "resolve")) {
      cli.sub = a;
    } else if (!a.empty() && a.front() == '-' && cli.sub.empty()) {
      error = "unknown option: " + std::string(a);
      return false;
    } else {
      cli.positional.emplace_back(a);
    }
  }
  return true;
}

// Fill whatever the caller did not pass from tmux's idea of "current".
void complete_context(ccmm::Context& ctx) {
  if (!ctx.client.empty() && !ctx.session.empty() && !ctx.window.empty() && !ctx.pane.empty())
    return;
  const std::vector<std::string> args{
      "display-message", "-p", "#{client_name}\t#{session_id}\t#{window_id}\t#{pane_id}"};
  const auto r = ccmm::run_tmux(args);
  if (r.status != 0) return;
  std::vector<std::string> f(1);
  for (const char c : r.out) {
    if (c == '\t')
      f.emplace_back();
    else if (c != '\n')
      f.back().push_back(c);
  }
  f.resize(4);
  if (ctx.client.empty()) ctx.client = f[0];
  if (ctx.session.empty()) ctx.session = f[1];
  if (ctx.window.empty()) ctx.window = f[2];
  if (ctx.pane.empty()) ctx.pane = f[3];
}

std::string join(const std::vector<std::string>& v, std::string_view sep) {
  std::string out;
  for (const auto& s : v) {
    if (!out.empty()) out += sep;
    out += s;
  }
  return out;
}

int cmd_rows(const ccmm::Catalog& cat, const ccmm::Context& ctx, std::string_view query) {
  for (const auto i : ccmm::filter(query, cat.actions)) {
    const auto& a = cat.actions[i];
    std::string err;
    const auto argv = ccmm::resolve_action(a, cat, ctx, err);
    const bool cmd = a.kind == ccmm::Kind::Command;
    // Enter on these opens the argument bar instead of running (see ui.cpp).
    const std::string what =
        ccmm::needs_destination(a)       ? "edit:" + ccmm::destination_line_for(a)
        : !ccmm::required_args(a).empty() ? "edit:" + ccmm::edit_line_for(a)
        : argv                           ? ccmm::quote_argv(*argv)
                                         : "!" + err;
    std::cout << (cmd ? "cmd" : "key") << '\t' << (cmd ? a.name : a.note) << '\t'
              << (cmd ? a.alias : ccmm::badge_text(a.table, a.key)) << '\t'
              << join(a.badges, ",") << '\t' << what << '\n';
  }
  for (const auto& w : cat.warnings) std::cerr << "cc-main-menu: warning: " << w << '\n';
  return 0;
}

int cmd_resolve(const ccmm::Catalog& cat, const ccmm::Context& ctx, std::string_view line) {
  std::string err;
  const auto argv = ccmm::resolve_line(line, cat, ctx, err);
  if (!argv) {
    std::cerr << "cc-main-menu: " << err << '\n';
    return 1;
  }
  for (const auto& w : *argv) std::cout << w << '\n';
  return 0;
}

bool write_argv(const std::string& path, const std::vector<std::string>& argv) {
  std::ofstream f(path, std::ios::binary | std::ios::trunc);
  for (const auto& w : argv) {
    f << w;
    f.put('\0');
  }
  return static_cast<bool>(f);
}

}  // namespace

int main(int argc, char** argv) {
  const std::span<char*> args(argv + 1, static_cast<std::size_t>(argc > 0 ? argc - 1 : 0));
  if (!args.empty()) {
    const std::string_view first = args.front();
    if (first == "--version") return std::cout << kVersion << '\n', 0;
    if (first == "--help" || first == "-h") return std::cout << kUsage, 0;
  }

  Cli cli;
  std::string error;
  if (!parse_cli(args, cli, error)) {
    std::cerr << "cc-main-menu: " << error << '\n' << kUsage;
    return 2;
  }
  complete_context(cli.ctx);

  std::string load_error;
  const auto catalog = ccmm::load_catalog(load_error);

  if (cli.sub == "rows" || cli.sub == "resolve") {
    if (!load_error.empty()) {
      std::cerr << "cc-main-menu: " << load_error << '\n';
      return 1;
    }
    if (cli.sub == "rows") return cmd_rows(catalog, cli.ctx, join(cli.positional, " "));
    if (cli.positional.size() != 1) {
      std::cerr << "cc-main-menu: resolve takes exactly one LINE argument\n";
      return 2;
    }
    return cmd_resolve(catalog, cli.ctx, cli.positional.front());
  }
  if (!cli.positional.empty()) {
    std::cerr << "cc-main-menu: unexpected argument: " << cli.positional.front() << '\n'
              << kUsage;
    return 2;
  }

  const auto result = ccmm::run_ui(catalog, cli.ctx, load_error);
  if (!cli.out.empty()) {
    if (!write_argv(cli.out, result.argv)) {
      std::cerr << "cc-main-menu: cannot write " << cli.out << '\n';
      return 1;
    }
  } else if (!result.argv.empty()) {
    std::cout << "tmux " << ccmm::quote_argv(result.argv) << '\n';
  }
  return 0;
}
