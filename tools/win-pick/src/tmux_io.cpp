#include "tmux_io.hpp"

#include <cerrno>
#include <fcntl.h>
#include <sys/wait.h>
#include <unistd.h>
#include <utility>

namespace win_pick {

namespace {

class Fd {
 public:
  Fd() = default;
  explicit Fd(int fd) noexcept : fd_(fd) {}
  ~Fd() { reset(); }

  Fd(const Fd&) = delete;
  Fd& operator=(const Fd&) = delete;
  Fd(Fd&& other) noexcept : fd_(std::exchange(other.fd_, -1)) {}
  Fd& operator=(Fd&& other) noexcept {
    if (this != &other) {
      reset();
      fd_ = std::exchange(other.fd_, -1);
    }
    return *this;
  }

  [[nodiscard]] int get() const noexcept { return fd_; }
  void reset() noexcept {
    if (fd_ >= 0) ::close(fd_);
    fd_ = -1;
  }

 private:
  int fd_ = -1;
};

std::vector<std::string> build_argv(const std::vector<std::string>& args,
                                    std::string_view socket) {
  std::vector<std::string> argv;
  argv.reserve(args.size() + 3);
  argv.emplace_back("tmux");
  if (!socket.empty()) {
    argv.emplace_back("-L");
    argv.emplace_back(socket);
  }
  for (const auto& a : args) {
    argv.push_back(a);
  }
  return argv;
}

std::string_view trim_newline(std::string_view s) {
  while (!s.empty() && (s.back() == '\n' || s.back() == '\r')) {
    s.remove_suffix(1);
  }
  return s;
}

}  // namespace

std::optional<std::string> tmux_out(const std::vector<std::string>& args,
                                    std::string_view socket) {
  int raw_fds[2];
  if (::pipe(raw_fds) != 0) return std::nullopt;
  Fd read_end{raw_fds[0]};
  Fd write_end{raw_fds[1]};

  const auto argv_storage = build_argv(args, socket);
  std::vector<char*> argv;
  argv.reserve(argv_storage.size() + 1);
  for (const auto& s : argv_storage) {
    argv.push_back(const_cast<char*>(s.c_str()));
  }
  argv.push_back(nullptr);

  const pid_t pid = ::fork();
  if (pid < 0) return std::nullopt;

  if (pid == 0) {
    read_end.reset();
    if (::dup2(write_end.get(), STDOUT_FILENO) < 0) ::_exit(127);
    write_end.reset();
    const int devnull = ::open("/dev/null", O_WRONLY);
    if (devnull >= 0) {
      ::dup2(devnull, STDERR_FILENO);
      ::close(devnull);
    }
    ::execvp("tmux", argv.data());
    ::_exit(127);
  }

  write_end.reset();
  std::string out;
  char buf[4096];
  for (;;) {
    const ssize_t n = ::read(read_end.get(), buf, sizeof(buf));
    if (n > 0) {
      out.append(buf, static_cast<std::size_t>(n));
    } else if (n == 0) {
      break;
    } else if (errno != EINTR) {
      break;
    }
  }
  read_end.reset();

  int status = 0;
  while (::waitpid(pid, &status, 0) < 0) {
    if (errno != EINTR) return std::nullopt;
  }
  if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) return std::nullopt;
  return out;
}

bool tmux_run(const std::vector<std::string>& args, std::string_view socket) {
  return tmux_out(args, socket).has_value();
}

std::optional<Inventory> fetch_inventory(std::string_view socket) {
  Inventory inv;
  if (auto ctx = tmux_out({"display-message", "-p", "#{session_id}\t#{window_id}"}, socket)) {
    const std::string_view line = trim_newline(*ctx);
    const std::size_t tab = line.find('\t');
    if (tab != std::string_view::npos) {
      inv.current_session_id = std::string(line.substr(0, tab));
      inv.current_window_id = std::string(line.substr(tab + 1));
    }
  }

  auto raw = tmux_out({"list-panes", "-a", "-F", std::string(kPaneListFormat)}, socket);
  if (!raw) return std::nullopt;

  inv.windows = parse_panes_output(*raw, inv.current_session_id);
  return inv;
}

std::string capture_pane_preview(std::string_view pane_id, std::string_view socket) {
  if (pane_id.empty()) return {};
  auto out = tmux_out({"capture-pane", "-e", "-p", "-t", std::string(pane_id)}, socket);
  if (!out) return {};
  return *out;
}

bool jump_to_window(std::string_view window_id, bool same_session, std::string_view socket) {
  if (window_id.empty()) return false;
  if (same_session) {
    return tmux_run({"select-window", "-t", std::string(window_id)}, socket);
  }
  if (tmux_run({"switch-client", "-t", std::string(window_id)}, socket)) {
    return true;
  }
  return tmux_run({"select-window", "-t", std::string(window_id)}, socket);
}

}  // namespace win_pick
