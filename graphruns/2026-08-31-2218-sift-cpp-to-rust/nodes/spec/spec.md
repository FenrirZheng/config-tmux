# `sift` — behavioural specification

Reimplementation target: `sift`, the tmux popup regex-search tool bound to `prefix /`.

This document is written so a Rust implementation can be built **without reading the C++
source**, and so a third party can verify that implementation **against this document
alone**. The authority for every statement here is
`/home/fenrir/.tmux/tools/sift/src/main.cpp` (800 lines) plus behaviour measured on this
machine on 2026-08-31 (tmux 3.5a, glibc 2.41, `LC_CTYPE=en_US.UTF-8`).

Provenance labels used below:

- **[src]** — read directly out of the C++ source.
- **[meas]** — measured on 2026-08-31 against the shipped binary
  (`tools/target/release/sift`, built 2026-08-28) driving throwaway tmux servers, or
  against glibc's `regcomp`/`regexec` called directly through `ctypes`.
- **[doc≠src]** — the runbook / ADR / atlas says one thing and the program does another;
  every one of these is repeated in §8.

Build contract (unchanged by the port unless the caller says otherwise): the binary must
land at `tools/target/release/sift`, because `claude.conf` binds `prefix /` straight at
that path. The C++ build is cmake (`cmake -S sift -B target/cmake-build
-DCMAKE_BUILD_TYPE=Release && cmake --build target/cmake-build`), C++20, `-Wall -Wextra
-Wpedantic`, and deliberately has **no external dependencies** — raw termios, ANSI,
POSIX `<regex.h>`, libc `wcwidth`. [src, CMakeLists.txt]

---

## 1. CLI surface

### 1.1 argv shapes

There are exactly three accepted shapes. Dispatch is decided by `argv[1]`. [src]

| invocation | meaning |
|---|---|
| `sift` | interactive popup TUI over the **client's active pane** (resolved from tmux, §2.1) |
| `sift <pane-id>` | interactive popup TUI over `<pane-id>` |
| `sift rows <pane-id> <regex>` | headless seam: print one tab-separated row per match, no TUI |

Details:

- The `rows` subcommand is selected by an exact `strcmp(argv[1], "rows") == 0`. Any other
  `argv[1]` — including a string that merely starts with `rows` — is treated as a pane
  id. [src]
- `sift rows` requires `argc >= 4`. Extra arguments beyond `argv[3]` are **silently
  ignored**. [src]
- In the TUI shape, `argv[2]` and beyond are ignored. [src]
- There is no `--help`, no `--version`, no flag parsing of any kind. A leading `-` is
  just a pane id that will not resolve. [src]
- `main` begins with `setlocale(LC_ALL, "")` before anything else. This is
  load-bearing — see §3.5 and §4.3. [src]

### 1.2 Exit codes

**The program always exits 0.** Every error path returns 0. [src, meas]

This is an invariant inherited from `tools/ARCHITECTURE.org` and stated in the source
header: *"Always exit 0. A non-zero exit from a key binding surfaces as a tmux error
popup."* A Rust port must not use `std::process::exit(1)`, must not return `Err` from
`main`, and must not panic on any reachable input path (a panic exits 101).

Measured [meas]: `sift rows %0 '('` → exit 0; `sift rows` (too few args) → exit 0;
`sift` with no reachable tmux server → exit 0; `sift %999` against a live server → exit 0.

The only non-zero `_exit` in the program is inside the forked child of the tmux spawn
helper (`_exit(127)` when `dup2` or `execvp` fails); it is never the process's own exit
status. [src]

### 1.3 stderr messages (verbatim)

Exactly three strings are written to stderr by the program itself. Each ends with a
newline. [src]

```
usage: sift rows <pane-id> <regex>
```
Emitted when `argv[1] == "rows"` and `argc < 4`. [src, meas]

```
sift: no pane — run it inside tmux, or pass a pane id
```
Emitted from `main` in the TUI shape when pane resolution returns an empty string. The
dash is U+2014 EM DASH. [src, meas]

```
sift: invalid regex: <regerror text>
```
Emitted from the `rows` path when `regcomp` fails. `<regerror text>` is glibc's
`regerror` message, e.g. `Unmatched ( or \(`. Format string is
`"sift: invalid regex: %s\n"`. [src, meas]

Note that this last one appears **only** in the `rows` path. In the TUI, an invalid
pattern is reported in the header (§7), never on stderr. [src]

Additionally, **tmux's own stderr is inherited** by every tmux subprocess the program
spawns (the pipe replaces stdout only). So tmux's diagnostics leak through to the user's
stderr: [src, meas]

```
can't find pane: %999
error connecting to /nonexistent/sock (No such file or directory)
```

These are not sift's strings and must not be reproduced by the port — the port simply
must not redirect the child's stderr.

### 1.4 tmux status-line messages (verbatim)

Four user-facing messages go out as `tmux display-message -l <text>` (see §2.6). They are
*not* printed to stdout or stderr. [src]

```
sift: cannot read pane <pane>
sift: nothing to search in <pane>
sift: no terminal (run it from a tmux popup)
sift: the pane moved — landed on the nearest match of /<pattern>/
```

`<pane>` is the resolved pane id, `<pattern>` the regex the user typed. The dash in the
last message is U+2014 EM DASH. Each is built by plain string concatenation. [src]

### 1.5 Behaviour outside tmux, and with a bad pane id

**No tmux server reachable** (`$TMUX` points at a dead socket, or no server on the
default socket). `tmux display-message -p '#{pane_id}'` exits non-zero, pane resolution
yields `""`, and the program prints the `sift: no pane —…` line to stderr and exits 0.
tmux's own `error connecting to …` line precedes it. [meas]

**`tmux` binary not on `PATH`.** `execvp` fails, the child `_exit(127)`s, the spawn
reports failure, and the outcome is identical to the previous case: `sift: no pane —…`,
exit 0. [meas]

**Outside tmux but a tmux server *is* running on the default socket.** This is a trap
worth stating explicitly: `tmux display-message -p '#{pane_id}'` **succeeds** — tmux
falls back to the default socket and names some active pane — so the program proceeds to
search a pane the user never asked about. It then fails at raw-mode setup (§5.1) and
emits `sift: no terminal (run it from a tmux popup)` into that unrelated server's status
line, and exits 0. [meas] There is no "am I inside tmux?" check anywhere in the program;
`$TMUX` is never read by sift itself, only by the tmux child. [src]

**Bad pane id, TUI shape** (`sift %999`). `origin_pane` returns the argument as-is
without validation. `pane_geom` then runs `tmux display-message -p -t %999 …`, which
fails; tmux writes `can't find pane: %999` to stderr; sift emits `sift: cannot read pane
%999` via `display-message -l` and returns 0. [src, meas]

**Bad pane id, `rows` shape.** `pane_geom` fails and `run_rows` returns 0 immediately —
**silently**, with no message of any kind from sift (only tmux's inherited stderr). [src]

**Argument that still looks like an unexpanded tmux format.** If the argument begins with
the two characters `#{`, it is discarded and the pane is resolved from tmux instead. This
is a deliberate self-heal for a regressed key binding (§2.1). Any *other* argument,
including an empty string, is honoured verbatim — note an explicitly empty `argv[1]`
(`sift ""`) falls through to tmux resolution too, because the test is `arg && *arg &&
strncmp(arg,"#{",2) != 0`. [src, meas]

### 1.6 `rows` output format

One line per match, written to stdout with `printf("%ld\t%d\t%d\t%d\t%s\n", …)`. Five
tab-separated fields, terminated by `\n`: [src, meas]

| # | field | meaning |
|---|---|---|
| 1 | `line` | 0-based index into the capture — physical line counting from the top of history |
| 2 | `char_start` | number of **characters** before the match start |
| 3 | `char_end` | number of **characters** before the match end |
| 4 | `cell_start` | number of **terminal cells** before the match start |
| 5 | `text` | the whole capture line, unmodified, un-escaped, un-truncated |

Byte offsets are **not** emitted, even though they are computed internally. [src]

Measured example [meas] — pane containing `中文測試 aa999 尾巴` at capture line 3,
`sift rows %0 尾巴`:

```
3	11	13	15	中文測試 aa999 尾巴
```

11 characters precede `尾` (`中文測試` = 4, space, `aa999` = 5, space); 13 characters
precede the end; 15 cells precede it (4 CJK chars × 2 cells + 1 + 5 + 1). The byte
offset would have been 19 and is deliberately not shown.

Measured example with three hits on one line [meas]:

```
4	0	3	0	foo foo foo
4	4	7	4	foo foo foo
4	8	11	8	foo foo foo
```

Note the same line text is repeated per occurrence — one row per **occurrence**, not per
line. Rows appear in scrollback order: ascending line index, then ascending byte offset
within the line. [src, meas]

Because the text field is emitted raw and the separator is a literal tab, a capture line
containing a tab is ambiguous to a naive parser. That is accepted; do not "fix" it.
(Capture lines cannot contain a newline — they are the split units — and cannot contain
NUL.) [src]

---

## 2. tmux interaction

### 2.0 How tmux is invoked

Every tmux call goes through one helper. Its contract, which the port must preserve
exactly: [src]

- `pipe()`, `fork()`, in the child `dup2(pipe_write, STDOUT_FILENO)`, then
  `execvp("tmux", argv)` with **`argv[0] = "tmux"`** and the caller's arguments appended
  as an argv vector.
- **Never a shell string.** This is a stated invariant: *"tmux is always spawned via
  execvp with an argv vector — never a shell string — so a scrollback line full of shell
  metacharacters is inert."* A Rust port uses `std::process::Command` with `.arg()` per
  element and must never build a `sh -c` string. [src]
- **stderr is left attached to sift's own.** Do not capture or suppress it. [src]
- stdout is read to EOF into a `String` (64 KiB read buffer; no size limit on the
  result).
- `waitpid` is retried on `EINTR`.
- Success = `WIFEXITED(status) && WEXITSTATUS(status) == 0`. On `pipe()` or `fork()`
  failure the helper returns an empty string and reports failure.
- The captured stdout is returned **whether or not the command succeeded**; callers that
  care check the success flag.

There is no timeout, no retry, and no environment manipulation. tmux finds its server
the ordinary way, through the inherited `$TMUX` / default socket. [src]

### 2.1 Pane resolution (`origin_pane`)

```
tmux display-message -p '#{pane_id}'
```

Issued only when no usable argument was given (§1.5). The output is right-trimmed of
`\n` and `\r` (repeatedly). On success the trimmed string is the pane; on failure the
result is the empty string. [src]

**Why this exists — the load-bearing surprise.** The obvious design, having the key
binding pass `#{pane_id}`, does not work, and its failure mode is invisible in tests.
Measured on tmux 3.5a and recorded in ADR-0005 note 5 and in `claude.conf`:

- `display-popup`'s *shell-command* is **not** format-expanded. A binding written
  `display-popup -E "sift '#{pane_id}'"` hands the program those seven literal
  characters. Neither is `-e VAR=#{pane_id}` expanded.
- `run-shell` *does* expand formats, which is exactly what makes the asymmetry easy to
  miss.
- `$TMUX_PANE` inside a popup names the **popup's own pseudo-pane**, not the pane the
  popup was opened over. Using it would search the wrong pane rather than fail loudly.
- While a popup is open, the client's active pane is still the pane the key was pressed
  in — so asking tmux is correct.

Hence the `#{`-prefix self-heal in §1.5: an argument that still looks like an unexpanded
format means a binding regressed, and the program repairs itself rather than flashing a
popup shut.

The shipped binding passes **no argument at all**:

```tmux
bind -T prefix / display-popup -E -w 95% -h 90% -T ' regex ' \
  "~/.tmux/tools/target/release/sift"
```

(95%/90% rather than the 90%/70% other popups use, because this one is a result list;
on a 282×71 client display-popup hands the program 266×62 — the box minus its border.)
The binding is wrapped in a load-time `if-shell '[ -x …/sift ]'` guard whose else-branch
is a `display-message -l 'sift: not built — …'` stub. [claude.conf]

### 2.2 Geometry query (`pane_geom`)

```
tmux display-message -p -t <pane> '#{history_size}\t#{pane_height}\t#{alternate_on}'
```

The `\t` are literal tab characters inside one format argument. The reply is split on
tabs into at most three fields, each converted with `strtol(field, nullptr, 10)` (base
10; a missing or non-numeric field yields 0). The splitter stops early if a tab is
absent, leaving later fields at 0. Result: `history_size`, `height`, `alternate` (`!= 0`).
The whole struct is marked "not ok" if the command failed. [src]

Called twice: once at startup (both in the TUI and in `rows`), and **again at jump time**
(§2.4). [src]

### 2.3 Scrollback capture

```
tmux capture-pane -p -t <pane> -S -<history_size> -E <height-1>
```

`-S` is the string `"-"` concatenated with `history_size` (so `-S -0` when there is no
history); `-E` is `height - 1` as a decimal string. [src]

Flags deliberately **absent**, each with a stated reason: [src]

- **No `-J`.** Joining wrapped lines would desynchronise sift's line indices from the
  physical lines copy-mode navigates. The port must not join.
- **No `-e`.** Escape sequences would be matched by the regex and rendered raw.
- No `-N`, no `-C`, no `-a`.

**Splitting.** The blob is split on `\n`. A trailing newline does **not** produce a final
empty element; a run of `\n\n` does produce an empty element. Empty elements are kept in
the vector (they occupy an index) but are skipped by the matcher (§3.4). Index `i` in the
resulting vector is *physical line `i` counting from the top of history* — this is the
coordinate the jump arithmetic is written in. [src]

**Trailing whitespace is trimmed by tmux itself.** Measured [meas]: a pane line printed
as `trail   ` comes back as `trail`. The port inherits this for free by issuing the same
command; do not re-pad.

**Failure.** If the capture command fails, the line vector is empty. In the TUI that
produces `sift: nothing to search in <pane>` and a return; in `rows` it produces no
output at all. An *empty but successful* capture is indistinguishable and takes the same
path. [src]

**The alternate-screen case.** When `#{alternate_on}` is 1, sift does **not** change what
it captures — it issues the identical command and merely prepends a warning to the header
(§7.2). Measured [meas] on tmux 3.5a with a pane running `less /etc/services`:
`#{history_size}` reported **30**, not 0, and `capture-pane -p -S -30 -E 11` returned
**42 lines** — the 30 pre-alternate history lines *followed by* the 12 rows of the
alternate screen. So the header's claim is imprecise; see §8.

### 2.4 The jump sequence

Triggered by `Enter` on a selected hit. Everything below goes out as **one tmux command
list in a single invocation** — the `;` are separate argv elements. This matters: the
popup is still open while it runs, panes do not redraw under a popup, and the pane is
only seen after the process exits and the popup closes. [src]

Step 0 — **re-read the geometry**:

```
tmux display-message -p -t <pane> '#{history_size}\t#{pane_height}\t#{alternate_on}'
```

If this fails, the jump reports failure immediately and nothing is sent. The
`history_size` used below is this fresh one, **not** the one from capture time. [src]

Step 1 — the command list. Base:

```
tmux copy-mode -t <pane> \
   ; send-keys -X -t <pane> history-top \
   …
```

Then exactly one of two branches, on the freshly-read `history_size` (call it `H`) and
the hit's line index `L`:

- **`L <= H`** (target is in history, or is the first visible row):
  ```
   ; send-keys -X -t <pane> goto-line <H - L>
  ```
- **`L > H`** (target is inside the visible screen):
  ```
   ; send-keys -X -t <pane> goto-line 0
   ; send-keys -X -N <L - H> -t <pane> cursor-down      # only if L - H > 0
  ```

Then, **only if `char_end > 0`**:

```
   ; send-keys -X -N <char_end> -t <pane> cursor-right
```

Then, unconditionally:

```
   ; send-keys -X -t <pane> search-backward <pattern>
```

Note the argument order is `-X -N <n> -t <pane> <command>` for the counted sends and
`-X -t <pane> <command>` for the uncounted ones. Reproduce it. [src]

**Why each piece — the load-bearing surprises.** All measured on tmux 3.5a; the source
comments, ADR-0005 §Notes and the atlas node all call these out:

1. **`goto-line N` does not go to line N.** It sets `oy`, the scroll offset from the
   *bottom* of the history, and leaves the cursor row `cy` untouched. The absolute line
   is `history_size - oy + cy`. So the cursor row must be pinned first — `history-top`
   puts it at `cy = 0` — and only then is `goto-line (history_size - L)` an exact seek to
   line `L`. A port that treats `goto-line` as "go to line" lands somewhere plausible and
   wrong.
2. **`oy` is not stable while the pane keeps printing; the index from the top of history
   is.** That is why `history_size` is re-read at jump time rather than reused from the
   capture.
3. **`search-forward` leaves the cursor one cell *past* the end of the match;
   `search-backward` leaves it on the match *start*.** seek's `w`/`W`/`l`/`L` grab keys
   read the token *under* the cursor, so the backward search is the only usable one —
   which is precisely why the cursor is first seated `char_end` characters right (just
   past the chosen occurrence) and the search then walks back onto it.
4. **Registering the pattern with tmux is the point of the final step, not just the
   positioning.** It is what makes the match highlight, `n`/`N`, and seek's grab keys work
   after the popup closes. Not owning the final jump is the whole architectural bargain
   of ADR-0005 — sift picks the occurrence, tmux performs the search.
5. **`cursor-right` counts CHARACTERS**, which is why `char_end` (not `byte_end`, not
   `cell_end`) is the argument. See §4.1.

If the command list fails (non-zero exit), the jump reports failure and the verification
below is skipped. [src]

### 2.5 Landing verification

```
tmux display-message -p -t <pane> \
  '#{history_size}\t#{scroll_position}\t#{copy_cursor_y}\t#{copy_cursor_x}\t#{search_present}'
```

Five tab-separated fields in **one** call, parsed with the same early-stopping
tab-splitter and `strtol` as §2.2. The jump succeeds iff **all three** hold: [src]

```
f[4] == 1                       # search_present
f[0] - f[1] + f[2] == L         # history_size - scroll_position + copy_cursor_y
f[3] == cell_start              # copy_cursor_x
```

**Why verify at all.** Both `search-backward` and `search-forward` **wrap silently** when
they fail, so a bad landing is not reported by tmux — it just lands somewhere plausible.
[src, ADR-0005 note 4]

**Why one call.** All five fields must describe one instant. And
`history_size - scroll_position` is stable while the pane keeps printing, because
copy-mode holds its view and new output grows both terms together. [src]

**Why positional and never textual.** `#{copy_cursor_line}` looks like the obvious probe
and is a trap: measured on 3.5a it **truncates at the first wide character** — a line
reading `中文測試 aa999 尾巴` comes back as `中` — so comparing text would false-alarm on
every CJK line. Cursor coordinates have no such problem. A port must not "improve" this
by comparing the line text. [src, ADR-0005 note 6]

Note the asymmetry the port must reproduce: the row check is against `L` in **line**
units and the column check is against `cell_start` in **cell** units, even though the
cursor was moved with a **character** count.

On failure the program emits
`sift: the pane moved — landed on the nearest match of /<pattern>/` (§1.4) and still
exits 0. The pane is left wherever tmux put it — sift does **not** try to undo the jump.
[src]

Measured end-to-end [meas]: after `Enter` on a hit at line 3, the pane reported
`pane_in_mode=1 history_size=29 scroll_position=26 copy_cursor_y=0 copy_cursor_x=7
search_present=1` → `29 - 26 + 0 = 3` ✓.

### 2.6 Literal messages

```
tmux display-message -l <text>
```

Every user-facing message carries `-l`. This is a stated invariant: *"Pane text never
enters a format context: no pane options are stamped and every display-message carries
-l."* Pane text reaches the user only inside sift's own popup rendering, so there is no
format context to escape from and nothing has to be filtered — filtering the text would
corrupt what the user wants to read. The `-l` is not optional and the port must not drop
it. [src]

Related invariants from the same header block: **never write pane titles**, and **stamp
no pane options**. sift issues neither `set-option` nor `select-pane -T`. [src]

### 2.7 Complete inventory

Six distinct invocation sites, four distinct tmux command words
(`display-message`, `capture-pane`, `copy-mode`, `send-keys`), in this order:

| # | when | command |
|---|---|---|
| 1 | pane resolution, if needed | `display-message -p '#{pane_id}'` |
| 2 | startup (TUI and `rows`) | `display-message -p -t P '#{history_size}\t#{pane_height}\t#{alternate_on}'` |
| 3 | startup (TUI and `rows`) | `capture-pane -p -t P -S -<H> -E <height-1>` |
| 4 | any user-facing message | `display-message -l <text>` |
| 5 | on Enter: geometry re-read | same as #2 |
| 6 | on Enter: the jump | `copy-mode -t P ; send-keys -X -t P history-top ; send-keys -X -t P goto-line N [; send-keys -X -N D -t P cursor-down] [; send-keys -X -N C -t P cursor-right] ; send-keys -X -t P search-backward <pattern>` |
| 7 | on Enter: verification | `display-message -p -t P '#{history_size}\t#{scroll_position}\t#{copy_cursor_y}\t#{copy_cursor_x}\t#{search_present}'` |

No other tmux command is ever issued. In particular: no `set-option`, no `select-pane`,
no `list-panes`, no `send-keys` outside `-X` copy-mode commands, no `refresh-client`.
[src]

---

## 3. Regex semantics

### 3.1 Compilation flags

```c
regcomp(&re, pattern, REG_EXTENDED)
```

**`REG_EXTENDED` and nothing else.** [src] In particular:

- **No `REG_ICASE`.** Search is **case sensitive**, always, with no way to turn it off
  and no inline flag syntax available (§3.3). Measured [meas]: `ABC` does not match
  `abc`.
- **No `REG_NEWLINE`.** Irrelevant in practice — the input is already split on newlines,
  so no subject string contains one.
- **No `REG_NOSUB`.** `regexec` is called with `nmatch = 1` and one `regmatch_t`; only
  group 0 (the whole match) is ever read. Capture groups may be written by the user but
  their spans are never used. [src]

The compiled regex is freed (`regfree`) on every path, including the failure path. [src]

### 3.2 Execution flags — `REG_NOTBOL` on continuation scans

Matching a line is a loop over byte offset `off`, starting at 0:

```c
int flags = (off == 0) ? 0 : REG_NOTBOL;
regexec(re, s.c_str() + off, 1, &m, flags);
```

Two consequences the port must reproduce exactly: [src]

- The subject handed to `regexec` is the **suffix** `s + off`, so the returned
  `rm_so`/`rm_eo` are relative to `off` and must be re-based by adding `off`.
- `REG_NOTBOL` on every scan after the first means `^` cannot match at a continuation
  offset. Measured [meas]: `^a` on `ab` matches `[0,1)` without the flag and **NOMATCH**
  with it. The observable effect is correct and desirable: an anchored pattern like `^foo`
  yields **exactly one hit per line**, never a spurious second one.
- **`REG_NOTEOL` is *not* set.** `$` is therefore unaffected and still anchors at the true
  end of the line, because the suffix's end *is* the line's end. Measured [meas]: `b$` on
  `ab` matches `[1,2)` with and without `REG_NOTBOL`.
- **Word-boundary operators do *not* get the same protection.** `\<` and `\b` treat
  position 0 of the *suffix* as a boundary, losing the character that precedes it in the
  full line. Measured [meas]: `\<a` scanned this way over `aa a` produces hits at bytes
  0, 1 and 3 — byte 1 is not a real word start. A Rust port that scans the *whole* line
  with a start-offset-aware API (e.g. `regex::Regex::find_at`, which keeps the preceding
  context) would produce **two** hits here, not three. To match sift, the port must slice
  the subject.

### 3.3 POSIX ERE consequences (glibc), measured

These are the semantics a different regex engine would get wrong. All measured 2026-08-31
by calling glibc 2.41 `regcomp(REG_EXTENDED)`/`regexec` directly, under
`setlocale(LC_ALL, "")` with `LC_CTYPE=en_US.UTF-8`. [meas]

**Leftmost-longest, not leftmost-first.** This is the single largest divergence from
Rust's `regex` crate.

| pattern | subject | glibc ERE result | Rust `regex` would give |
|---|---|---|---|
| `a\|ab\|abc` | `xabc` | `[1,4)` = `abc` | `[1,2)` = `a` |
| `(abc\|abcd)x` | `abcdx` | `[0,5)` = `abcdx` | `[0,5)` (backtracking would too) |
| `(a\|ab)(c\|bcd)` | `abcd` | `[0,4)` = `abcd` | `[0,4)` |

The first row is the one that bites: alternation picks the **longest** alternative at the
leftmost start, not the first-listed one. A hit list built with leftmost-first semantics
shows different spans, different highlight extents, different `char_end` values (so a
different cursor seat), and — because the advance is `off = e` — potentially a different
*number* of hits.

**Backreferences are supported.** `(a+)\1` on `aaaa` → `[0,4)`. This is a glibc extension
to ERE. Rust's `regex` crate **cannot** compile this at all — it would be an error where
sift finds a match.

**GNU operators are enabled; Perl classes mostly are not.**

| syntax | glibc ERE | note |
|---|---|---|
| `\w` `\W` | **works** — `\w+` on `ab_c!` → `[0,4)` | GNU extension; equals `[[:alnum:]_]` |
| `\s` `\S` | **works** — `\s` on `a b` → `[1,2)` | GNU extension |
| `\b` `\<` `\>` | **works** — `\bfoo` on `a foo` → `[2,5)` | GNU extensions |
| `\d` `\D` | **does not work** — `\d+` on `a123` → NOMATCH | matches a literal `d` |
| `\n` `\t` | **not escapes** — mean literal `n`, `t` | `\n` on `a<LF>b` → NOMATCH |
| `\.` | works — escapes the metacharacter | |
| `(?i)` `(?:…)` `(?=…)` | **compile error** `Invalid preceding regular expression` | no Perl group syntax at all |
| `a+?` (lazy) | **not lazy** — `a+?` on `aaa` → `[0,3)` | `?` is a redundant quantifier on `a+` |

Rust's `regex` crate has almost the mirror-image profile: it supports `\d` and lazy
quantifiers and `(?i)`, and does **not** support backreferences. Neither the source nor
this spec authorises silently swapping the dialect.

**Intervals `{n,m}`.** Standard and greedy: `a{2,3}` on `aaaa` → `[0,3)`. `a{,3}` is
accepted as `{0,3}` (glibc leniency). `a{3,1}` is a compile error, `Invalid content of
\{\}`. `{1}` with nothing preceding is a compile error. `a**` is *accepted*
(`[0,2)` on `aa`).

**Bracket expressions.** POSIX character classes work: `[[:alpha:]]+` on `abc1` →
`[0,3)`. Equivalence classes work: `[[=a=]]` matches `á` in this locale. Collating
symbols do **not**: `[[.hyphen.]]` → compile error `Invalid collation character`. `]` as
the first bracket member is literal: `[]a]` matches `]`. `[z-a]` is a compile error,
`Invalid range end`. `[a` (unterminated) is a compile error.

**Anchors.** `^` and `$` are ordinary anchors of the subject string. `$` alone matches
the empty span at end-of-subject: `$` on `ab` → `[2,2)`.

### 3.4 The scan loop, exactly

For each capture line `s`, in ascending line index: [src]

```
if (s is empty) skip this line entirely
off = 0
while (off <= s.size()):
    if regexec(re, s + off, flags = (off == 0 ? 0 : REG_NOTBOL)) != 0: break
    b = off + rm_so;  e = off + rm_eo
    push Hit{ line, byte_start=b, byte_end=e,
              char_start=chars(s, b), char_end=chars(s, e),
              cell_start=cells(s, b) }
    if hits.size() >= kMatchCap: return hits          // stop everything
    off = (e == b) ? e + 1 : e                        // +1 BYTE on empty match
```

Four behaviours to reproduce precisely:

1. **Empty lines are skipped before any regex work.** An empty-matching pattern (`x*`,
   `^`, `()`) therefore produces **no hit at all** on an empty capture line, where a
   naive port would produce one. Measured [meas]: with `o*` over a pane whose line 2 was
   blank, the `rows` output jumps from line 1 to line 3.
2. **The loop bound is `off <= s.size()`, not `<`.** An empty-matching pattern gets one
   final scan against the empty suffix, so it produces `len+1` hits on an `len`-byte
   line. Measured [meas]: `x*` on `yyy` → hits at bytes 0, 1, 2, **3**.
3. **The empty-match advance is `+1 BYTE`, not +1 character.** Measured [meas]: `x*` on
   `中文` (6 bytes) → **7** hits, at bytes 0..6, i.e. hits landing *inside* multi-byte
   characters. The resulting `char_start`/`cell_start` are what §4.1's counters return
   for a mid-character byte offset (§4.2). This is faithful behaviour, not a bug to fix.
4. **A non-empty match advances by exactly its own length**, so overlapping matches are
   not found: `a` on `aaa` → 3 hits; `aa` on `aaa` → 1 hit at `[0,2)`.

### 3.5 Caps

```c
constexpr size_t kMatchCap = 20000;
```

**There is exactly one cap and it is global, not per line.** [src] The counter is the
total number of hits collected across the whole capture. `find_all` returns *immediately*
when `hits.size() >= kMatchCap` — mid-line, without finishing the current line and without
looking at any later line. There is no per-line limit of any kind. (Note: the task brief
speaks of "per-line and total caps"; only the total exists.)

The cap applies in **both** the TUI and `rows`. [src]

`capped` is computed as `hits.size() >= kMatchCap`, which is true exactly when the cap was
hit. It is surfaced in the header as `20000+ matches (capped)` — reported rather than
silently applied (§7.2). `rows` does not report it at all: the output simply stops at
20000 lines. [src]

Rationale from the source: `.` over a full 100 000-line scrollback is a legitimate
keystroke on the way to a real pattern; the cap keeps that from costing seconds. The
runbook records one full filter pass over a 100 000-line scrollback at **53 ms** on this
machine (2026-08-27).

### 3.6 Malformed patterns

`regcomp` failure is handled differently in the two paths: [src]

- **TUI (`refilter`)**: `bad_re` is set, the `regerror` text (truncated to a 128-byte
  buffer) is stored, the hit list is **emptied**, and `regfree` is called. The list on
  screen goes blank and the header reads `invalid regex: <text>` (§7.2). Keeping the
  previous result set would be a lie about what the pattern matches — a half-typed regex
  is invalid most of the time, so this state is common and must not be treated as an
  error condition. `sel` and `top` are **not** reset on this path (only the
  empty-pattern path resets them); with an empty hit list they are harmless and the next
  successful `refilter` overwrites `sel`.
- **`rows`**: the message goes to stderr and the process returns 0 with no output
  (§1.3).

The `regerror` buffer is 128 bytes in both places; longer messages are truncated by
`regerror`'s own semantics. [src]

Note glibc's `regfree` is called on a `regex_t` that failed to compile. That is the
source's behaviour; a Rust port has nothing to reproduce here.

### 3.7 Empty pattern

An empty pattern short-circuits before `regcomp`: hits cleared, `capped` false, `bad_re`
false, `sel = 0`, `top = 0`, return. Header reads `type an extended regex` (§7.2). [src]
This matters — `regcomp("")` under `REG_EXTENDED` would otherwise compile and match
everything.

---

## 4. Text handling

### 4.1 Three different "columns"

The source is explicit that confusing these is the classic bug here, and that ADR-0004
was written about its sibling in `seek`: [src]

| unit | who counts in it | consumer |
|---|---|---|
| **bytes** | `regexec` (`rm_so` / `rm_eo`) | internal only; feeds the highlight span in rendering |
| **characters** | `utf8_chars` | copy-mode's `cursor-right -N` moves by character |
| **cells** | `utf8_cells` (CJK = 2) | `#{copy_cursor_x}` and the screen are measured in cells |

Nothing may mix them silently. Specifically: `cursor-right` is given `char_end`; the
landing verification compares `#{copy_cursor_x}` against `cell_start`; the `rows` seam
emits both plus the line index.

### 4.2 UTF-8 decoding

A hand-rolled decoder, deliberately permissive. Its contract: **decode one code point at
byte index `i`, return the byte length, which is always ≥ 1 so a malformed byte advances
rather than looping forever.** [src]

```
c = s[i]
avail = s.size() - i
cont(k)  ≡  k < avail  &&  (s[i+k] & 0xC0) == 0x80

c < 0x80                                        → cp = c;                      len 1
(c & 0xE0) == 0xC0 && cont(1)                   → cp = ((c&0x1F)<<6) | (s[i+1]&0x3F);   len 2
(c & 0xF0) == 0xE0 && cont(1..2)                → cp = ((c&0x0F)<<12) | … ;              len 3
(c & 0xF8) == 0xF0 && cont(1..3)                → cp = ((c&0x07)<<18) | … ;              len 4
otherwise                                       → cp = 0xFFFD;                 len 1
```

Note what is **not** checked, and must not be added by the port: no overlong-encoding
rejection, no surrogate rejection, no `cp > 0x10FFFF` rejection, no validation that
`c` is not a bare continuation byte beyond the `0xFFFD` fallback. A 5-byte-lead byte,
an overlong `C0 80`, and a lone `0x80` all take the `0xFFFD`/length-1 path or decode as
written. A Rust port must **not** use `str::chars()` on validated UTF-8 — capture output
is not guaranteed valid UTF-8 and the substitution behaviour differs. Operate on `&[u8]`
and reimplement the table above.

Two counters are built on it: [src]

```
utf8_chars(s, byte_end): n = 0; for (i = 0; i < byte_end && i < s.size(); ) { decode; ++n; i += len; }
utf8_cells(s, byte_end): n = 0; for (i = 0; i < byte_end && i < s.size(); ) { decode; n += cell_width(cp); i += len; }
```

Both loop on `i < byte_end`, but decoding may carry `i` *past* `byte_end`. So when
`byte_end` falls **mid-character**, the straddling character is counted **in full**.
This is exactly the situation the empty-match `+1 byte` advance (§3.4) creates.

### 4.3 `cell_width` and `wcwidth`

```c
int cell_width(wchar_t cp) { int w = wcwidth(cp); return w < 0 ? 1 : w; }
```

[src] Three cases:

- **`wcwidth` returns ≥ 1** — used as-is. CJK ideographs, fullwidth forms and most emoji
  return 2 in this locale.
- **`wcwidth` returns 0** — combining marks, zero-width joiner, etc. Contributes **0
  cells**. A combining mark therefore adds 1 to `char_start` and 0 to `cell_start`, and
  in rendering (§7.3) occupies no width.
- **`wcwidth` returns −1** — control characters (C0/C1) and code points unassigned in the
  locale, *including* `0xFFFD` results from the malformed-byte path if the locale does not
  know it. These are rendered as **one cell** rather than vanishing, per the source
  comment: *"render control/unknown as one cell rather than vanish"*. This is the one
  place the program deliberately diverges from what the terminal will actually do.

**`setlocale(LC_ALL, "")` is a hard prerequisite.** It is the first statement in `main`.
[src] Without it the process runs in the `"C"` locale and:

- `wcwidth` returns −1 for every non-ASCII code point, so every CJK character would count
  as 1 cell instead of 2, and every `cell_start` sent to the landing verification would
  be wrong on a CJK line — the jump would report `sift: the pane moved` on every such
  line.
- glibc's regex engine switches to byte-wise matching: measured [meas] under
  `LC_CTYPE=en_US.UTF-8`, `.` on `中文` matches `[0,3)` — one whole three-byte character —
  and `[[:alpha:]]+` on `中文x` matches all 7 bytes. In the `C` locale `.` would match one
  byte and split characters.

A Rust port has no `setlocale`; it must reimplement `wcwidth` (an East-Asian-width table)
and must decide the regex engine's Unicode mode explicitly. The behaviour to reproduce is
the **UTF-8-locale** behaviour above, because that is what the shipped program does under
the environment it ships into.

### 4.4 Truncation and highlighting at a given width (`render_line`)

Inputs: the line `s`, the match's `byte_start` and `byte_end`, and a target `width` in
cells. Output: a string of escape sequences and text occupying at most `width` cells.
[src]

```
if width <= 0: return ""

# 1. cells before the match start
cells_to_start = sum of cell_width over decoded chars while byte index < byte_start

# 2. horizontal scroll: put the match about a third in, never before the line start
skip_cells = 0
if cells_to_start > width - 12:  skip_cells = cells_to_start - width / 3   # integer division
if skip_cells < 0:               skip_cells = 0

# 3. emit
o = ""; cells = 0; seen = 0; inverted = false
if skip_cells > 0:  o += "…"; cells = 1                     # U+2026, 3 bytes, 1 cell

for each decoded char at byte i, width w:
    if seen + w > skip_cells:
        if cells + w > width: break                          # stop, no ellipsis at the end
        want = (i >= byte_start && i < byte_end)
        if  want && !inverted: o += "\x1b[7m";  inverted = true
        if !want &&  inverted: o += "\x1b[27m"; inverted = false
        o += the char's bytes
        cells += w
    seen += w
    i += len
if inverted: o += "\x1b[27m"
```

Behaviours worth naming:

- The `12` and the `width / 3` are exact magic numbers; reproduce them, including the
  integer division.
- The head ellipsis is `…` (U+2026, three bytes) and counts as **one** cell. It is not
  guaranteed to be aligned with the first emitted character, and there is deliberately
  **no** tail ellipsis — long lines just stop.
- The "is this char inside the match" test uses the **byte** index of the character's
  first byte against `[byte_start, byte_end)`.
- The invert is entered/left with `ESC[7m` / `ESC[27m` (not `ESC[0m`), so surrounding
  attributes survive. A trailing `ESC[27m` is emitted if the match ran to the cut.
- A zero-width character exactly at the `skip_cells` boundary (`seen + 0 > skip_cells` is
  false when `seen == skip_cells`) is **dropped**. Faithful; do not fix.
- Because `w` can be 0, `cells + w > width` never breaks on a combining mark.

Measured [meas], `width = 56`, hit at line start:

```
foo bar                        →  \x1b[7mfoo\x1b[27m bar
```

Measured [meas], match far right of a long line — the row rendered as
`> 0 … do echo "line $i foo"; done`, confirming the head ellipsis and horizontal scroll.

---

## 5. Terminal control

### 5.1 Raw mode

Entering raw mode: [src]

```c
tcgetattr(STDIN_FILENO, &g_saved)                     // fail → raw() returns false
t = g_saved
t.c_lflag &= ~(ECHO | ICANON | ISIG | IEXTEN)
t.c_iflag &= ~(IXON | ICRNL | INLCR | BRKINT | ISTRIP)
t.c_oflag &= ~(OPOST)
t.c_cc[VMIN]  = 1
t.c_cc[VTIME] = 0
tcsetattr(STDIN_FILENO, TCSAFLUSH, &t)                // fail → raw() returns false
g_raw = true
write(STDOUT_FILENO, "\x1b[?1049h\x1b[2J", 12)
```

Exact flag list — nothing more, nothing less. This is **not** `cfmakeraw`: `IGNBRK`,
`PARMRK`, `INPCK`, `IGNCR` and `IXANY` are left alone, and `CS8` is **not** forced.
Reproduce the set that is actually touched. [src]

Consequences the key map depends on:

- `ICRNL` cleared → Enter arrives as byte 13 (`\r`), not 10. The decoder accepts both.
- `ISIG` cleared → `C-c`, `C-\`, `C-z` deliver bytes 3, 28, 26; **no signal is raised**.
  `C-c` is handled as a key (§6).
- `IXON` cleared → `C-s`/`C-q` deliver bytes 19/17 rather than flow control.
- `OPOST` cleared → the terminal does **not** translate `\n` to `\r\n`; every line break
  the renderer emits is a literal `\r\n` (§7).
- `VMIN = 1, VTIME = 0` → a blocking `read` returns after one byte. Timing is done by
  `poll`, not by the termios timer.

If `raw()` fails (either `tcgetattr` or `tcsetattr`), the TUI emits
`sift: no terminal (run it from a tmux popup)` and returns 0 — no raw mode, no
alternate screen, nothing written to stdout. [src]

### 5.2 Restore path

```c
void cooked() {
    if (!g_raw) return;                                          // idempotent
    tcsetattr(STDIN_FILENO, TCSAFLUSH, &g_saved);
    g_raw = false;
    write(STDOUT_FILENO, "\x1b[?25h\x1b[?1049l", 14);            // show cursor, leave alt screen
}
```

[src] Registered with `atexit(cooked)` immediately after `raw()` succeeds, and also
called **explicitly** on the two normal exits: [src]

- `Esc`/cancel: `cooked()` then return.
- `Enter`: `cooked()` is called **before** the jump is issued, with the comment *"restore
  before the pane redraws"* — the popup's terminal must be back in cooked mode and off
  the alternate screen before tmux starts moving the target pane.

**There is no signal-based restore.** The only signal handler installed is `SIGWINCH`;
`SIGTERM`, `SIGHUP`, `SIGINT` (which cannot arrive anyway, `ISIG` is cleared) and
`SIGSEGV` are unhandled, and `atexit` handlers do not run on death by signal. Killing the
process leaves the terminal in raw mode on the alternate screen. This is tolerable only
because that terminal is a tmux popup that dies with the process. A Rust port using a
`Drop` guard gets the same coverage as `atexit` (normal returns) and no more, which is
correct. Do not add signal-based cleanup without being told to. [src]

### 5.3 Terminal size

```c
ioctl(STDOUT_FILENO, TIOCGWINSZ, &ws)
if (ok && ws.ws_col > 0 && ws.ws_row > 0)  { w = ws_col; h = ws_row; }
else                                        { w = 80;    h = 24;    }
```

[src] Note the query is on **stdout**, not stdin, and the zero-guard is required (tmux
can report 0 transiently). The size is re-queried **inside every `draw()` call** and
again inside the PgUp/PgDn handler; it is never cached. [src]

### 5.4 SIGWINCH — the trap

```c
volatile sig_atomic_t g_resized = 0;
void on_winch(int) { g_resized = 1; }
...
signal(SIGWINCH, on_winch);
```

and in the event loop:

```c
Input in = read_key();
if (g_resized) { g_resized = 0; draw(u); }
switch (in.key) { ... }
```

[src] The intent is obvious: note the resize, redraw. **What actually happens is that a
resize while sift is idle terminates the program.**

Mechanism: `read_key` blocks in `poll(&p, 1, -1)`. Per `signal(7)`, `poll` is **never**
restarted after a handler runs, regardless of `SA_RESTART` (which glibc's `signal()` does
set). So `poll` returns −1/`EINTR`, `read_byte` returns −1, and `read_key`'s
`if (c < 0) { in.key = K_ESC; return in; }` yields **`K_ESC`**. The loop then draws once
(because `g_resized` is set) and immediately takes the `K_ESC` branch: `cooked()` and
`return 0`. `EINTR` is never distinguished from a real error, and `read_byte` never
retries.

Measured [meas], 2026-08-31, tmux 3.5a, shipped binary:

- Control: sift running in a pane with a pattern typed, left idle 3 s → pane still alive,
  list still rendered.
- Test: `tmux resize-pane -t <sift pane> -y 6` → the sift pane is **gone**; the process
  exited. Repeated in a second independent run.

So the observable contract is: **resizing the popup (or the client) while sift waits for
a key cancels the search and leaves the target pane untouched.** The `g_resized` redraw
branch only ever fires on the same iteration that is about to exit; it is effectively
dead code.

A Rust port that retries on `EINTR` — which is the natural thing to do, and what
`nix::poll` users usually write — will **not** exit on resize. That is a behaviour change.
Decide it deliberately: it is arguably a fix, but it is not what this spec describes.
See §8 and §9.

### 5.5 `poll()` timeout discipline

All input goes through one function: [src]

```c
int read_byte(int timeout_ms) {
    pollfd p{STDIN_FILENO, POLLIN, 0};
    if (poll(&p, 1, timeout_ms) <= 0) return -1;
    unsigned char c;
    if (read(STDIN_FILENO, &c, 1) != 1) return -1;
    return c;
}
```

One byte per call. `poll` returning 0 (timeout) and −1 (error, including `EINTR`) are
**not distinguished** — both give −1, as does a short/failed `read` and EOF.

Exactly three timeout values are used: [src]

| call site | timeout | on −1 |
|---|---|---|
| first byte of a key | **−1** (block forever) | `K_ESC` — cancel (§5.4) |
| bytes 2, 3, 4 of an escape sequence | **40 ms** | `K_ESC` after byte 2 or 3; the `~` consumer ignores the result |
| continuation bytes of a UTF-8 character | **40 ms** | `break` — emit the partial character as text |

The 40 ms window is what separates a bare `Escape` (cancel) from an escape *sequence* (a
cursor key). The source names this: *"The only thing separating them is timing, so give
the rest of the sequence a brief window to arrive."* Reproduce the value. [src]

### 5.6 Every ANSI sequence emitted, verbatim

| where | bytes | effect |
|---|---|---|
| `raw()` | `\x1b[?1049h\x1b[2J` | enter alternate screen, clear it |
| `cooked()` | `\x1b[?25h\x1b[?1049l` | show cursor, leave alternate screen |
| `draw()` frame start | `\x1b[H\x1b[2J` | home, clear |
| header, prompt | `\x1b[1m` … `\x1b[0m` | bold prompt |
| header, status | `\x1b[2m` … `\x1b[0m` | dim status (only if it fits, §7.2) |
| every row end / blank row | `\r\n` | literal CR LF (`OPOST` is off) |
| selected row marker | `\x1b[1m> ` | bold, and the bold is **not** closed until end of row |
| unselected row marker | `  ` (two spaces) | |
| row line number | `\x1b[2m` `<num>` `\x1b[22m ` | dim number, then un-dim, then a space |
| row end | `\x1b[0m\r\n` | reset |
| match highlight (in `render_line`) | `\x1b[7m` … `\x1b[27m` | reverse video on/off |
| footer | `\x1b[2m↑↓ select  Enter jump  Esc cancel  C-w word  C-u clear\x1b[0m` | dim |
| cursor park | `\x1b[1;<plen+1>H\x1b[?25h` | move to row 1, column `plen+1`; show cursor |

No other escape sequence is emitted. There is no mouse enabling, no bracketed paste, no
cursor hide during redraw, no scroll-region manipulation, no colour. [src]

**The entire frame is one `write(2)`.** `draw()` accumulates into a single `std::string`
and issues one `write` at the end; `raw()` and `cooked()` each issue their own single
`write`. Return values are discarded (`(void)r`) — short writes are not retried. [src]

---

## 6. Key map

### 6.1 Decoding

Single-byte keys, checked first: [src]

| byte(s) | key |
|---|---|
| 13 (`\r`), 10 (`\n`) | `ENTER` |
| 127 (DEL), 8 (BS) | `BACKSPACE` |
| 23 (`C-w`) | `KILL_WORD` |
| 21 (`C-u`) | `KILL_LINE` |
| 3 (`C-c`), 7 (`C-g`) | `ESC` (cancel) |
| 16 (`C-p`) | `UP` |
| 14 (`C-n`) | `DOWN` |

Then byte 27 (`ESC`) starts sequence decoding: [src]

```
ESC                                  → wait 40 ms for the next byte
  timeout / error                    → ESC (cancel)
  next byte is '[' or 'O'            → wait 40 ms for the third byte
      timeout / error                → ESC (cancel)
      'A'                            → UP
      'B'                            → DOWN
      'H'                            → HOME
      'F'                            → END
      '5'                            → read one more byte with 40 ms (the '~'), DISCARD it → PGUP
      '6'                            → read one more byte with 40 ms (the '~'), DISCARD it → PGDN
      anything else                  → NONE (ignored)
  next byte is anything else         → NONE (ignored, and that byte is consumed and lost)
```

Then: [src]

- Any remaining byte **< 32** → `NONE`, ignored. This includes TAB (9), `C-a` (1),
  `C-d` (4), `C-e` (5), `C-k` (11), `C-l` (12), `C-r` (18), `C-z` (26)… none of them do
  anything.
- Any byte **≥ 32** (other than 127, handled above) → `TEXT`. The byte is pushed, then
  the lead-byte pattern decides how many continuation bytes to read, each with a **40 ms**
  timeout, `break`ing on failure:
  `(c & 0xE0) == 0xC0` → 1 more; `(c & 0xF0) == 0xE0` → 2 more; `(c & 0xF8) == 0xF0` → 3
  more; otherwise 0 more.
  The continuation bytes are **not validated** — whatever arrives is appended. A lone
  `0x80`–`0xBF` byte therefore becomes a one-byte `TEXT` and is appended to the pattern
  as-is.

Important negative results, measured on the shipped binary under tmux's default terminal
emulation [meas]:

- **`Home` and `End` do not work.** tmux sends `ESC [ 1 ~` and `ESC [ 4 ~` (screen/tmux
  terminfo `khome`/`kend`). The decoder sees `ESC [ 1`, falls into the "anything else"
  arm and returns `NONE`; the trailing `~` is then read as a fresh key and lands in the
  pattern as **literal text**. Measured: pattern `foo` + `Home` → header shows
  `regex> foo~` and `no match`; a further `End` → `regex> foo~~`.
  Only the `ESC [ H` / `ESC O H` / `ESC [ F` / `ESC O F` forms are recognised, and tmux
  does not emit them by default. See §8.
- `PageUp`/`PageDown` **do** work — tmux sends `ESC [ 5 ~` / `ESC [ 6 ~`, which are
  handled including the `~` consumption. Measured. Modified variants (`ESC [ 5 ; 2 ~`)
  would consume the `;` and leak `2~` into the pattern.
- `Left`/`Right` (`ESC [ C` / `ESC [ D`) are `NONE` — there is **no cursor movement
  inside the pattern**; editing is append-and-delete-from-the-end only.
- `Delete` (`ESC [ 3 ~`) leaks a `~` the same way `Home` does.

### 6.2 Actions

Handled in the event loop after decoding. `refilter` (§3, §3.7) runs on and only on the
four pattern-mutating keys; the selection keys never re-run the regex. `draw()` runs after
every iteration, including on `NONE`. [src]

| key | action |
|---|---|
| `ESC` (bare Esc, `C-c`, `C-g`, any read failure) | `cooked()`, return 0. **The pane is not touched.** |
| `ENTER` | if the hit list is empty, do nothing; otherwise snapshot `pattern`, `line`, `char_end`, `cell_start` from the selected hit, `cooked()`, run the jump (§2.4–2.5), emit `sift: the pane moved — …` if verification fails, return 0 |
| `UP` / `C-p` / `ESC[A` | `if (sel > 0) --sel` — **no wrap** |
| `DOWN` / `C-n` / `ESC[B` | `if (sel + 1 < hits.size()) ++sel` — **no wrap** |
| `HOME` (`ESC[H` / `ESC OH` only) | `sel = 0` |
| `END` (`ESC[F` / `ESC OF` only) | `if (!hits.empty()) sel = hits.size() - 1` |
| `PGUP` | re-query terminal size; `step = (h > 4) ? h - 3 : 1`; `sel = (sel > step) ? sel - step : 0` |
| `PGDN` | same `step`; `if (!hits.empty()) sel = min(sel + step, hits.size() - 1)` |
| `BACKSPACE` | if pattern empty, nothing; else walk back over UTF-8 continuation bytes (`(b & 0xC0) == 0x80`), then back one more byte if any remain, truncate there, `refilter` |
| `KILL_WORD` (`C-w`) | from the end, skip trailing **spaces** (0x20 only), then skip non-spaces, truncate there, `refilter` |
| `KILL_LINE` (`C-u`) | clear the pattern entirely, `refilter` |
| `TEXT` | append the decoded character's bytes to the pattern, `refilter` |
| `NONE` | nothing (but `draw()` still runs) |

Notes on the arithmetic:

- **Initial selection** is set by `refilter`: `sel = hits.empty() ? 0 : hits.size() - 1`
  — the **last** hit, i.e. the match nearest the bottom of the scrollback. This mirrors
  the most-recent-first bias of the `search-backward` binding sift replaced. `top` is
  reset to 0 and re-derived by `draw`. [src]
- **`sel` is recomputed from scratch on every keystroke that changes the pattern.** There
  is no attempt to preserve the user's position across a refilter.
- **No wrapping anywhere.** `Up` at the first hit and `Down` at the last are no-ops.
- **The page step is one row smaller than the visible list.** The list is `h - 2` rows
  (§7.1) but the step is `h - 3`, giving a one-row overlap between pages. Measured [meas]
  with `h = 8` (list = 6 rows, step = 5): from `sel` = 38 → PgUp → 33 → PgUp → 28 → PgDn
  → 33.
- `PGUP` uses `sel > step` (strict), so a `sel` exactly equal to `step` clamps to 0
  rather than landing on 0 by subtraction — same result, stated for completeness.
- The pattern is stored and edited as **bytes**. `C-w` splits on the ASCII space only —
  not tabs, not punctuation, not CJK spacing.
- `BACKSPACE` deletes one whole UTF-8 character (it strips continuation bytes first),
  but a *malformed* trailing byte sequence can leave it deleting a single byte.

---

## 7. Rendering

Everything below is produced by one `draw()` call, accumulated into one string and
written with one `write`. [src]

### 7.1 Frame geometry

```
query terminal size → w, h
if (h < 4 || w < 20) return;                # draw NOTHING; the previous frame stays
list_rows = h - 2                           # header + footer
if (list_rows < 1) list_rows = 1
```

[src] The tiny-terminal early return means a sufficiently small popup shows a stale (or,
at startup, blank alternate) screen and still responds to keys.

Scroll window (`top` = index of the first visible hit), applied in this order: [src]

```
if (sel < top)                        top = sel
if (sel >= top + list_rows)           top = sel - list_rows + 1
if (hits.size() <= list_rows)         top = 0
```

Frame layout: row 1 = header, rows 2..(h−1) = the list, row h = footer.

### 7.2 Header — every state

```
prompt = "regex> " + pattern
plen   = 7 + utf8_chars(pattern)             # CHARACTERS, not cells
slen   = utf8_chars(status)                  # CHARACTERS, not cells
emit  "\x1b[1m" + prompt + "\x1b[0m"
gap = w - plen - slen
if (gap > 0) { emit gap spaces; emit "\x1b[2m" + status + "\x1b[0m" }
emit "\r\n"
```

[src] If `gap <= 0` the status is **dropped entirely** — never truncated, never wrapped.

The `status` string, in priority order (first match wins): [src]

| condition | status text (verbatim) |
|---|---|
| `regcomp` failed | `invalid regex: ` + regerror text |
| pattern is empty | `type an extended regex` |
| hit list is empty | `no match` |
| capped | `<N>+ matches (capped)` |
| otherwise | `<N> matches` |

Then, **unconditionally prepended** if `#{alternate_on}` was non-zero at startup:

```
⚠ visible screen only · 
```

(U+26A0 WARNING SIGN, space, the words, space, U+00B7 MIDDLE DOT, space.) It is
prepended to whichever status was chosen, including the invalid-regex one. [src]

There is **no pluralisation**: a single hit renders as `1 matches`. Measured [meas].

Measured header lines [meas], `w = 60`:

```
regex>                                type an extended regex
regex> foo                                         3 matches
regex> zline                                      40 matches
regex> zzzz                                         no match
regex> (                    invalid regex: Unmatched ( or \(
regex> tcp                  ⚠ visible screen only · no match
```

### 7.3 List rows

Line-number column width, computed once per frame from the **capture size**, not from the
hits, so the text column does not jitter: [src]

```
numw = 1
for (v = lines.empty() ? 0 : lines.size() - 1;  v >= 10;  v /= 10)  ++numw
```

i.e. `numw` = number of decimal digits in `lines.size() - 1`.

For each of the `list_rows` rows, `idx = top + r`: [src]

- **`idx >= hits.size()`** → emit just `\r\n` (an empty row). Trailing empty rows are
  normal.
- otherwise:
  ```
  emit  (idx == sel) ? "\x1b[1m> " : "  "
  emit  "\x1b[2m"
  emit  snprintf("%*ld", numw, hit.line)          # right-aligned, space-padded
  emit  "\x1b[22m "
  text_w = w - numw - 3
  emit  render_line(lines[hit.line], hit.byte_start, hit.byte_end, text_w)   # §4.4
  emit  "\x1b[0m\r\n"
  ```

Note that the selected row's `\x1b[1m` is never closed before the end of the row, so the
**whole selected row renders bold**, including the (dim) line number region and the
highlighted match. Confirmed in a raw capture [meas]:

```
^[[1m> ^[[2m3^[[0m foo ^[[7mfoo^[[0m
```

`text_w` can go ≤ 0 on a narrow terminal, in which case `render_line` returns the empty
string (§4.4) and the row shows only the marker and the number. The `w < 20` early return
makes this rare but not impossible for a large `numw`.

The line number printed is the **capture index** (`hit.line`), 0-based from the top of
history — the same number the `rows` seam prints. It is not a "line N of the pane" number
in any user-facing sense, and there is no attempt to make it one.

### 7.4 Footer and cursor

```
emit  "\x1b[2m↑↓ select  Enter jump  Esc cancel  C-w word  C-u clear\x1b[0m"
emit  "\x1b[1;" + (plen + 1) + "H"
emit  "\x1b[?25h"
```

[src] Verbatim, including the two-space separators, `↑↓` = U+2191 U+2193, and the absence
of a trailing `\r\n` after the footer.

The cursor is parked at row 1, column `plen + 1` — i.e. just past the pattern — "so
typing looks normal". Because `plen` counts **characters**, a pattern containing wide
characters parks the cursor too far left by one column per wide character. Faithful
behaviour; reproduce it. [src]

### 7.5 No-match and empty-pattern states

Both render an entirely empty list (every row is a bare `\r\n`) with the header status
carrying the explanation, and the footer unchanged. Neither is an error state; neither
touches the pane; `Enter` in either state is a no-op (the `ENTER` branch breaks out when
the hit list is empty). Measured [meas]. [src]

---

## 8. Documented vs actual

Nine divergences. The source is the authority in every one.

1. **`Home` / `End` do not work.** The runbook's key table says
   *"`PgUp` / `PgDn`, `Home` / `End` — page, or jump to the first / last match"*. The
   decoder only recognises `ESC [ H`/`ESC O H` and `ESC [ F`/`ESC O F`; tmux's default
   terminal emulation sends `ESC [ 1 ~` and `ESC [ 4 ~`. Measured [meas]: pressing `Home`
   with pattern `foo` leaves the pattern `foo~` and the header at `no match`. `PgUp`/
   `PgDn` genuinely work. **The port should reproduce the runbook's promise only if told
   to; this spec's normative behaviour is the source's.** If the fix is authorised, it is
   the addition of `1~`, `4~`, `7~`, `8~` (and, for `Delete`, `3~`) to the CSI arm.

2. **`\w` and `\s` *are* available.** The runbook's troubleshooting says
   *"Perl-isms are not available: `\d`, `\w`, `\s` and lazy quantifiers are not
   extended-regex syntax."* Measured against glibc 2.41 [meas]: `\w`, `\W`, `\s`, `\S`,
   `\b`, `\<`, `\>` **all work** (GNU extensions enabled in glibc's
   `RE_SYNTAX_POSIX_EXTENDED`), and **backreferences work too** (`(a+)\1`). Only `\d` and
   lazy quantifiers are genuinely absent. The runbook's advice is safe but its factual
   claim is wrong, and a port that swaps in a non-glibc engine will change behaviour that
   users may already rely on.

3. **A resize cancels the search.** No document mentions this. The source *intends* to
   redraw on `SIGWINCH` (`g_resized` + a `draw()` branch), but `poll` is never restarted
   after a handler, so the interrupted `read_byte` returns −1 and `read_key` reports
   `K_ESC`, which is the cancel path. Measured twice with a control [meas]: idle 3 s →
   alive; `resize-pane` → process gone. The `g_resized` redraw branch is effectively dead.

4. **The alternate-screen header overstates the restriction.** The runbook says
   *"When the pane is on the alternate screen … there is no scrollback to search"*, and
   the header says `⚠ visible screen only`. Measured on 3.5a [meas]: with `less` running,
   `#{history_size}` reported **30** (not 0) and the identical `capture-pane -S -30 -E 11`
   returned **42** lines — the 30 pre-alternate history lines *plus* the 12 alternate-screen
   rows. So sift does search pre-alternate history, and the header's wording is
   pessimistic rather than descriptive. sift takes no other action on the alternate-screen
   flag: no different capture, no disabled jump.

5. **"cursor on the match start" is a consequence, not an assertion.** The runbook says
   *"After the jump the pane is in copy-mode with the cursor on the match start."* True,
   but only because `search-backward` seats the cursor there; sift's own positioning puts
   the cursor `char_end` characters right — *past* the match — and relies on the backward
   search to walk back. The verification then asserts `#{copy_cursor_x} == cell_start`.
   A port that skips the `search-backward` (or substitutes `search-forward`) breaks both
   the landing and the downstream `n`/`N` and `seek` grab keys.

6. **There is no per-line match cap.** The task brief and a casual reading of the source
   both suggest one; there is only the single global `kMatchCap = 20000`, checked after
   every push, which aborts the whole scan mid-line.

7. **`1 matches`.** The header never pluralises. Not documented anywhere; measured [meas].

8. **The atlas node's `:lines:` offsets are stale.** `tools/atlas/sift.org` cites
   `sift/src/main.cpp :lines 297-306`, `313-322` and `342-352`; the quoted code is at
   lines 323-332, 339-348 and 368-378 in the current file. The quoted *text* is accurate.
   Cosmetic, but a verifier following the line numbers will land in the wrong place.

9. **Verification-script assertion counts disagree between documents.** The runbook says
   `verify-sift-live.sh` carries *"5 assertions"*; the atlas node says *"6 assertions
   over real keystrokes"*. Not resolved here — neither script was read for this spec, and
   neither claim is load-bearing for the port.

Two things the documents get exactly right and that a port must not weaken: the
`display-popup` format-expansion trap and the `$TMUX_PANE` trap (§2.1), and the
`#{copy_cursor_line}` truncation trap (§2.5). Both are recorded in ADR-0005's Notes and
in the source comments, and both are the kind of thing a port "simplifies" away.

---

## 9. Port hazards, ranked

Ranked by (likelihood a naive Rust port gets it wrong) × (how badly the symptom misleads).

**1. Using Rust's `regex` crate instead of POSIX ERE.** Symptom: the hit list disagrees
with tmux's own search, so the highlight/`n`/`N` chain after the jump lands on matches the
list never showed — and, per ADR-0005, the whole architectural bargain (tmux performs the
search) breaks. Concretely: `a|ab|abc` on `xabc` gives `abc` under POSIX (leftmost-longest)
and `a` under `regex` (leftmost-first), changing the span, the highlight, `char_end` (so
the cursor seat) and possibly the hit count. `(a+)\1` compiles under glibc and is a hard
error under `regex`. `\d` works under `regex` and silently matches a literal `d` under
glibc. Mitigation: bind glibc's `regcomp`/`regexec` (or a POSIX-compatible engine); do
not substitute dialects.

**2. Not slicing the subject on continuation scans.** Symptom: `^` produces a hit at every
byte of every line, because Rust's `find_at` keeps `^` anchored at the real line start
only if you also pass the right anchor semantics — and conversely `\b`/`\<` produce
*fewer* hits than sift does, because `find_at` retains the preceding-character context
that sift's slicing throws away (measured: `\<a` over `aa a` → 3 hits in sift, 2 with
context). Mitigation: literally slice `&line[off..]` and set `REG_NOTBOL` equivalent.

**3. Skipping `setlocale` / getting `wcwidth` wrong.** Symptom: on any line containing CJK,
`cell_start` is computed as 1 cell per character instead of 2, the landing verification's
`#{copy_cursor_x}` check fails, and **every** jump on such a line reports
`sift: the pane moved — landed on the nearest match of /…/` even though it landed
perfectly. Secondary symptom: `.` matches one byte instead of one character, splitting
characters mid-sequence. Mitigation: reimplement an East-Asian-width table with the exact
three-case rule (≥1 as-is, 0 for combining, **1** for `wcwidth < 0`).

**4. Confusing bytes / characters / cells at the three consumer sites.** Symptom: a jump
that lands on the right *line* but the wrong *column* — silently, if the verification is
also ported with the wrong unit. `cursor-right -N` takes **characters**;
`#{copy_cursor_x}` reports **cells**; `regexec` reports **bytes**; the highlight span in
`render_line` is in **bytes**; the `rows` seam prints characters and cells but never
bytes. Mitigation: three distinct newtypes, not three `usize`s.

**5. "Fixing" `goto-line`.** Symptom: the pane lands `history_size - line` rows away from
the target, plausibly and consistently, so it looks like an off-by-N rather than a
misunderstanding. `goto-line N` sets the scroll offset `oy` from the bottom of history and
leaves the cursor row alone; `history-top` first is what makes `goto-line (H − L)` exact.
Also: re-read `history_size` at jump time, or a pane that printed while the popup was open
lands wrong.

**6. Retrying `EINTR` in `poll`.** Symptom: sift no longer exits when the popup is
resized. Arguably better, definitely different (§5.4). If the port keeps sift's behaviour,
`poll` returning −1 must map to the cancel path without inspecting `errno`.

**7. Advancing by characters, not bytes, on an empty match.** Symptom: `x*` on a CJK line
yields 3 hits instead of 7, and the `rows` seam's golden output no longer matches.
Mitigation: `off = if e == b { e + 1 } else { e }` on **byte** indices, and let the
char/cell counters round mid-character offsets up (§4.2).

**8. Not skipping empty capture lines.** Symptom: every blank line in the scrollback
produces a spurious hit for any empty-matching pattern, and the match count is inflated.
One `if line.is_empty() { continue }` before the scan loop.

**9. Building tmux commands as a shell string.** Symptom: no symptom at all until a
scrollback line — or a regex pattern, which is passed to `search-backward` — contains
`;`, `` ` ``, `$(`, or a newline, at which point arbitrary command execution or silent
corruption. The source states this as an invariant. Use `Command::arg` per element, with
the `;` separators as their own arguments.

**10. Returning a non-zero exit code, or panicking.** Symptom: tmux shows an error popup
over the user's pane on what should be a quiet no-op. Every path returns 0, including
"invalid regex", "no pane", "cannot read pane" and "usage". A Rust `main() -> Result` and
any reachable `unwrap()` both violate this. Note especially: parsing tmux's numeric fields
must behave like `strtol` (non-numeric ⇒ 0), not like `str::parse().unwrap()`.

**11. Dropping `-l` from `display-message`, or adding `-J`/`-e` to `capture-pane`.**
Symptoms respectively: pane text reaching a tmux format context (the proof obligation the
source discharges); wrapped lines joined so every index after the first wrap is off by
one; escape sequences matched by the regex and rendered raw.

**12. Emitting `\n` instead of `\r\n`.** Symptom: a staircase display. `OPOST` is cleared,
so the terminal does no translation. Rust's `println!`/`writeln!` emit `\n` — do not use
them for frame output.

**13. Not writing the frame as one `write`.** Symptom: visible tearing and cursor flicker
under a tmux popup. Build the whole `String`, write once.

**14. Restoring the terminal on signals.** Symptom: none visible, but a `Drop`-guard port
that also installs `SIGTERM`/`SIGINT` handlers changes behaviour and, given `ISIG` is
cleared, cannot receive `SIGINT` from the keyboard anyway. Match the existing coverage:
explicit restore on both normal exits plus an `atexit`-equivalent.

**15. Validating UTF-8.** Symptom: a panic or a mangled line on capture output containing
invalid UTF-8 (which tmux will happily produce). Operate on bytes throughout; reproduce
the permissive decoder including its `0xFFFD`/length-1 fallback and its lack of
overlong/surrogate checks.

**16. "Improving" the landing verification to compare text.** Symptom: every jump on a CJK
line falsely reports `sift: the pane moved`, because `#{copy_cursor_line}` truncates at the
first wide character. Keep it positional, and keep all five fields in one
`display-message` call.

**17. Adding pluralisation, a tail ellipsis, wrap-around selection, or cursor movement
inside the pattern.** Symptom: a verifier's golden output no longer matches. `1 matches`,
no tail ellipsis, no wrap, append-and-delete-only editing are all deliberate current
behaviour (§6.2, §4.4, §7.2).
