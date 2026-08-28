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

#include <algorithm>
#include <cerrno>
#include <clocale>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
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

// ── tmux plumbing ──────────────────────────────────────────────────────────
//
// The Rust crates get this from tmuxlib; there is no C++ tmuxlib, so these two
// functions are the whole of what had to be re-implemented on this side.

// Run tmux with `args` (argv[0] is supplied here) and return its stdout.
// `ok` reports whether it exited 0. stderr is left attached to ours so a real
// tmux error is still visible when debugging by hand.
std::string tmux_out(const std::vector<std::string>& args, bool* ok = nullptr) {
    if (ok) *ok = false;
    int fds[2];
    if (pipe(fds) != 0) return {};

    pid_t pid = fork();
    if (pid < 0) { close(fds[0]); close(fds[1]); return {}; }

    if (pid == 0) {
        close(fds[0]);
        if (dup2(fds[1], STDOUT_FILENO) < 0) _exit(127);
        close(fds[1]);
        std::vector<char*> argv;
        argv.reserve(args.size() + 2);
        argv.push_back(const_cast<char*>("tmux"));
        for (const auto& a : args) argv.push_back(const_cast<char*>(a.c_str()));
        argv.push_back(nullptr);
        execvp("tmux", argv.data());
        _exit(127);
    }

    close(fds[1]);
    std::string out;
    char buf[65536];
    ssize_t n;
    while ((n = read(fds[0], buf, sizeof buf)) > 0) out.append(buf, static_cast<size_t>(n));
    close(fds[0]);

    int status = 0;
    while (waitpid(pid, &status, 0) < 0 && errno == EINTR) {}
    if (ok) *ok = WIFEXITED(status) && WEXITSTATUS(status) == 0;
    return out;
}

bool tmux_run(const std::vector<std::string>& args) {
    bool ok = false;
    tmux_out(args, &ok);
    return ok;
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

// Decode one code point at `i`; returns its byte length (>=1 always, so a
// malformed byte advances rather than looping forever).
int utf8_decode(const std::string& s, size_t i, wchar_t* cp) {
    unsigned char c = static_cast<unsigned char>(s[i]);
    size_t avail = s.size() - i;
    auto cont = [&](size_t k) {
        return k < avail && (static_cast<unsigned char>(s[i + k]) & 0xC0) == 0x80;
    };
    if (c < 0x80) { *cp = c; return 1; }
    if ((c & 0xE0) == 0xC0 && cont(1)) {
        *cp = static_cast<wchar_t>(((c & 0x1F) << 6) | (s[i + 1] & 0x3F));
        return 2;
    }
    if ((c & 0xF0) == 0xE0 && cont(1) && cont(2)) {
        *cp = static_cast<wchar_t>(((c & 0x0F) << 12) | ((s[i + 1] & 0x3F) << 6) | (s[i + 2] & 0x3F));
        return 3;
    }
    if ((c & 0xF8) == 0xF0 && cont(1) && cont(2) && cont(3)) {
        *cp = static_cast<wchar_t>(((c & 0x07) << 18) | ((s[i + 1] & 0x3F) << 12) |
                                   ((s[i + 2] & 0x3F) << 6) | (s[i + 3] & 0x3F));
        return 4;
    }
    *cp = 0xFFFD;
    return 1;
}

// Characters in s[0, byte_end) — this is what `cursor-right -N` counts.
int utf8_chars(const std::string& s, size_t byte_end) {
    int n = 0;
    for (size_t i = 0; i < byte_end && i < s.size();) {
        wchar_t cp;
        i += static_cast<size_t>(utf8_decode(s, i, &cp));
        ++n;
    }
    return n;
}

// Cells occupied by s[0, byte_end) — what #{copy_cursor_x} counts.
int utf8_cells(const std::string& s, size_t byte_end);

int cell_width(wchar_t cp) {
    int w = wcwidth(cp);
    return w < 0 ? 1 : w;   // render control/unknown as one cell rather than vanish
}

int utf8_cells(const std::string& s, size_t byte_end) {
    int n = 0;
    for (size_t i = 0; i < byte_end && i < s.size();) {
        wchar_t cp;
        i += static_cast<size_t>(utf8_decode(s, i, &cp));
        n += cell_width(cp);
    }
    return n;
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
    if (arg && *arg && strncmp(arg, "#{", 2) != 0) return arg;
    bool ok = false;
    std::string s = tmux_out({"display-message", "-p", "#{pane_id}"}, &ok);
    while (!s.empty() && (s.back() == '\n' || s.back() == '\r')) s.pop_back();
    return ok ? s : std::string();
}

// ── pane inspection ────────────────────────────────────────────────────────

struct Geom {
    long history_size = 0;
    long height       = 0;
    bool alternate    = false;
    bool ok           = false;
};

Geom pane_geom(const std::string& pane) {
    Geom g;
    bool ok = false;
    std::string s = tmux_out({"display-message", "-p", "-t", pane,
                              "#{history_size}\t#{pane_height}\t#{alternate_on}"}, &ok);
    if (!ok) return g;
    long v[3] = {0, 0, 0};
    size_t start = 0;
    for (int i = 0; i < 3; ++i) {
        size_t tab = s.find('\t', start);
        std::string f = s.substr(start, tab == std::string::npos ? std::string::npos : tab - start);
        v[i] = strtol(f.c_str(), nullptr, 10);
        if (tab == std::string::npos) break;
        start = tab + 1;
    }
    g.history_size = v[0];
    g.height       = v[1];
    g.alternate    = v[2] != 0;
    g.ok           = true;
    return g;
}

// Capture history + visible screen. Deliberately no -J: joining wrapped lines
// would desynchronise our indices from the physical lines copy-mode navigates.
// No -e either: escape sequences would be matched by the regex and rendered raw.
// Index i in the result is physical line i counting from the top of history —
// the coordinate the jump arithmetic below is written in.
std::vector<std::string> capture(const std::string& pane, const Geom& g) {
    bool ok = false;
    std::string blob = tmux_out({"capture-pane", "-p", "-t", pane,
                                 "-S", "-" + std::to_string(g.history_size),
                                 "-E", std::to_string(g.height - 1)}, &ok);
    std::vector<std::string> lines;
    if (!ok) return lines;
    size_t start = 0;
    while (start <= blob.size()) {
        size_t nl = blob.find('\n', start);
        if (nl == std::string::npos) {
            if (start < blob.size()) lines.push_back(blob.substr(start));
            break;
        }
        lines.push_back(blob.substr(start, nl - start));
        start = nl + 1;
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
    long line;        // index into the capture
    int  char_start;  // characters before the match — feeds `cursor-right -N`
    int  char_end;    // characters before the match END — the cursor seat
    int  cell_start;  // CELLS before the match — what #{copy_cursor_x} reports
    int  byte_start;
    int  byte_end;
};

// Every occurrence, in scrollback order. A line with three hits yields three
// entries: the user picks an occurrence, not a line, and the jump is exact.
std::vector<Hit> find_all(const std::vector<std::string>& lines, const regex_t* re, size_t cap) {
    std::vector<Hit> hits;
    for (size_t i = 0; i < lines.size(); ++i) {
        const std::string& s = lines[i];
        if (s.empty()) continue;
        size_t off = 0;
        while (off <= s.size()) {
            regmatch_t m;
            int flags = off == 0 ? 0 : REG_NOTBOL;
            if (regexec(re, s.c_str() + off, 1, &m, flags) != 0) break;
            size_t b = off + static_cast<size_t>(m.rm_so);
            size_t e = off + static_cast<size_t>(m.rm_eo);
            Hit h;
            h.line       = static_cast<long>(i);
            h.byte_start = static_cast<int>(b);
            h.byte_end   = static_cast<int>(e);
            h.char_start = utf8_chars(s, b);
            h.char_end   = utf8_chars(s, e);
            h.cell_start = utf8_cells(s, b);
            hits.push_back(h);
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
    Geom now = pane_geom(pane);
    if (!now.ok) return false;

    std::vector<std::string> a = {"copy-mode", "-t", pane,
                                  ";", "send-keys", "-X", "-t", pane, "history-top"};

    if (line <= now.history_size) {
        a.insert(a.end(), {";", "send-keys", "-X", "-t", pane,
                           "goto-line", std::to_string(now.history_size - line)});
    } else {
        // Target is in the visible screen: pin the viewport to the bottom and
        // step down to the row.
        long down = line - now.history_size;
        a.insert(a.end(), {";", "send-keys", "-X", "-t", pane, "goto-line", "0"});
        if (down > 0)
            a.insert(a.end(), {";", "send-keys", "-X", "-N", std::to_string(down),
                               "-t", pane, "cursor-down"});
    }

    if (char_end > 0)
        a.insert(a.end(), {";", "send-keys", "-X", "-N", std::to_string(char_end),
                           "-t", pane, "cursor-right"});

    // Registering the pattern with tmux is the point of this step, not just the
    // positioning: it is what makes the match highlight, `n`/`N`, and seek's
    // grab keys work afterwards.
    a.insert(a.end(), {";", "send-keys", "-X", "-t", pane, "search-backward", pattern});

    if (!tmux_run(a)) return false;

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
    bool ok = false;
    std::string s = tmux_out({"display-message", "-p", "-t", pane,
                              "#{history_size}\t#{scroll_position}\t#{copy_cursor_y}"
                              "\t#{copy_cursor_x}\t#{search_present}"}, &ok);
    if (!ok) return false;

    long f[5] = {0, 0, 0, 0, 0};
    size_t start = 0;
    for (int i = 0; i < 5; ++i) {
        size_t tab = s.find('\t', start);
        f[i] = strtol(s.substr(start, tab == std::string::npos ? std::string::npos
                                                              : tab - start).c_str(), nullptr, 10);
        if (tab == std::string::npos) break;
        start = tab + 1;
    }
    long landed = f[0] - f[1] + f[2];
    return f[4] == 1 && landed == line && f[3] == cell_start;
}

// ── terminal ───────────────────────────────────────────────────────────────

termios g_saved;
bool    g_raw = false;
volatile sig_atomic_t g_resized = 0;

void on_winch(int) { g_resized = 1; }

void cooked() {
    if (!g_raw) return;
    tcsetattr(STDIN_FILENO, TCSAFLUSH, &g_saved);
    g_raw = false;
    // Leave the alternate screen and re-show the cursor.
    const char* s = "\x1b[?25h\x1b[?1049l";
    ssize_t r = write(STDOUT_FILENO, s, strlen(s));
    (void)r;
}

bool raw() {
    if (tcgetattr(STDIN_FILENO, &g_saved) != 0) return false;
    termios t = g_saved;
    t.c_lflag &= ~static_cast<tcflag_t>(ECHO | ICANON | ISIG | IEXTEN);
    t.c_iflag &= ~static_cast<tcflag_t>(IXON | ICRNL | INLCR | BRKINT | ISTRIP);
    t.c_oflag &= ~static_cast<tcflag_t>(OPOST);
    t.c_cc[VMIN]  = 1;
    t.c_cc[VTIME] = 0;
    if (tcsetattr(STDIN_FILENO, TCSAFLUSH, &t) != 0) return false;
    g_raw = true;
    const char* s = "\x1b[?1049h\x1b[2J";
    ssize_t r = write(STDOUT_FILENO, s, strlen(s));
    (void)r;
    return true;
}

void term_size(int* w, int* h) {
    winsize ws{};
    if (ioctl(STDOUT_FILENO, TIOCGWINSZ, &ws) == 0 && ws.ws_col > 0 && ws.ws_row > 0) {
        *w = ws.ws_col;
        *h = ws.ws_row;
    } else {
        *w = 80;
        *h = 24;
    }
}

// ── key decoding ───────────────────────────────────────────────────────────

enum Key {
    K_NONE = 0, K_ENTER, K_ESC, K_BACKSPACE, K_UP, K_DOWN,
    K_PGUP, K_PGDN, K_HOME, K_END, K_KILL_WORD, K_KILL_LINE, K_TEXT
};

struct Input {
    Key         key = K_NONE;
    std::string text;   // for K_TEXT: one whole UTF-8 character
};

// Read with a timeout; -1 on timeout/error.
int read_byte(int timeout_ms) {
    pollfd p{STDIN_FILENO, POLLIN, 0};
    int n = poll(&p, 1, timeout_ms);
    if (n <= 0) return -1;
    unsigned char c;
    ssize_t r = read(STDIN_FILENO, &c, 1);
    if (r != 1) return -1;
    return c;
}

Input read_key() {
    Input in;
    int c = read_byte(-1);
    if (c < 0) { in.key = K_ESC; return in; }

    switch (c) {
        case '\r': case '\n': in.key = K_ENTER;      return in;
        case 127:  case 8:    in.key = K_BACKSPACE;  return in;
        case 23:              in.key = K_KILL_WORD;  return in;  // C-w
        case 21:              in.key = K_KILL_LINE;  return in;  // C-u
        case 3:   case 7:     in.key = K_ESC;        return in;  // C-c / C-g
        case 16:              in.key = K_UP;         return in;  // C-p
        case 14:              in.key = K_DOWN;       return in;  // C-n
        default: break;
    }

    if (c == 0x1b) {
        // A bare Escape is cancel; an escape SEQUENCE is a cursor key. The only
        // thing separating them is timing, so give the rest of the sequence a
        // brief window to arrive.
        int c1 = read_byte(40);
        if (c1 < 0) { in.key = K_ESC; return in; }
        if (c1 == '[' || c1 == 'O') {
            int c2 = read_byte(40);
            if (c2 < 0) { in.key = K_ESC; return in; }
            switch (c2) {
                case 'A': in.key = K_UP;   return in;
                case 'B': in.key = K_DOWN; return in;
                case 'H': in.key = K_HOME; return in;
                case 'F': in.key = K_END;  return in;
                case '5': case '6': {
                    int c3 = read_byte(40);          // consume the '~'
                    (void)c3;
                    in.key = (c2 == '5') ? K_PGUP : K_PGDN;
                    return in;
                }
                default: in.key = K_NONE; return in;   // unknown CSI: ignore
            }
        }
        in.key = K_NONE;
        return in;
    }

    if (c < 32) { in.key = K_NONE; return in; }   // other control bytes: ignore

    // A printable byte, possibly the head of a multi-byte character.
    in.key = K_TEXT;
    in.text.push_back(static_cast<char>(c));
    int extra = 0;
    if ((c & 0xE0) == 0xC0) extra = 1;
    else if ((c & 0xF0) == 0xE0) extra = 2;
    else if ((c & 0xF8) == 0xF0) extra = 3;
    for (int i = 0; i < extra; ++i) {
        int n = read_byte(40);
        if (n < 0) break;
        in.text.push_back(static_cast<char>(n));
    }
    return in;
}

// ── rendering ──────────────────────────────────────────────────────────────

void out(const std::string& s) {
    ssize_t r = write(STDOUT_FILENO, s.data(), s.size());
    (void)r;
}

// Render one capture line into `width` cells, guaranteeing the match is on
// screen (long lines scroll horizontally so a hit at column 400 is still seen)
// and reverse-videoing the matched span.
std::string render_line(const std::string& s, int byte_start, int byte_end, int width) {
    if (width <= 0) return {};

    // Cell offset of the match start, and of every byte we may need to cut at.
    int cells_to_start = 0;
    for (size_t i = 0; i < s.size() && static_cast<int>(i) < byte_start;) {
        wchar_t cp;
        int len = utf8_decode(s, i, &cp);
        cells_to_start += cell_width(cp);
        i += static_cast<size_t>(len);
    }

    // Scroll so the match sits about a third in, but never past the line start.
    int skip_cells = 0;
    if (cells_to_start > width - 12) skip_cells = cells_to_start - width / 3;
    if (skip_cells < 0) skip_cells = 0;

    std::string o;
    int cells = 0, seen = 0;
    bool inverted = false;
    if (skip_cells > 0) { o += "…"; cells = 1; }

    for (size_t i = 0; i < s.size();) {
        wchar_t cp;
        int len = utf8_decode(s, i, &cp);
        int w   = cell_width(cp);
        if (seen + w > skip_cells) {
            if (cells + w > width) break;
            bool want = (static_cast<int>(i) >= byte_start && static_cast<int>(i) < byte_end);
            if (want && !inverted) { o += "\x1b[7m"; inverted = true; }
            else if (!want && inverted) { o += "\x1b[27m"; inverted = false; }
            o.append(s, i, static_cast<size_t>(len));
            cells += w;
        }
        seen += w;
        i += static_cast<size_t>(len);
    }
    if (inverted) o += "\x1b[27m";
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
    size_t                   sel      = 0;
    size_t                   top      = 0;   // first visible hit
    bool                     bad_re   = false;
    bool                     capped   = false;
    std::string              re_error;
};

void refilter(Ui& u) {
    u.hits.clear();
    u.capped = false;
    u.bad_re = false;
    u.re_error.clear();
    if (u.pattern.empty()) { u.sel = 0; u.top = 0; return; }

    regex_t re;
    int rc = regcomp(&re, u.pattern.c_str(), REG_EXTENDED);
    if (rc != 0) {
        // Half-typed patterns are invalid most of the time. Keeping the previous
        // result set on screen would be a lie about what the pattern matches, so
        // the list empties but the header says why.
        char buf[128];
        regerror(rc, &re, buf, sizeof buf);
        u.bad_re   = true;
        u.re_error = buf;
        regfree(&re);
        return;
    }
    u.hits = find_all(u.lines, &re, kMatchCap);
    u.capped = u.hits.size() >= kMatchCap;
    regfree(&re);

    // Default to the match nearest the bottom — the same "most recent first"
    // bias as the search-backward binding this replaces.
    u.sel = u.hits.empty() ? 0 : u.hits.size() - 1;
    u.top = 0;
}

void draw(Ui& u) {
    int w, h;
    term_size(&w, &h);
    if (h < 4 || w < 20) return;

    int list_rows = h - 2;                       // header + footer
    if (list_rows < 1) list_rows = 1;

    // Keep the selection in view.
    if (u.sel < u.top) u.top = u.sel;
    if (u.sel >= u.top + static_cast<size_t>(list_rows))
        u.top = u.sel - static_cast<size_t>(list_rows) + 1;
    if (u.hits.size() <= static_cast<size_t>(list_rows)) u.top = 0;

    std::string o = "\x1b[H\x1b[2J";

    // Header: prompt on the left, status on the right.
    std::string status;
    if (u.bad_re)              status = "invalid regex: " + u.re_error;
    else if (u.pattern.empty())status = "type an extended regex";
    else if (u.hits.empty())   status = "no match";
    else                       status = std::to_string(u.hits.size()) +
                                        (u.capped ? "+ matches (capped)" : " matches");
    if (u.geom.alternate) status = "⚠ visible screen only · " + status;

    std::string prompt = "regex> " + u.pattern;
    int plen = 7 + utf8_chars(u.pattern, u.pattern.size());
    int slen = utf8_chars(status, status.size());
    o += "\x1b[1m" + prompt + "\x1b[0m";
    int gap = w - plen - slen;
    if (gap > 0) { o.append(static_cast<size_t>(gap), ' '); o += "\x1b[2m" + status + "\x1b[0m"; }
    o += "\r\n";

    // Widest line number in view, so the text column does not jitter.
    int numw = 1;
    for (long v = u.lines.empty() ? 0 : static_cast<long>(u.lines.size()) - 1; v >= 10; v /= 10) ++numw;

    for (int r = 0; r < list_rows; ++r) {
        size_t idx = u.top + static_cast<size_t>(r);
        if (idx >= u.hits.size()) { o += "\r\n"; continue; }
        const Hit& hit = u.hits[idx];
        bool cur = (idx == u.sel);

        char num[32];
        snprintf(num, sizeof num, "%*ld", numw, hit.line);
        o += cur ? "\x1b[1m> " : "  ";
        o += "\x1b[2m";
        o += num;
        o += "\x1b[22m ";
        int text_w = w - numw - 3;
        o += render_line(u.lines[static_cast<size_t>(hit.line)], hit.byte_start, hit.byte_end, text_w);
        o += "\x1b[0m\r\n";
    }

    o += "\x1b[2m↑↓ select  Enter jump  Esc cancel  C-w word  C-u clear\x1b[0m";

    // Park the real cursor at the end of the pattern so typing looks normal.
    o += "\x1b[1;" + std::to_string(plen + 1) + "H\x1b[?25h";
    out(o);
}

int run_ui(const std::string& pane) {
    Ui u;
    u.pane = pane;
    u.geom = pane_geom(pane);
    if (!u.geom.ok) { say("sift: cannot read pane " + pane); return 0; }
    u.lines = capture(pane, u.geom);
    if (u.lines.empty()) { say("sift: nothing to search in " + pane); return 0; }

    if (!raw()) { say("sift: no terminal (run it from a tmux popup)"); return 0; }
    atexit(cooked);
    signal(SIGWINCH, on_winch);

    draw(u);
    for (;;) {
        Input in = read_key();
        if (g_resized) { g_resized = 0; draw(u); }

        switch (in.key) {
            case K_ESC:
                cooked();
                return 0;                                  // cancel: pane untouched

            case K_ENTER: {
                if (u.hits.empty()) break;
                const Hit& hit = u.hits[u.sel];
                std::string pattern = u.pattern;
                long line  = hit.line;
                int  cend  = hit.char_end;
                int  cstart = hit.cell_start;
                cooked();                                  // restore before the pane redraws
                if (!jump(pane, pattern, line, cend, cstart))
                    say("sift: the pane moved — landed on the nearest match of /" + pattern + "/");
                return 0;
            }

            case K_UP:   if (u.sel > 0) --u.sel; break;
            case K_DOWN: if (u.sel + 1 < u.hits.size()) ++u.sel; break;
            case K_HOME: u.sel = 0; break;
            case K_END:  if (!u.hits.empty()) u.sel = u.hits.size() - 1; break;

            case K_PGUP: case K_PGDN: {
                int w, h;
                term_size(&w, &h);
                size_t step = static_cast<size_t>(h > 4 ? h - 3 : 1);
                if (in.key == K_PGUP) u.sel = (u.sel > step) ? u.sel - step : 0;
                else if (!u.hits.empty()) u.sel = std::min(u.sel + step, u.hits.size() - 1);
                break;
            }

            case K_BACKSPACE: {
                if (u.pattern.empty()) break;
                size_t i = u.pattern.size();
                while (i > 0 && (static_cast<unsigned char>(u.pattern[i - 1]) & 0xC0) == 0x80) --i;
                if (i > 0) --i;
                u.pattern.resize(i);
                refilter(u);
                break;
            }

            case K_KILL_WORD: {
                size_t i = u.pattern.size();
                while (i > 0 && u.pattern[i - 1] == ' ') --i;
                while (i > 0 && u.pattern[i - 1] != ' ') --i;
                u.pattern.resize(i);
                refilter(u);
                break;
            }

            case K_KILL_LINE:
                u.pattern.clear();
                refilter(u);
                break;

            case K_TEXT:
                u.pattern += in.text;
                refilter(u);
                break;

            default:
                break;
        }
        draw(u);
    }
}

// ── headless seam ──────────────────────────────────────────────────────────

int run_rows(const std::string& pane, const std::string& pattern) {
    Geom g = pane_geom(pane);
    if (!g.ok) return 0;
    std::vector<std::string> lines = capture(pane, g);

    regex_t re;
    int rc = regcomp(&re, pattern.c_str(), REG_EXTENDED);
    if (rc != 0) {
        char buf[128];
        regerror(rc, &re, buf, sizeof buf);
        fprintf(stderr, "sift: invalid regex: %s\n", buf);
        regfree(&re);
        return 0;                                  // still exit 0 — see invariants
    }
    std::vector<Hit> hits = find_all(lines, &re, kMatchCap);
    regfree(&re);

    for (const Hit& h : hits)
        printf("%ld\t%d\t%d\t%d\t%s\n", h.line, h.char_start, h.char_end, h.cell_start,
               lines[static_cast<size_t>(h.line)].c_str());
    return 0;
}

}  // namespace

int main(int argc, char** argv) {
    setlocale(LC_ALL, "");

    if (argc >= 2 && strcmp(argv[1], "rows") == 0) {
        if (argc < 4) {
            fprintf(stderr, "usage: sift rows <pane-id> <regex>\n");
            return 0;
        }
        return run_rows(origin_pane(argv[2]), argv[3]);
    }

    std::string pane = origin_pane(argc >= 2 ? argv[1] : nullptr);
    if (pane.empty()) {
        fprintf(stderr, "sift: no pane — run it inside tmux, or pass a pane id\n");
        return 0;
    }
    return run_ui(pane);
}
