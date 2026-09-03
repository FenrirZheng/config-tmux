// pane-jump — popup pane picker for `prefix g`: list every pane, type part of a
// pane TITLE, Enter switches the client to it.
//
// The sibling of `sift` (prefix /). tmux can list panes and it can filter a
// `choose-tree` once, but it has no as-you-type filter over pane titles, and
// `choose-tree -f` ignores an empty filter — a miss shows the whole tree instead
// of nothing. So this tool owns the interaction loop, exactly as sift does.
// Rationale and the scope line against ADR-0005:
//   docs/adr/0008-own-the-interaction-loop-for-pane-selection.org
//
// Usage:
//   pane-jump                    the popup TUI; the bound form
//   pane-jump rows [fragment]    one line per pane, no TUI — the seam every
//                                headless test asserts against, mirroring
//                                `sift rows` and `cc-fleet rows`. Fields:
//                                pane_id, session:window.pane, command, title.
//                                With a fragment, only the rows the TUI would
//                                show for that query.
//   pane-jump jump <pane-id>     the guarded switch-client, on its own
//
// Invariants inherited from tools/ARCHITECTURE.org:
//   * Always exit 0. A non-zero exit from a key binding surfaces as a tmux
//     error popup.
//   * Pane titles never enter a format context: no pane options are stamped and
//     every status-line message is literal (-l). Same proof obligation seek and
//     sift discharge; a title is untrusted text that only ever reaches this
//     popup's own rendering and the `rows` stdout.
//   * Never write pane titles. The Claude-pane readers (talk ping, cc-attend,
//     cc-fleet) depend on that, so this file issues no title-setting and no
//     window-renaming command at all.
//   * tmux is always spawned via execvp with an argv vector — never a shell
//     string — so a pane title full of shell metacharacters is inert.
//
// Those last two are proved by grep, not by reading, so the prose here does not
// spell out the command names it forbids: a comment naming them would make the
// count non-zero and the check worthless. The greps are in the plan's step 2 and
// in runbooks/switch-pane-by-name.md.
//
// Provenance of the terminal layer: Fd, write_all, tmux_out, tmux_run, say, the
// UTF-8 decoder (utf8_decode / utf8_chars / cell_width / utf8_cells / utf8_fit)
// with its static_assert tests, RawMode, TermSize / term_size, on_winch, the
// escape constants and read_byte / read_key are copied from
// tools/sift/src/main.cpp (lines 74-97, 158-237, 248-324, 516-581, 585-667,
// 673-684 as of 2026-09-03). Three deliberate differences, all narrower than the
// original:
//   * sift's Key::AltDigit (ordinal mode, ADR-0007) is dropped — this list is
//     short enough that up/down suffice, and the spec leaves ordinals as a
//     follow-up.
//   * the CSI decoder also accepts ESC [ 1 ~ and ESC [ 4 ~ (Home / End) and
//     consumes the trailing `~` of any numeric CSI. sift accepts only ESC [ H /
//     ESC [ F, which is the known defect recorded in runbooks/sift.md ("Home /
//     End do nothing, or leak a `~` into the pattern"); fixing it there is a
//     change to sift and is not part of this one.
//   * two members of the 158-237 range above are omitted: sift's StringLike
//     concept with chain() (sift lines 202-214) and parse_fields (sift lines
//     216-231). chain() builds a `;`-joined tmux command list, and this file
//     sends one command at a time; parse_fields splits the tab-separated reply
//     of a multi-field status query, which this file never asks tmux for. So
//     the copied range is 158-201 and 232-237, not the whole span.
// A shared header both tools include is the clean form and is named as a
// follow-up in the spec's flagged concerns; until then, a fix to either copy
// belongs in both.
//
// C++ conventions in this file (C++20, no external dependencies):
//   * Every OS resource is owned by an RAII type — Fd, RawMode. There is no bare
//     close()/tcsetattr on an error path, and no atexit.
//   * Reader parameters are std::string_view; only what outlives a call owns a
//     std::string.
//   * Fallible lookups return std::optional; there are no `bool* ok` out-params.
//   * The UTF-8 decoder is constexpr and carries its unit tests as
//     static_assert, so a byte/char/cell mix-up fails the build, not the popup.

#include <algorithm>
#include <array>
#include <cerrno>
#include <clocale>
#include <cstdio>
#include <format>
#include <iterator>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <poll.h>
#include <signal.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>
#include <wchar.h>
#include <wctype.h>

namespace {

// ── RAII: owned OS handles ─────────────────────────────────────────────────

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

// ── tmux plumbing ──────────────────────────────────────────────────────────
//
// The Rust crates get this from tmuxlib; there is no C++ tmuxlib, so these
// functions are the whole of what had to be re-implemented on this side.

// write() is allowed to write less than asked; on a pty under a full redraw it
// does. Discarding the return value discards exactly the information needed to
// notice.
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
    std::string             out;
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

// Every user-facing message is literal (-l): a pane title must never be parsed
// as a format string.
void say(const std::string& text) {
    tmux_run({"display-message", "-l", text});
}

// ── UTF-8 ──────────────────────────────────────────────────────────────────
//
// Two different "columns" exist and confusing them is the classic bug here
// (ADR-0004 was written about its sibling in seek):
//   * a std::string index is a BYTE offset;
//   * the prompt cursor is placed by CHARACTER;
//   * the screen is measured in CELLS (CJK = 2).
// Nothing below ever mixes them silently.

struct Decoded {
    char32_t cp  = 0;
    int      len = 1;   // >=1 always, so a malformed byte advances rather than looping
};

// Decode one code point at `i`. constexpr so the static_asserts below are the
// decoder's unit tests: a regression fails the build instead of the popup.
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

// Characters in s[0, byte_end) — what the prompt cursor counts.
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
static_assert(utf8_chars("中文 aa", 9) == 5);                // 中 文 SP a a

[[nodiscard]] int cell_width(char32_t cp) noexcept {
    // wcwidth takes wchar_t; on every platform this tool runs on that is UCS-4,
    // so the cast is a spelling change, not a narrowing one.
    static_assert(sizeof(wchar_t) >= sizeof(char32_t),
                  "wcwidth cannot represent a full code point on this platform");
    const int w = wcwidth(static_cast<wchar_t>(cp));
    return w < 0 ? 1 : w;   // render control/unknown as one cell rather than vanish
}

// Cells occupied by s[0, byte_end) — what the screen counts.
[[nodiscard]] int utf8_cells(std::string_view s, size_t byte_end) {
    int n = 0;
    for (size_t i = 0; i < byte_end && i < s.size();) {
        const auto d = utf8_decode(s, i);
        n += cell_width(d.cp);
        i += static_cast<size_t>(d.len);
    }
    return n;
}

// Inverse of utf8_cells: the byte length of the longest prefix of `s` that fits
// in `budget` cells. Same decoder and same width table, walked forwards until
// the budget runs out, so the cut always lands on a character boundary — a
// byte-boundary cut would emit half of a `✳`, and the cell budget is what keeps
// a wide glyph from straddling the popup's edge.
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

// ── rows ───────────────────────────────────────────────────────────────────

struct Row {
    std::string id;      // #{pane_id} — carried on the row, never displayed
    std::string where;   // session:window.pane
    std::string cmd;     // #{pane_current_command}
    std::string title;   // #{pane_title} — the only matched column (R2)
};

// One tmux call, argv vector, never a shell string. Scope is `-a`: every pane on
// the server, in every session, which is the scope the replaced binding had.
constexpr std::string_view kRowFormat =
    "#{pane_id}\t#{session_name}:#{window_index}.#{pane_index}\t"
    "#{pane_current_command}\t#{pane_title}";

// Split on the FIRST three tabs and give the remainder to the title: a title
// containing a tab then arrives whole instead of shifting the fields left. No
// writer in this repo emits one, but the failure mode of guessing the other way
// is a row whose id column holds someone else's text.
[[nodiscard]] std::optional<Row> parse_row(std::string_view line) {
    Row              r;
    std::string_view rest   = line;
    std::string*     dst[3] = {&r.id, &r.where, &r.cmd};
    for (auto* field : dst) {
        const size_t tab = rest.find('\t');
        if (tab == std::string_view::npos) return std::nullopt;   // short row: drop it
        field->assign(rest.substr(0, tab));
        rest.remove_prefix(tab + 1);
    }
    r.title.assign(rest);
    if (r.id.empty()) return std::nullopt;
    return r;
}

std::vector<Row> list_rows() {
    std::vector<Row> rows;
    const auto       blob = tmux_out({"list-panes", "-a", "-F", std::string(kRowFormat)});
    if (!blob) return rows;

    std::string_view rest{*blob};
    while (!rest.empty()) {
        const size_t     nl   = rest.find('\n');
        std::string_view line = rest.substr(0, nl);
        if (auto r = parse_row(line)) rows.push_back(std::move(*r));
        if (nl == std::string_view::npos) break;
        rest.remove_prefix(nl + 1);
    }
    return rows;
}

// ── matching ───────────────────────────────────────────────────────────────
//
// Case-insensitive substring on the TITLE column only (R2). The command and
// location columns are shown but not matched, so typing `bash` does not select
// every shell. The query is a plain string, not a regex: `.` and `*` are
// literal, which is what tmux's own `#{mi:*x*,…}` did before this tool.
//
// Both sides are decoded to code points and folded with towlower under the
// user's locale. ASCII behaves exactly as `#{mi:}` did; non-ASCII folding is
// best effort and is NOT claimed to match tmux's mi (Chinese has no case, so
// there the fold is the identity).

[[nodiscard]] std::u32string fold(std::string_view s) {
    std::u32string out;
    for (size_t i = 0; i < s.size();) {
        const auto d = utf8_decode(s, i);
        out.push_back(static_cast<char32_t>(towlower(static_cast<wint_t>(d.cp))));
        i += static_cast<size_t>(d.len);
    }
    return out;
}

// An empty needle matches every row: the list opens complete (R1) and narrows
// as characters arrive.
[[nodiscard]] bool title_matches(const Row& r, const std::u32string& needle) {
    return fold(r.title).find(needle) != std::u32string::npos;
}

// ── terminal ───────────────────────────────────────────────────────────────

// The only global left. A signal handler may touch nothing else.
volatile sig_atomic_t g_resized = 0;

void on_winch(int) { g_resized = 1; }

// Raw mode and the alternate screen, released by the destructor. restore() is
// idempotent and public because Enter's path must return the terminal BEFORE
// the client is switched to another pane, not at scope exit.
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
    None, Enter, Esc, Backspace, Up, Down, PgUp, PgDn, Home, End, KillWord, KillLine, Text
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
        if (c1 != '[' && c1 != 'O') return key_only(Key::None);
        const int c2 = read_byte(40);
        if (c2 < 0) return key_only(Key::Esc);
        switch (c2) {
            case 'A': return key_only(Key::Up);
            case 'B': return key_only(Key::Down);
            case 'H': return key_only(Key::Home);
            case 'F': return key_only(Key::End);
            // Numeric CSI: ESC [ <n> ~. The `~` is consumed here rather than
            // left in the input buffer — sift leaves it and it lands in the
            // query as a stray character (runbooks/sift.md, "Home / End do
            // nothing"). tmux sends 1~ for Home and 4~ for End.
            case '1': case '4': case '5': case '6': {
                read_byte(40);                                 // consume the '~'
                if (c2 == '1') return key_only(Key::Home);
                if (c2 == '4') return key_only(Key::End);
                return key_only(c2 == '5' ? Key::PgUp : Key::PgDn);
            }
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

// Named once so a stray digit in an escape sequence is a compile error at this
// definition rather than a rendering fault visible only when the popup runs.
constexpr std::string_view kHomeClear = "\x1b[H\x1b[2J";
constexpr std::string_view kBold      = "\x1b[1m";
constexpr std::string_view kDim       = "\x1b[2m";
constexpr std::string_view kReset     = "\x1b[0m";
constexpr std::string_view kInvertOn  = "\x1b[7m";
constexpr std::string_view kInvertOff = "\x1b[27m";

// ── the popup ──────────────────────────────────────────────────────────────

struct Ui {
    std::vector<Row>    rows;
    std::string         query;
    std::vector<size_t> shown;      // indices into rows, in list-panes order
    size_t              sel = 0;
    size_t              top = 0;    // first visible row
    std::string         notice;     // one keypress long; R4's message lives here
};

// The selection resets to the first row on every query change (R3): a fragment
// that narrows the list to one pane then needs no Down before Enter.
void refilter(Ui& u) {
    const std::u32string needle = fold(u.query);
    u.shown.clear();
    for (size_t i = 0; i < u.rows.size(); ++i)
        if (title_matches(u.rows[i], needle)) u.shown.push_back(i);
    u.sel = 0;
    u.top = 0;
}

// Pad `s` to `width` cells, cutting at a cell boundary if it is longer, so the
// next column starts in the same screen cell on every row.
[[nodiscard]] std::string column(std::string_view s, int width) {
    if (width <= 0) return {};
    std::string o(s.substr(0, utf8_fit(s, width)));
    const int   used = utf8_cells(o, o.size());
    if (used < width) o.append(static_cast<size_t>(width - used), ' ');
    return o;
}

// The command column is a fixed 10 cells: it is context, not the thing being
// read, and a width derived from the rows would make the title column jitter as
// the filter changes which commands are on screen.
constexpr int kCmdWidth = 10;

// Rows of the popup that are not pane rows: the header and the key-hint footer.
// draw() and the PgUp/PgDn step must agree on this number, so it is named once.
constexpr int kChromeRows = 2;

void draw(Ui& u) {
    const auto [w, h] = term_size();
    if (h < 4 || w < 20) return;

    const int list_rows = std::max(h - kChromeRows, 1);

    // Keep the selection in view.
    if (u.sel < u.top) u.top = u.sel;
    if (u.sel >= u.top + static_cast<size_t>(list_rows))
        u.top = u.sel - static_cast<size_t>(list_rows) + 1;
    if (u.shown.size() <= static_cast<size_t>(list_rows)) u.top = 0;

    std::string o{kHomeClear};

    // Header: prompt on the left, status on the right. The notice outranks the
    // count for exactly one keypress — it is the only report R4 gets, and the
    // popup stays open behind it.
    const std::string status =
        u.notice.empty() ? std::format("{}/{}", u.shown.size(), u.rows.size()) : u.notice;

    constexpr std::string_view kPrompt = "pane name: ";
    const int plen = static_cast<int>(kPrompt.size()) + utf8_chars(u.query, u.query.size());
    const int slen = utf8_cells(status, status.size());
    o += std::format("{}{}{}{}", kBold, kPrompt, u.query, kReset);
    if (const int gap = w - plen - slen; gap > 0) {
        o.append(static_cast<size_t>(gap), ' ');
        o += std::format("{}{}{}", kDim, status, kReset);
    }
    o += "\r\n";

    // The location column is as wide as the widest location on the SERVER, not
    // the widest currently shown: filtering must not shift the title column
    // sideways while the user is reading it.
    int wherew = 0;
    for (const Row& r : u.rows) wherew = std::max(wherew, utf8_cells(r.where, r.where.size()));

    // Row layout, and the whole of the width budget:
    //   "> " or "  "   2
    //   location       wherew
    //   space          1
    //   command        kCmdWidth
    //   space          1
    //   title          the rest
    const int titlew = w - wherew - kCmdWidth - 4;

    for (int r = 0; r < list_rows; ++r) {
        const size_t vis = u.top + static_cast<size_t>(r);
        if (vis >= u.shown.size()) { o += "\r\n"; continue; }
        const Row& row = u.rows[u.shown[vis]];

        const bool selected = (vis == u.sel);
        if (selected) o += kInvertOn;
        o += selected ? "> " : "  ";
        // Location and command are context: dimmed, so the eye lands on the
        // titles, which are the column being typed against. kReset ends the dim
        // and the invert together, so the invert is re-armed after it.
        o += std::format("{}{} {}{}", kDim, column(row.where, wherew),
                         column(row.cmd, kCmdWidth), kReset);
        if (selected) o += kInvertOn;
        o += " ";
        o += column(row.title, titlew);
        if (selected) o += kInvertOff;
        o += kReset;
        o += "\r\n";
    }

    // Cut in cells, not bytes (utf8_fit): draw() emits exactly h lines, so a
    // footer one cell too wide wraps, the pane scrolls, and the header — the
    // only place the query and the R4 notice are visible — leaves the screen.
    constexpr std::string_view kFooter =
        "type to filter titles  ↑↓ select  Enter jump  Esc cancel  C-w word  C-u clear";
    o += std::format("{}{}{}", kDim, kFooter.substr(0, utf8_fit(kFooter, w)), kReset);

    // Park the real cursor at the end of the query so typing looks normal.
    o += std::format("\x1b[1;{}H\x1b[?25h", plen + 1);
    write_all(STDOUT_FILENO, o);
}

// ── the jump ───────────────────────────────────────────────────────────────
//
// Measured on tmux 3.5a (runbooks/switch-pane-by-name.md): `switch-client -t
// %0` changes session, window AND pane in one call, so a pane id is a complete
// target and needs no window- or pane-selection follow-up. That is what keeps
// switch-client the only tmux command in this file that writes anything (R6);
// cc-fleet's focus_pane needs three calls because it starts from a session name.
//
// The guard is the point: `switch-client -t ""` exits 0 and moves nothing, so an
// empty id would be a silent no-op. It is refused here and reported instead, on
// stderr for the `jump` seam and on the status line for a human.
bool jump(const std::string& id) {
    if (id.empty()) {
        fprintf(stderr, "pane-jump: no pane selected\n");
        say("pane-jump: no pane selected");
        return false;
    }
    return tmux_run({"switch-client", "-t", id});
}

int run_ui() {
    Ui u;
    u.rows = list_rows();
    if (u.rows.empty()) { say("pane-jump: no panes"); return 0; }
    refilter(u);

    RawMode term;
    if (!term.engage()) { say("pane-jump: no terminal (run it from a tmux popup)"); return 0; }
    signal(SIGWINCH, on_winch);

    draw(u);
    for (;;) {
        const Input in = read_key();
        if (g_resized) { g_resized = 0; draw(u); }

        // A notice lives for exactly one keypress: it reports what the LAST key
        // did, and the next key is about to make it stale.
        if (in.key != Key::None) u.notice.clear();

        switch (in.key) {
            case Key::Esc:
                return 0;                     // cancel: silent, nothing moved (R5)
                                              // (~RawMode restores the terminal)

            case Key::Enter: {
                if (u.shown.empty()) {
                    // Fail closed (R4): no full list, no silent success, and the
                    // popup stays open so the query can be corrected.
                    u.notice = "pane-jump: no pane title matches";
                    break;
                }
                const std::string id = u.rows[u.shown[u.sel]].id;
                term.restore();               // before the client moves elsewhere
                // A failed switch-client would otherwise be a silent no-op: the
                // popup is already gone and the client is still where it was.
                if (!jump(id)) say(std::format("pane-jump: switch-client failed for {}", id));
                return 0;
            }

            case Key::Up:   if (u.sel > 0) --u.sel; break;
            case Key::Down: if (u.sel + 1 < u.shown.size()) ++u.sel; break;
            case Key::Home: u.sel = 0; break;
            case Key::End:  if (!u.shown.empty()) u.sel = u.shown.size() - 1; break;

            case Key::PgUp: case Key::PgDn: {
                const auto sz = term_size();
                // One page is the visible pane rows (h - kChromeRows) less one
                // row kept on screen as context across the jump.
                const size_t step =
                    static_cast<size_t>(sz.h > 4 ? sz.h - kChromeRows - 1 : 1);
                if (in.key == Key::PgUp) u.sel = (u.sel > step) ? u.sel - step : 0;
                else if (!u.shown.empty()) u.sel = std::min(u.sel + step, u.shown.size() - 1);
                break;
            }

            case Key::Backspace: {
                if (u.query.empty()) break;
                size_t i = u.query.size();
                while (i > 0 && (static_cast<unsigned char>(u.query[i - 1]) & 0xC0) == 0x80) --i;
                if (i > 0) --i;
                u.query.resize(i);
                refilter(u);
                break;
            }

            case Key::KillWord: {
                size_t i = u.query.size();
                while (i > 0 && u.query[i - 1] == ' ') --i;
                while (i > 0 && u.query[i - 1] != ' ') --i;
                u.query.resize(i);
                refilter(u);
                break;
            }

            case Key::KillLine:
                u.query.clear();
                refilter(u);
                break;

            case Key::Text:
                u.query += in.text;
                refilter(u);
                break;

            case Key::None:
                break;
        }
        draw(u);
    }
}

// ── headless seams ─────────────────────────────────────────────────────────

int run_rows(std::string_view fragment) {
    Ui u;
    u.rows  = list_rows();
    u.query = std::string(fragment);
    refilter(u);

    // One buffer, one write: a syscall per row would measure the pipe rather
    // than the tool.
    std::string out;
    for (const size_t i : u.shown) {
        const Row& r = u.rows[i];
        std::format_to(std::back_inserter(out), "{}\t{}\t{}\t{}\n", r.id, r.where, r.cmd, r.title);
    }
    write_all(STDOUT_FILENO, out);
    return 0;
}

int run_jump(const char* id) {
    const std::string target = id ? std::string(id) : std::string();
    // jump() reports the empty-target case itself; anything else that returns
    // false is a switch-client that failed, and must not pass as a no-op.
    if (!jump(target) && !target.empty()) {
        fprintf(stderr, "pane-jump: switch-client failed for %s\n", target.c_str());
        say(std::format("pane-jump: switch-client failed for {}", target));
    }
    return 0;                                  // always exit 0 — see invariants
}

}  // namespace

int main(int argc, char** argv) {
    setlocale(LC_ALL, "");

    if (argc >= 2) {
        const std::string_view verb{argv[1]};
        if (verb == "rows") return run_rows(argc >= 3 ? argv[2] : "");
        if (verb == "jump") return run_jump(argc >= 3 ? argv[2] : nullptr);
        fprintf(stderr,
                "usage: pane-jump | pane-jump rows [fragment] | pane-jump jump <pane-id>\n");
        return 0;                              // always exit 0 — see invariants
    }

    return run_ui();
}
