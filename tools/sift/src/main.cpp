// sift — popup regex search over a pane's scrollback, bound to `prefix /`.
//
// The regex sibling of `seek` (prefix Space). seek can lean on tmux's built-in
// incremental search because that search is plain-text; tmux has no incremental
// *regex* search to borrow, so this tool owns the interaction loop instead.
// Rationale and the scope line against ADR-0001:
//   docs/adr/0005-own-the-interaction-loop-for-regex-search.org
//
// Usage:
//   sift [pane-id]                  the popup TUI (pane defaults to the active
//                                   one — see origin_pane() for why the binding
//                                   cannot simply pass it in)
//   sift rows <pane-id> <regex>     one line per match, no TUI — the seam every
//                                   headless test asserts against, mirroring
//                                   `cc-fleet rows`. Fields:
//                                   line, char_start, char_end, cell_start, text
//
// Invariants inherited from tools/ARCHITECTURE.org:
//   * Always exit 0. A non-zero exit from a key binding surfaces as a tmux
//     error popup.
//   * Pane text never enters a format context: no pane options are stamped and
//     every display-message carries -l. Same proof obligation seek discharges;
//     filtering the text instead would corrupt what the user wants to read.
//   * Never write pane titles.
//   * tmux is always spawned via execvp with an argv vector — never a shell
//     string — so a scrollback line full of shell metacharacters is inert.
//
// C++ conventions in this file (C++20, no external dependencies):
//   * Every OS resource is owned by an RAII type — Fd, Regex, RawMode. There is
//     no bare close()/regfree()/tcsetattr on an error path, and no atexit.
//   * Reader parameters are std::string_view; only what outlives a call owns a
//     std::string.
//   * Fallible lookups return std::optional; there are no `bool* ok` out-params.
//   * The UTF-8 decoder is constexpr and carries its unit tests as
//     static_assert, so a byte/char/cell mix-up fails the build, not the pane.

#include <algorithm>
#include <array>
#include <cerrno>
#include <charconv>
#include <clocale>
#include <concepts>
#include <cstdio>
#include <cstring>
#include <format>
#include <iterator>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <fcntl.h>
#include <poll.h>
#include <regex.h>
#include <signal.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>
#include <wchar.h>

namespace {

// ── RAII: owned OS handles ─────────────────────────────────────────────────
//
// The three resources this program acquires — a pipe, a compiled regex, the
// terminal's line discipline — each used to be released by hand on every exit
// path. That is the shape that leaks the first time someone adds an early
// return, so each one now has an owner whose destructor is the release.

// A self-closing file descriptor. Move-only: two owners closing one fd would
// be a double close, and after fork() the child needs to drop its copy anyway.
class Fd {
public:
    Fd() = default;
    explicit Fd(int fd) noexcept : fd_(fd) {}
    ~Fd() { reset(); }

    Fd(const Fd&)            = delete;
    Fd& operator=(const Fd&) = delete;
    Fd(Fd&& other) noexcept : fd_(std::exchange(other.fd_, -1)) {}
    Fd& operator=(Fd&& other) noexcept {
        if (this != &other) { reset(); fd_ = std::exchange(other.fd_, -1); }
        return *this;
    }

    [[nodiscard]] int  get() const noexcept { return fd_; }
    [[nodiscard]] bool valid() const noexcept { return fd_ >= 0; }
    void reset() noexcept {
        if (fd_ >= 0) ::close(fd_);
        fd_ = -1;
    }

private:
    int fd_ = -1;
};

// A compiled POSIX regex that frees itself.
//
// Note what the destructor is NOT armed for: POSIX leaves *preg undefined when
// regcomp() fails, so a failed compile owns nothing. (The hand-written version
// called regfree() there; glibc tolerates it, but it was outside the contract
// and the RAII form makes the distinction structural rather than remembered.)
class Regex {
public:
    Regex() = default;
    ~Regex() { reset(); }

    Regex(const Regex&)            = delete;
    Regex& operator=(const Regex&) = delete;
    Regex(Regex&& other) noexcept : re_(other.re_), live_(std::exchange(other.live_, false)) {}
    Regex& operator=(Regex&& other) noexcept {
        if (this != &other) {
            reset();
            re_   = other.re_;
            live_ = std::exchange(other.live_, false);
        }
        return *this;
    }

    // Returns the POSIX error text, or an empty string on success.
    [[nodiscard]] std::string compile(const std::string& pattern) {
        reset();
        regex_t re;
        const int rc = regcomp(&re, pattern.c_str(), REG_EXTENDED);
        if (rc != 0) {
            std::array<char, 128> buf{};
            regerror(rc, &re, buf.data(), buf.size());
            return std::string(buf.data());
        }
        re_   = re;
        live_ = true;
        return {};
    }

    [[nodiscard]] bool           valid() const noexcept { return live_; }
    [[nodiscard]] const regex_t* get() const noexcept { return &re_; }

    void reset() noexcept {
        if (live_) regfree(&re_);
        live_ = false;
    }

private:
    regex_t re_{};
    bool    live_ = false;
};

// ── tmux plumbing ──────────────────────────────────────────────────────────
//
// The Rust crates get this from tmuxlib; there is no C++ tmuxlib, so these two
// functions are the whole of what had to be re-implemented on this side.

// Run tmux with `args` (argv[0] is supplied here) and return its stdout, or
// nullopt when tmux did not exit 0. stderr is left attached to ours so a real
// tmux error is still visible when debugging by hand.
std::optional<std::string> tmux_out(const std::vector<std::string>& args) {
    int raw_fds[2];
    if (pipe(raw_fds) != 0) return std::nullopt;
    Fd read_end{raw_fds[0]};
    Fd write_end{raw_fds[1]};

    const pid_t pid = fork();
    if (pid < 0) return std::nullopt;

    if (pid == 0) {
        // Child. _exit() skips destructors by design — nothing here may run the
        // parent's cleanup — so the fds it must drop are dropped explicitly.
        read_end.reset();
        if (dup2(write_end.get(), STDOUT_FILENO) < 0) _exit(127);
        write_end.reset();
        std::vector<char*> argv;
        argv.reserve(args.size() + 2);
        // execvp's argv is char* const[] for historical reasons; it does not
        // write through them, which is why the const_cast is the standard idiom.
        argv.push_back(const_cast<char*>("tmux"));
        for (const auto& a : args) argv.push_back(const_cast<char*>(a.c_str()));
        argv.push_back(nullptr);
        execvp("tmux", argv.data());
        _exit(127);
    }

    write_end.reset();   // the parent must not hold it, or the read never ends
    std::string           out;
    std::array<char, 65536> buf;
    ssize_t                 n;
    while ((n = read(read_end.get(), buf.data(), buf.size())) > 0)
        out.append(buf.data(), static_cast<size_t>(n));
    read_end.reset();

    int status = 0;
    while (waitpid(pid, &status, 0) < 0 && errno == EINTR) {}
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) return std::nullopt;
    return out;
}

bool tmux_run(const std::vector<std::string>& args) {
    return tmux_out(args).has_value();
}

// A tmux command list is `cmd args ; cmd args ; …`. Building it by hand read as
// vector surgery (`a.insert(a.end(), {";", "send-keys", …})`) and buried the
// separator in every call site; chain() puts the commands back in the foreground
// and owns the `;` itself. The concept is what keeps a stray int or char* length
// from being appended as if it were an argument.
template <typename T>
concept StringLike = std::convertible_to<T, std::string>;

template <StringLike... Args>
void chain(std::vector<std::string>& cmds, Args&&... args) {
    if (!cmds.empty()) cmds.emplace_back(";");
    (cmds.emplace_back(std::forward<Args>(args)), ...);
}

// tmux answers a multi-field display-message with tab-separated integers — one
// call so every field describes ONE instant. from_chars parses them without
// errno, without locale, and without strtol's habit of returning 0 for both
// "the field said 0" and "the field was garbage" via a shared global.
template <size_t N>
std::array<long, N> parse_fields(std::string_view s) {
    std::array<long, N> out{};
    for (size_t i = 0; i < N; ++i) {
        const size_t     tab   = s.find('\t');
        std::string_view field = s.substr(0, tab);
        std::from_chars(field.data(), field.data() + field.size(), out[i]);
        if (tab == std::string_view::npos) break;
        s.remove_prefix(tab + 1);
    }
    return out;
}

// Every user-facing message is literal (-l): pane text must never be parsed as
// a format string.
void say(const std::string& text) {
    tmux_run({"display-message", "-l", text});
}

// ── UTF-8 ──────────────────────────────────────────────────────────────────
//
// Two different "columns" exist and confusing them is the classic bug here
// (ADR-0004 was written about its sibling in seek):
//   * regexec reports BYTE offsets;
//   * copy-mode's `cursor-right` moves by CHARACTER;
//   * #{copy_cursor_x} and the screen are measured in CELLS (CJK = 2).
// Nothing below ever mixes them silently.

struct Decoded {
    char32_t cp  = 0;
    int      len = 1;   // >=1 always, so a malformed byte advances rather than looping
};

// Decode one code point at `i`. constexpr so the static_asserts below are the
// decoder's unit tests: a regression fails the build instead of the pane.
[[nodiscard]] constexpr Decoded utf8_decode(std::string_view s, size_t i) noexcept {
    const size_t avail = s.size() - i;
    const auto   at    = [&](size_t k) { return static_cast<unsigned char>(s[i + k]); };
    const auto   cont  = [&](size_t k) { return k < avail && (at(k) & 0xC0) == 0x80; };

    const unsigned char c = at(0);
    if (c < 0x80) return {c, 1};
    if ((c & 0xE0) == 0xC0 && cont(1))
        return {static_cast<char32_t>(((c & 0x1F) << 6) | (at(1) & 0x3F)), 2};
    if ((c & 0xF0) == 0xE0 && cont(1) && cont(2))
        return {static_cast<char32_t>(((c & 0x0F) << 12) | ((at(1) & 0x3F) << 6) | (at(2) & 0x3F)), 3};
    if ((c & 0xF8) == 0xF0 && cont(1) && cont(2) && cont(3))
        return {static_cast<char32_t>(((c & 0x07) << 18) | ((at(1) & 0x3F) << 12) |
                                      ((at(2) & 0x3F) << 6) | (at(3) & 0x3F)),
                4};
    return {0xFFFD, 1};
}

// Characters in s[0, byte_end) — this is what `cursor-right -N` counts.
[[nodiscard]] constexpr int utf8_chars(std::string_view s, size_t byte_end) noexcept {
    int n = 0;
    for (size_t i = 0; i < byte_end && i < s.size(); ++n)
        i += static_cast<size_t>(utf8_decode(s, i).len);
    return n;
}

// The decoder is the one piece with no runtime test seam, and the one where a
// byte/char/cell mix-up hides. These run at compile time.
static_assert(utf8_decode("a", 0).len == 1 && utf8_decode("a", 0).cp == U'a');
static_assert(utf8_decode("中", 0).len == 3 && utf8_decode("中", 0).cp == U'中');
static_assert(utf8_decode("\xE4\xB8", 0).len == 1);          // truncated: advance, don't spin
static_assert(utf8_decode("\xFF", 0).cp == 0xFFFD);          // invalid lead byte
static_assert(utf8_chars("中文 aa", 6) == 2);                // 2 CJK chars == 6 bytes
static_assert(utf8_chars("中文 aa", 9) == 5);   // 中 文 SP a a

[[nodiscard]] int cell_width(char32_t cp) noexcept {
    // wcwidth takes wchar_t; on every platform this tool runs on that is UCS-4,
    // so the cast is a spelling change, not a narrowing one.
    static_assert(sizeof(wchar_t) >= sizeof(char32_t),
                  "wcwidth cannot represent a full code point on this platform");
    const int w = wcwidth(static_cast<wchar_t>(cp));
    return w < 0 ? 1 : w;   // render control/unknown as one cell rather than vanish
}

// Cells occupied by s[0, byte_end) — what #{copy_cursor_x} counts.
[[nodiscard]] int utf8_cells(std::string_view s, size_t byte_end) {
    int n = 0;
    for (size_t i = 0; i < byte_end && i < s.size();) {
        const auto d = utf8_decode(s, i);
        n += cell_width(d.cp);
        i += static_cast<size_t>(d.len);
    }
    return n;
}

// Inverse of utf8_cells: the byte length of the longest prefix of `s` that
// fits in `budget` cells. Same decoder and same width table, walked forwards
// until the budget runs out, so the cut always lands on a character boundary —
// a byte-boundary cut would emit half of a `↑`.
[[nodiscard]] size_t utf8_fit(std::string_view s, int budget) {
    size_t i = 0;
    for (int n = 0; i < s.size();) {
        const auto d = utf8_decode(s, i);
        const int  w = cell_width(d.cp);
        if (n + w > budget) break;
        n += w;
        i += static_cast<size_t>(d.len);
    }
    return i;
}

// Which pane are we searching?
//
// The obvious answer — have the key binding pass `#{pane_id}` — does not work.
// Measured on tmux 3.5a: `display-popup`'s shell-command is NOT format-expanded,
// so the program receives the literal seven characters `#{pane_id}`; neither is
// `-e VAR=#{pane_id}`. (`run-shell` does expand, which is what makes the
// difference easy to miss.) And `$TMUX_PANE` inside a popup names the POPUP's
// own pseudo-pane, not the pane it was opened over — using it would search the
// wrong thing rather than fail loudly.
//
// What does work is asking tmux directly: while a popup is open the client's
// active pane is still the one the key was pressed in.
//
// An explicit argument is honoured when given (the `rows` seam and the tests
// rely on it) — except when it still looks like an unexpanded format, which
// means a binding regressed and is better self-healed than flashed away.
std::string origin_pane(const char* arg) {
    if (arg && *arg && std::string_view(arg).substr(0, 2) != "#{") return arg;
    auto s = tmux_out({"display-message", "-p", "#{pane_id}"});
    if (!s) return {};
    while (!s->empty() && (s->back() == '\n' || s->back() == '\r')) s->pop_back();
    return *s;
}

// ── pane inspection ────────────────────────────────────────────────────────

struct Geom {
    long history_size = 0;
    long height       = 0;
    bool alternate    = false;
};

std::optional<Geom> pane_geom(const std::string& pane) {
    const auto s = tmux_out({"display-message", "-p", "-t", pane,
                             "#{history_size}\t#{pane_height}\t#{alternate_on}"});
    if (!s) return std::nullopt;
    const auto f = parse_fields<3>(*s);
    return Geom{.history_size = f[0], .height = f[1], .alternate = f[2] != 0};
}

// Capture history + visible screen. Deliberately no -J: joining wrapped lines
// would desynchronise our indices from the physical lines copy-mode navigates.
// No -e either: escape sequences would be matched by the regex and rendered raw.
// Index i in the result is physical line i counting from the top of history —
// the coordinate the jump arithmetic below is written in.
std::vector<std::string> capture(const std::string& pane, const Geom& g) {
    std::vector<std::string> lines;
    const auto blob = tmux_out({"capture-pane", "-p", "-t", pane,
                                "-S", "-" + std::to_string(g.history_size),
                                "-E", std::to_string(g.height - 1)});
    if (!blob) return lines;

    std::string_view rest{*blob};
    while (!rest.empty()) {
        const size_t nl = rest.find('\n');
        if (nl == std::string_view::npos) {
            lines.emplace_back(rest);
            break;
        }
        lines.emplace_back(rest.substr(0, nl));
        rest.remove_prefix(nl + 1);
    }
    return lines;
}

// ── matching ───────────────────────────────────────────────────────────────
//
// POSIX regcomp(REG_EXTENDED), not std::regex: the jump is ultimately performed
// by tmux's own search, so our match set has to be the same one tmux would find
// or the list would show hits the jump cannot reproduce. (Verified by the
// differential test in records/.../verify-sift-regex-parity.sh.) std::regex is
// ECMAScript by default and an order of magnitude slower besides.

struct Hit {
    long line       = 0;   // index into the capture
    int  char_start = 0;   // characters before the match — feeds `cursor-right -N`
    int  char_end   = 0;   // characters before the match END — the cursor seat
    int  cell_start = 0;   // CELLS before the match — what #{copy_cursor_x} reports
    int  byte_start = 0;
    int  byte_end   = 0;
};

// Every occurrence, in scrollback order. A line with three hits yields three
// entries: the user picks an occurrence, not a line, and the jump is exact.
std::vector<Hit> find_all(const std::vector<std::string>& lines, const Regex& re, size_t cap) {
    std::vector<Hit> hits;
    for (size_t i = 0; i < lines.size(); ++i) {
        const std::string& s = lines[i];
        if (s.empty()) continue;
        size_t off = 0;
        while (off <= s.size()) {
            regmatch_t m;
            const int  flags = off == 0 ? 0 : REG_NOTBOL;
            if (regexec(re.get(), s.c_str() + off, 1, &m, flags) != 0) break;
            const size_t b = off + static_cast<size_t>(m.rm_so);
            const size_t e = off + static_cast<size_t>(m.rm_eo);
            hits.push_back(Hit{.line       = static_cast<long>(i),
                               .char_start = utf8_chars(s, b),
                               .char_end   = utf8_chars(s, e),
                               .cell_start = utf8_cells(s, b),
                               .byte_start = static_cast<int>(b),
                               .byte_end   = static_cast<int>(e)});
            if (hits.size() >= cap) return hits;
            // A zero-width match (e.g. `^`, `x*`) would spin here forever.
            off = (e == b) ? e + 1 : e;
        }
    }
    return hits;
}

// ── the jump ───────────────────────────────────────────────────────────────
//
// Measured on tmux 3.5a, because none of this behaves the way the command names
// suggest (probe scripts in records/.../assets/scripts/):
//
//   * `goto-line N` does NOT go to line N. It sets `oy`, the scroll offset from
//     the bottom of the history, and leaves the cursor row `cy` untouched:
//     absolute line = history_size - oy + cy. So the cursor row must be pinned
//     first — `history-top` puts it at cy=0 — and only then is
//     `goto-line (history_size - i)` an exact seek to line i.
//   * `oy` is NOT stable while the pane keeps printing; the index from the top
//     of history is. history_size is therefore re-read at jump time, not reused
//     from the capture.
//   * `search-forward` leaves the cursor one cell PAST the end of the match;
//     `search-backward` leaves it on the match START. seek's w/W/l/L read the
//     token under the cursor, so it has to be the backward one — which means
//     seating the cursor just past the chosen occurrence and searching back
//     onto it.
//   * Both searches WRAP silently when they fail, so a bad landing is not
//     reported by tmux. Hence the verification below.
//
// The whole sequence goes out as one tmux command list: the popup is still open
// while it runs (panes do not redraw under a popup) and the pane is only seen
// after this process exits and the popup closes.
bool jump(const std::string& pane, const std::string& pattern,
          long line, int char_end, int cell_start) {
    const auto now = pane_geom(pane);
    if (!now) return false;

    std::vector<std::string> cmds;
    chain(cmds, "copy-mode", "-t", pane);
    chain(cmds, "send-keys", "-X", "-t", pane, "history-top");

    if (line <= now->history_size) {
        chain(cmds, "send-keys", "-X", "-t", pane,
              "goto-line", std::to_string(now->history_size - line));
    } else {
        // Target is in the visible screen: pin the viewport to the bottom and
        // step down to the row.
        const long down = line - now->history_size;
        chain(cmds, "send-keys", "-X", "-t", pane, "goto-line", "0");
        if (down > 0)
            chain(cmds, "send-keys", "-X", "-N", std::to_string(down), "-t", pane, "cursor-down");
    }

    if (char_end > 0)
        chain(cmds, "send-keys", "-X", "-N", std::to_string(char_end), "-t", pane, "cursor-right");

    // Registering the pattern with tmux is the point of this step, not just the
    // positioning: it is what makes the match highlight, `n`/`N`, and seek's
    // grab keys work afterwards.
    chain(cmds, "send-keys", "-X", "-t", pane, "search-backward", pattern);

    if (!tmux_run(cmds)) return false;

    // Verify rather than assume: both searches wrap silently, so a miss lands
    // somewhere plausible instead of reporting itself.
    //
    // The check is positional, NOT textual. #{copy_cursor_line} looks like the
    // obvious probe and is a trap — measured on 3.5a it truncates at the first
    // wide character (a line reading "中文測試 aa999 尾巴" comes back as "中"),
    // so comparing text would false-alarm on every CJK line. Cursor coordinates
    // have no such problem.
    //
    // All five fields come from ONE call so they describe one instant, and
    // `history_size - scroll_position` is stable while the pane keeps printing:
    // copy-mode holds its view, so new output grows both terms together.
    const auto s = tmux_out({"display-message", "-p", "-t", pane,
                             "#{history_size}\t#{scroll_position}\t#{copy_cursor_y}"
                             "\t#{copy_cursor_x}\t#{search_present}"});
    if (!s) return false;

    const auto f      = parse_fields<5>(*s);
    const long landed = f[0] - f[1] + f[2];
    return f[4] == 1 && landed == line && f[3] == cell_start;
}

// ── terminal ───────────────────────────────────────────────────────────────

// The only global left. A signal handler may touch nothing else.
volatile sig_atomic_t g_resized = 0;

void on_winch(int) { g_resized = 1; }

// write() is allowed to write less than asked; on a pty under a full redraw it
// does. The old `(void)r` discarded exactly the information needed to notice.
void write_all(int fd, std::string_view s) noexcept {
    while (!s.empty()) {
        const ssize_t n = write(fd, s.data(), s.size());
        if (n < 0) {
            if (errno == EINTR) continue;
            return;                      // the terminal is gone; nothing to salvage
        }
        s.remove_prefix(static_cast<size_t>(n));
    }
}

// Raw mode and the alternate screen, released by the destructor. restore() is
// idempotent and public because Enter's path must return the terminal BEFORE
// the target pane redraws, not at scope exit.
class RawMode {
public:
    RawMode() = default;
    ~RawMode() { restore(); }

    RawMode(const RawMode&)            = delete;
    RawMode& operator=(const RawMode&) = delete;

    [[nodiscard]] bool engage() noexcept {
        if (tcgetattr(STDIN_FILENO, &saved_) != 0) return false;
        termios t = saved_;
        t.c_lflag &= ~static_cast<tcflag_t>(ECHO | ICANON | ISIG | IEXTEN);
        t.c_iflag &= ~static_cast<tcflag_t>(IXON | ICRNL | INLCR | BRKINT | ISTRIP);
        t.c_oflag &= ~static_cast<tcflag_t>(OPOST);
        t.c_cc[VMIN]  = 1;
        t.c_cc[VTIME] = 0;
        if (tcsetattr(STDIN_FILENO, TCSAFLUSH, &t) != 0) return false;
        raw_ = true;
        write_all(STDOUT_FILENO, "\x1b[?1049h\x1b[2J");
        return true;
    }

    void restore() noexcept {
        if (!raw_) return;
        tcsetattr(STDIN_FILENO, TCSAFLUSH, &saved_);
        raw_ = false;
        // Leave the alternate screen and re-show the cursor.
        write_all(STDOUT_FILENO, "\x1b[?25h\x1b[?1049l");
    }

private:
    termios saved_{};
    bool    raw_ = false;
};

struct TermSize {
    int w = 80;
    int h = 24;
};

[[nodiscard]] TermSize term_size() noexcept {
    winsize ws{};
    if (ioctl(STDOUT_FILENO, TIOCGWINSZ, &ws) == 0 && ws.ws_col > 0 && ws.ws_row > 0)
        return {ws.ws_col, ws.ws_row};
    return {};   // a sane 80x24 rather than a division by zero
}

// ── key decoding ───────────────────────────────────────────────────────────

enum class Key {
    None, Enter, Esc, Backspace, Up, Down, PgUp, PgDn, Home, End, KillWord, KillLine, Text,
    AltDigit   // ESC + '0'..'9', i.e. left Alt-<digit> — ordinal mode (ADR-0007)
};

struct Input {
    Key         key = Key::None;
    std::string text;   // for Key::Text: one whole UTF-8 character
};

// A keypress that carries no text. Spelled out rather than `{.key = k}` so both
// members are initialised explicitly and -Wmissing-field-initializers stays on.
[[nodiscard]] Input key_only(Key k) { return Input{.key = k, .text = {}}; }

// Read with a timeout; -1 on timeout/error.
int read_byte(int timeout_ms) {
    pollfd p{STDIN_FILENO, POLLIN, 0};
    if (poll(&p, 1, timeout_ms) <= 0) return -1;
    unsigned char c;
    if (read(STDIN_FILENO, &c, 1) != 1) return -1;
    return c;
}

Input read_key() {
    Input     in;
    const int c = read_byte(-1);
    if (c < 0) return key_only(Key::Esc);

    switch (c) {
        case '\r': case '\n': return key_only(Key::Enter);
        case 127:  case 8:    return key_only(Key::Backspace);
        case 23:              return key_only(Key::KillWord);   // C-w
        case 21:              return key_only(Key::KillLine);   // C-u
        case 3:   case 7:     return key_only(Key::Esc);        // C-c / C-g
        case 16:              return key_only(Key::Up);         // C-p
        case 14:              return key_only(Key::Down);       // C-n
        default: break;
    }

    if (c == 0x1b) {
        // A bare Escape is cancel; an escape SEQUENCE is a cursor key. The only
        // thing separating them is timing, so give the rest of the sequence a
        // brief window to arrive.
        const int c1 = read_byte(40);
        if (c1 < 0) return key_only(Key::Esc);
        // ESC followed by a digit is left Alt-<digit> — alacritty's
        // Meta-as-ESC-prefix default, measured on this machine through a real
        // keypress. ADR-0007 claims those bytes for ordinal mode; until now
        // they fell through to Key::None below and were discarded. The 40 ms
        // window above is still the only thing separating this from a bare Esc.
        if (c1 >= '0' && c1 <= '9')
            return Input{.key = Key::AltDigit, .text = std::string(1, static_cast<char>(c1))};
        if (c1 != '[' && c1 != 'O') return key_only(Key::None);
        const int c2 = read_byte(40);
        if (c2 < 0) return key_only(Key::Esc);
        switch (c2) {
            case 'A': return key_only(Key::Up);
            case 'B': return key_only(Key::Down);
            case 'H': return key_only(Key::Home);
            case 'F': return key_only(Key::End);
            case '5': case '6':
                read_byte(40);                                 // consume the '~'
                return key_only((c2 == '5') ? Key::PgUp : Key::PgDn);
            default: return key_only(Key::None);                // unknown CSI: ignore
        }
    }

    if (c < 32) return key_only(Key::None);   // other control bytes: ignore

    // A printable byte, possibly the head of a multi-byte character.
    in.key = Key::Text;
    in.text.push_back(static_cast<char>(c));
    int extra = 0;
    if ((c & 0xE0) == 0xC0) extra = 1;
    else if ((c & 0xF0) == 0xE0) extra = 2;
    else if ((c & 0xF8) == 0xF0) extra = 3;
    for (int i = 0; i < extra; ++i) {
        const int n = read_byte(40);
        if (n < 0) break;
        in.text.push_back(static_cast<char>(n));
    }
    return in;
}

// ── rendering ──────────────────────────────────────────────────────────────

// Named once so a stray digit in an escape sequence is a compile error at the
// definition rather than a scrambled screen at 2am.
constexpr std::string_view kHomeClear   = "\x1b[H\x1b[2J";
constexpr std::string_view kBold        = "\x1b[1m";
constexpr std::string_view kDim         = "\x1b[2m";
constexpr std::string_view kUnbold      = "\x1b[22m";
constexpr std::string_view kReset       = "\x1b[0m";
constexpr std::string_view kInvertOn    = "\x1b[7m";
constexpr std::string_view kInvertOff   = "\x1b[27m";
// The ordinal column is the one number the user types (ADR-0007), so it is
// coloured rather than dimmed — a second dim number beside the line number
// would be indistinguishable from it at a glance.
constexpr std::string_view kCyan        = "\x1b[36m";
constexpr std::string_view kDefaultFg   = "\x1b[39m";

// Render one capture line into `width` cells, guaranteeing the match is on
// screen (long lines scroll horizontally so a hit at column 400 is still seen)
// and reverse-videoing the matched span.
std::string render_line(std::string_view s, int byte_start, int byte_end, int width) {
    if (width <= 0) return {};

    // Cell offset of the match start, and of every byte we may need to cut at.
    const int cells_to_start = utf8_cells(s, static_cast<size_t>(std::max(byte_start, 0)));

    // Scroll so the match sits about a third in, but never past the line start.
    int skip_cells = 0;
    if (cells_to_start > width - 12) skip_cells = cells_to_start - width / 3;
    if (skip_cells < 0) skip_cells = 0;

    std::string o;
    int         cells = 0, seen = 0;
    bool        inverted = false;
    if (skip_cells > 0) { o += "…"; cells = 1; }

    for (size_t i = 0; i < s.size();) {
        const auto d = utf8_decode(s, i);
        const int  w = cell_width(d.cp);
        if (seen + w > skip_cells) {
            if (cells + w > width) break;
            const bool want = (static_cast<int>(i) >= byte_start && static_cast<int>(i) < byte_end);
            if (want && !inverted)       { o += kInvertOn;  inverted = true; }
            else if (!want && inverted)  { o += kInvertOff; inverted = false; }
            o.append(s, i, static_cast<size_t>(d.len));
            cells += w;
        }
        seen += w;
        i += static_cast<size_t>(d.len);
    }
    if (inverted) o += kInvertOff;
    return o;
}

// ── the popup ──────────────────────────────────────────────────────────────

// Hard ceiling on collected matches. `.` over a full 100k-line scrollback is a
// legitimate keystroke on the way to a real pattern; the cap keeps that from
// costing seconds. It is reported in the header rather than silently applied.
constexpr size_t kMatchCap = 20000;

struct Ui {
    std::string              pane;
    std::vector<std::string> lines;
    Geom                     geom;
    std::string              pattern;
    std::vector<Hit>         hits;
    size_t                   sel    = 0;
    size_t                   top    = 0;   // first visible hit
    bool                     capped = false;
    std::string              re_error;     // non-empty == the pattern does not compile
    std::string              goto_buf;     // non-empty == ordinal mode is live (ADR-0007)
};

void refilter(Ui& u) {
    u.hits.clear();
    u.capped = false;
    u.re_error.clear();
    if (u.pattern.empty()) { u.sel = 0; u.top = 0; return; }

    Regex re;
    // Half-typed patterns are invalid most of the time. Keeping the previous
    // result set on screen would be a lie about what the pattern matches, so
    // the list empties but the header says why.
    u.re_error = re.compile(u.pattern);
    if (!re.valid()) return;

    u.hits   = find_all(u.lines, re, kMatchCap);
    u.capped = u.hits.size() >= kMatchCap;

    // Default to the match nearest the bottom — the same "most recent first"
    // bias as the search-backward binding this replaces.
    u.sel = u.hits.empty() ? 0 : u.hits.size() - 1;
    u.top = 0;
}

// ── ordinal mode ───────────────────────────────────────────────────────────
//
// The buffer and the selection are ONE object (ADR-0007): u.goto_buf holds the
// digits typed so far and u.sel is always that ordinal minus one, so Enter
// confirms something already on screen rather than wagering on a number. A
// non-empty buffer IS the mode — no second flag, because the mode is entered
// with its first digit already in hand and leaves the instant the last one is
// popped.
//
// Load-bearing, not incidental: every exit from the mode happens BEFORE the
// pattern can change, so refilter() can never run while a buffer is alive and
// the ordinals it names are frozen for the whole life of that buffer. That is
// what removes the "no such match" error path — keep it true of any key added
// later.

[[nodiscard]] bool in_ordinal_mode(const Ui& u) noexcept { return !u.goto_buf.empty(); }

// The 1-based ordinal a digit buffer names. Every digit in the buffer was
// bounds-checked against u.hits.size() (<= kMatchCap, so at most five digits)
// before it was appended, which is why this needs no overflow guard.
[[nodiscard]] size_t ordinal_of(std::string_view digits) noexcept {
    size_t v = 0;
    for (const char c : digits) v = v * 10 + static_cast<size_t>(c - '0');
    return v;
}

// Start or extend the buffer with one digit. A keystroke that would push the
// ordinal past N is NOT buffered — buffer and selection are left exactly as
// they were, and there is no error to render. With N = 12, `Alt-1` then `2`
// reaches 12 (1 and 12 are both in range) while `Alt-1` then `5` stays on 1.
// Ordinal 0 is the same refusal: `Alt-0` names no candidate, and neither does
// an empty list or a pattern that does not compile.
void push_ordinal_digit(Ui& u, char digit) {
    if (u.hits.empty() || !u.re_error.empty()) return;
    std::string next = u.goto_buf;
    next.push_back(digit);
    const size_t ord = ordinal_of(next);
    if (ord == 0 || ord > u.hits.size()) return;
    u.goto_buf = std::move(next);
    u.sel      = ord - 1;
}

// Every key that MOVES the selection rewrites the buffer to the new selection's
// ordinal, so `goto>` always names where the cursor actually is. ADR-0007
// states that invariant for the arrow keys; it only holds if PgUp/PgDn and
// Home/End keep it too.
void sync_ordinal(Ui& u) {
    if (in_ordinal_mode(u)) u.goto_buf = std::to_string(u.sel + 1);
}

void draw(Ui& u) {
    const auto [w, h] = term_size();
    if (h < 4 || w < 20) return;

    const int list_rows = std::max(h - 2, 1);    // header + footer

    // Keep the selection in view.
    if (u.sel < u.top) u.top = u.sel;
    if (u.sel >= u.top + static_cast<size_t>(list_rows))
        u.top = u.sel - static_cast<size_t>(list_rows) + 1;
    if (u.hits.size() <= static_cast<size_t>(list_rows)) u.top = 0;

    std::string o{kHomeClear};

    // Header: prompt on the left, status on the right.
    std::string status;
    if (!u.re_error.empty())     status = "invalid regex: " + u.re_error;
    else if (u.pattern.empty())  status = "type an extended regex";
    else if (u.hits.empty())     status = "no match";
    else                         status = std::to_string(u.hits.size()) +
                                          (u.capped ? "+ matches (capped)" : " matches");
    if (u.geom.alternate) status = "⚠ visible screen only · " + status;

    // The prompt names the live mode, so ordinal mode is never invisible. plen
    // is that prompt (ASCII, so bytes == characters) plus whatever is being
    // typed into it, and it is what parks the real cursor below — a plen
    // computed from the wrong prompt puts the cursor in the wrong cell.
    const std::string_view prompt = in_ordinal_mode(u) ? "goto> " : "regex> ";
    const std::string&     typed  = in_ordinal_mode(u) ? u.goto_buf : u.pattern;

    const int plen = static_cast<int>(prompt.size()) + utf8_chars(typed, typed.size());
    const int slen = utf8_chars(status, status.size());
    o += std::format("{}{}{}{}", kBold, prompt, typed, kReset);
    if (const int gap = w - plen - slen; gap > 0) {
        o.append(static_cast<size_t>(gap), ' ');
        o += std::format("{}{}{}", kDim, status, kReset);
    }
    o += "\r\n";

    // Widest line number in view, so the text column does not jitter.
    const int numw = static_cast<int>(
        std::to_string(u.lines.empty() ? 0 : u.lines.size() - 1).size());

    // Match ordinal: the row's 1-based position in u.hits, so the column is
    // exactly as wide as N. Unlike numw this DOES jitter — N changes on every
    // refilter — which ADR-0007 records and accepts. An empty list renders no
    // rows, so the width is never consulted in that case.
    const int ordw = static_cast<int>(std::to_string(u.hits.size()).size());

    // Row layout, and the whole of the width budget:
    //   "> " or "  "   2
    //   ordinal        ordw
    //   space          1
    //   line number    numw
    //   space          1
    //   text           the rest
    const int textw = w - numw - ordw - 4;

    for (int r = 0; r < list_rows; ++r) {
        const size_t idx = u.top + static_cast<size_t>(r);
        if (idx >= u.hits.size()) { o += "\r\n"; continue; }
        const Hit& hit = u.hits[idx];

        o += (idx == u.sel) ? std::format("{}> ", kBold) : "  ";
        o += std::format("{}{:>{}}{} ", kCyan, idx + 1, ordw, kDefaultFg);
        o += std::format("{}{:>{}}{} ", kDim, hit.line, numw, kUnbold);
        o += render_line(u.lines[static_cast<size_t>(hit.line)],
                         hit.byte_start, hit.byte_end, textw);
        o += kReset;
        o += "\r\n";
    }

    // "left-Alt" is not pedantry: ~/.config/keyd/default.conf gives rightalt to
    // the fcitx5 IME toggle at the kernel level, so it never reaches tmux here.
    // `goto match n`, not a bare `goto`: the verb alone names the action without
    // naming its object, which left "goto what?" to inference. `match` is the one
    // word already anchored on screen — the header counts `N matches`, the cyan
    // column IS the match ordinal — where a `#n` sigil would have had no referent
    // on a row, and `#` in this repo reads as tmux format syntax. Lowercase `n`
    // deliberately: capital N is the TOTAL, so `goto match N` would name the last.
    constexpr std::string_view kFooter =
        "↑↓ select  left-Alt-<n> goto match n  Enter jump  Esc cancel  C-w word  C-u clear";

    // Width guard, and the reason it is a guard rather than a shorter string:
    // draw() emits exactly h lines (header + list_rows + footer), so a footer
    // one cell too wide wraps, the pane scrolls, and the HEADER leaves the
    // screen — taking the `goto>` prompt with it, which is ADR-0007's only
    // on-screen evidence that ordinal mode is live. Any future key added below
    // would bring that back; cutting to the measured width cannot.
    //
    // Cut in cells, not bytes (utf8_fit, the same decoder utf8_cells uses):
    // the footer opens with `↑↓`. kDim/kReset are zero-width, so they are not
    // charged against the budget and stay wrapped around whatever survives.
    o += std::format("{}{}{}", kDim, kFooter.substr(0, utf8_fit(kFooter, w)), kReset);

    // Park the real cursor at the end of the pattern so typing looks normal.
    o += std::format("\x1b[1;{}H\x1b[?25h", plen + 1);
    write_all(STDOUT_FILENO, o);
}

int run_ui(const std::string& pane) {
    Ui         u;
    const auto geom = pane_geom(pane);
    if (!geom) { say("sift: cannot read pane " + pane); return 0; }

    u.pane  = pane;
    u.geom  = *geom;
    u.lines = capture(pane, u.geom);
    if (u.lines.empty()) { say("sift: nothing to search in " + pane); return 0; }

    RawMode term;
    if (!term.engage()) { say("sift: no terminal (run it from a tmux popup)"); return 0; }
    signal(SIGWINCH, on_winch);

    draw(u);
    for (;;) {
        const Input in = read_key();
        if (g_resized) { g_resized = 0; draw(u); }

        switch (in.key) {
            case Key::Esc:
                // Esc leaves the MODE, not sift. Outside the mode it cancels
                // sift exactly as it always has.
                if (in_ordinal_mode(u)) { u.goto_buf.clear(); break; }
                return 0;                                  // cancel: pane untouched
                                                           // (~RawMode restores)

            case Key::Enter: {
                if (u.hits.empty()) break;
                const Hit         hit     = u.hits[u.sel];
                const std::string pattern = u.pattern;
                term.restore();                            // before the pane redraws
                if (!jump(pane, pattern, hit.line, hit.char_end, hit.cell_start))
                    say("sift: the pane moved — landed on the nearest match of /" + pattern + "/");
                return 0;
            }

            case Key::Up:   if (u.sel > 0) --u.sel;                        sync_ordinal(u); break;
            case Key::Down: if (u.sel + 1 < u.hits.size()) ++u.sel;         sync_ordinal(u); break;
            case Key::Home: u.sel = 0;                                      sync_ordinal(u); break;
            case Key::End:  if (!u.hits.empty()) u.sel = u.hits.size() - 1; sync_ordinal(u); break;

            case Key::PgUp: case Key::PgDn: {
                const auto   sz   = term_size();
                const size_t step = static_cast<size_t>(sz.h > 4 ? sz.h - 3 : 1);
                if (in.key == Key::PgUp) u.sel = (u.sel > step) ? u.sel - step : 0;
                else if (!u.hits.empty()) u.sel = std::min(u.sel + step, u.hits.size() - 1);
                sync_ordinal(u);
                break;
            }

            case Key::AltDigit:
                // Entry — and the keystroke is itself the first digit. Pressed
                // again inside the mode it is simply another digit.
                if (!in.text.empty()) push_ordinal_digit(u, in.text[0]);
                break;

            case Key::Backspace: {
                // In ordinal mode Backspace pops one digit, and popping the LAST
                // one leaves the mode. So leaving the mode and deleting a
                // pattern character are never the same keystroke: the second
                // Backspace is the one that starts eating the pattern.
                if (in_ordinal_mode(u)) {
                    u.goto_buf.pop_back();
                    if (in_ordinal_mode(u)) u.sel = ordinal_of(u.goto_buf) - 1;
                    break;
                }
                if (u.pattern.empty()) break;
                size_t i = u.pattern.size();
                while (i > 0 && (static_cast<unsigned char>(u.pattern[i - 1]) & 0xC0) == 0x80) --i;
                if (i > 0) --i;
                u.pattern.resize(i);
                refilter(u);
                break;
            }

            case Key::KillWord: {
                u.goto_buf.clear();          // the pattern is about to change
                size_t i = u.pattern.size();
                while (i > 0 && u.pattern[i - 1] == ' ') --i;
                while (i > 0 && u.pattern[i - 1] != ' ') --i;
                u.pattern.resize(i);
                refilter(u);
                break;
            }

            case Key::KillLine:
                u.goto_buf.clear();          // the pattern is about to change
                u.pattern.clear();
                refilter(u);
                break;

            case Key::Text:
                if (in_ordinal_mode(u)) {
                    // A bare digit extends the buffer; anything else leaves the
                    // mode and is NOT swallowed — it lands in the pattern below.
                    if (in.text.size() == 1 && in.text[0] >= '0' && in.text[0] <= '9') {
                        push_ordinal_digit(u, in.text[0]);
                        break;
                    }
                    u.goto_buf.clear();
                }
                u.pattern += in.text;
                refilter(u);
                break;

            case Key::None:
                break;
        }
        draw(u);
    }
}

// ── headless seam ──────────────────────────────────────────────────────────

int run_rows(const std::string& pane, const std::string& pattern) {
    const auto g = pane_geom(pane);
    if (!g) return 0;
    const std::vector<std::string> lines = capture(pane, *g);

    Regex            re;
    const std::string err = re.compile(pattern);
    if (!re.valid()) {
        fprintf(stderr, "sift: invalid regex: %s\n", err.c_str());
        return 0;                                  // still exit 0 — see invariants
    }

    // One buffer, one write: the perf assertion in verify-sift-live.sh times a
    // full `rows` run, and a syscall per row would be measuring the pipe.
    std::string out;
    for (const Hit& h : find_all(lines, re, kMatchCap))
        std::format_to(std::back_inserter(out), "{}\t{}\t{}\t{}\t{}\n", h.line, h.char_start,
                       h.char_end, h.cell_start, lines[static_cast<size_t>(h.line)]);
    write_all(STDOUT_FILENO, out);
    return 0;
}

}  // namespace

int main(int argc, char** argv) {
    setlocale(LC_ALL, "");

    if (argc >= 2 && std::string_view(argv[1]) == "rows") {
        if (argc < 4) {
            fprintf(stderr, "usage: sift rows <pane-id> <regex>\n");
            return 0;
        }
        return run_rows(origin_pane(argv[2]), argv[3]);
    }

    const std::string pane = origin_pane(argc >= 2 ? argv[1] : nullptr);
    if (pane.empty()) {
        fprintf(stderr, "sift: no pane — run it inside tmux, or pass a pane id\n");
        return 0;
    }
    return run_ui(pane);
}
