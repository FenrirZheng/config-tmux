#include "bar_spec.hpp"

#include <cerrno>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>
#include <string_view>
#include <sys/wait.h>
#include <unistd.h>
#include <utility>
#include <vector>

namespace {

struct RunResult {
  int exit_code = 1;
  std::string stdout_data;
};

int wait_child(pid_t pid) {
  int status = 0;
  while (waitpid(pid, &status, 0) < 0) {
    if (errno == EINTR) {
      continue;
    }
    return 1;
  }
  if (WIFEXITED(status)) {
    return WEXITSTATUS(status);
  }
  if (WIFSIGNALED(status)) {
    return 128 + WTERMSIG(status);
  }
  return 1;
}

std::vector<char*> make_exec_argv(std::vector<std::string>& args) {
  std::vector<char*> argv;
  argv.reserve(args.size() + 1);
  for (auto& arg : args) {
    argv.push_back(arg.data());
  }
  argv.push_back(nullptr);
  return argv;
}

int run_tmux(std::vector<std::string> args) {
  auto argv = make_exec_argv(args);
  const pid_t pid = fork();
  if (pid < 0) {
    std::cerr << "tmux-bar: fork failed: " << std::strerror(errno) << '\n';
    return 1;
  }
  if (pid == 0) {
    execvp("tmux", argv.data());
    _exit(127);
  }
  const int rc = wait_child(pid);
  if (rc == 127) {
    std::cerr << "tmux-bar: failed to exec tmux\n";
  }
  return rc;
}

RunResult run_tmux_capture(std::vector<std::string> args) {
  auto argv = make_exec_argv(args);
  int pipefd[2];
  if (pipe(pipefd) < 0) {
    std::cerr << "tmux-bar: pipe failed: " << std::strerror(errno) << '\n';
    return {};
  }

  const pid_t pid = fork();
  if (pid < 0) {
    std::cerr << "tmux-bar: fork failed: " << std::strerror(errno) << '\n';
    close(pipefd[0]);
    close(pipefd[1]);
    return {};
  }
  if (pid == 0) {
    close(pipefd[0]);
    if (dup2(pipefd[1], STDOUT_FILENO) < 0) {
      _exit(126);
    }
    close(pipefd[1]);
    execvp("tmux", argv.data());
    _exit(127);
  }

  close(pipefd[1]);
  RunResult result;
  char buf[4096];
  for (;;) {
    const ssize_t n = read(pipefd[0], buf, sizeof(buf));
    if (n > 0) {
      result.stdout_data.append(buf, static_cast<std::size_t>(n));
    } else if (n == 0) {
      break;
    } else if (errno == EINTR) {
      continue;
    } else {
      break;
    }
  }
  close(pipefd[0]);
  result.exit_code = wait_child(pid);
  return result;
}

void push_tmux_prefix(std::vector<std::string>& args, std::string_view socket_name) {
  args.emplace_back("tmux");
  if (!socket_name.empty()) {
    args.emplace_back("-L");
    args.emplace_back(socket_name);
  }
}

int apply_all(std::string_view home_dir, std::string_view socket_name) {
  const auto options = tmux_bar::build_options(home_dir);
  if (options.empty()) {
    return 0;
  }

  // Batch all set-option calls into a single tmux invocation separated by ";".
  std::vector<std::string> cmd;
  push_tmux_prefix(cmd, socket_name);
  for (std::size_t i = 0; i < options.size(); ++i) {
    if (i > 0) {
      cmd.emplace_back(";");
    }
    cmd.emplace_back("set-option");
    cmd.emplace_back(options[i].scope == tmux_bar::Scope::GlobalWindow ? "-gw" : "-g");
    cmd.push_back(options[i].name);
    cmd.push_back(options[i].value);
  }
  if (const int rc = run_tmux(std::move(cmd)); rc != 0) {
    return rc;
  }

  // Apply the single-pane border visibility rule to existing windows (the
  // window-layout-changed hook handles future changes).
  std::vector<std::string> list_cmd;
  push_tmux_prefix(list_cmd, socket_name);
  list_cmd.emplace_back("list-windows");
  list_cmd.emplace_back("-a");
  list_cmd.emplace_back("-F");
  list_cmd.emplace_back("#{window_id}");
  const auto listed = run_tmux_capture(std::move(list_cmd));
  if (listed.exit_code != 0) {
    return 0;
  }

  // Apply per window so a window closing mid-loop (or >100 windows) cannot
  // abort the remaining windows.
  std::size_t start = 0;
  const std::string_view data = listed.stdout_data;
  const std::string rule(tmux_bar::existing_window_border_rule());
  while (start < data.size()) {
    const std::size_t end = data.find('\n', start);
    const std::string_view line =
        (end == std::string_view::npos) ? data.substr(start) : data.substr(start, end - start);
    if (!line.empty()) {
      std::vector<std::string> border_cmd;
      push_tmux_prefix(border_cmd, socket_name);
      border_cmd.emplace_back("set-option");
      border_cmd.emplace_back("-wF");
      border_cmd.emplace_back("-t");
      border_cmd.emplace_back(line);
      border_cmd.emplace_back("pane-border-status");
      border_cmd.push_back(rule);
      (void)run_tmux(std::move(border_cmd));
    }
    if (end == std::string_view::npos) {
      break;
    }
    start = end + 1;
  }

  return 0;
}

void print_usage() {
  std::cerr << "usage: tmux-bar [-L socket] [apply|dump|--print]\n";
}

}  // namespace

int main(int argc, char* argv[]) {
  std::string socket_name;
  std::string subcommand = "apply";

  for (int i = 1; i < argc; ++i) {
    const std::string_view arg(argv[i]);
    if (arg == "-L") {
      if (i + 1 >= argc) {
        print_usage();
        return 2;
      }
      socket_name = argv[++i];
    } else if (arg == "apply" || arg == "dump" || arg == "--print") {
      subcommand = std::string(arg);
    } else if (arg == "-h" || arg == "--help") {
      print_usage();
      return 0;
    } else {
      std::cerr << "tmux-bar: unknown argument: " << arg << '\n';
      print_usage();
      return 2;
    }
  }

  const char* home_env = std::getenv("HOME");
  const std::string_view home_dir = (home_env != nullptr) ? std::string_view(home_env) : "";

  if (subcommand == "dump" || subcommand == "--print") {
    std::cout << tmux_bar::render_tmux_script(tmux_bar::build_options(home_dir));
    return 0;
  }

  return apply_all(home_dir, socket_name);
}
