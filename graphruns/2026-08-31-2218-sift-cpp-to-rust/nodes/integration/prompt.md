Produce — **as a staged patch set, mutating nothing** — the complete documentation and
configuration change that accompanies replacing a C++ tool with a Rust one.

## CRITICAL: you have no write permission on the working tree

You must **not** edit, create, or delete any file under `/home/fenrir/.tmux/` (except
inside your staging directory) or anywhere else in the user's home. A human gate reviews
your output before anything is applied. Writing directly would defeat the gate.

**How to stage instead.** For every file you would change, write its complete new content
to `/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/nodes/integration/staged/<path-with-slashes-replaced-by-percent>`, e.g.

- `/home/fenrir/.tmux/runbooks/sift.md`  →  `staged/%home%fenrir%.tmux%runbooks%sift.md`
- `/home/fenrir/CLAUDE.md`               →  `staged/%home%fenrir%CLAUDE.md`

and append one line to `/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/nodes/integration/staged/MANIFEST.tsv`:

```
<action>\t<absolute target path>\t<staged filename or ->\t<one-line why>
```

`<action>` is `modify`, `create`, or `delete`. For `delete`, the staged filename is `-`.

## The change

`sift` was a C++ tool built by cmake, deliberately outside the cargo workspace. It is now
a Rust crate — a workspace member built by `cargo build --release` like every sibling. The
port is complete and verified:

- `libc` FFI throughout (`regcomp`/`regexec` with `REG_EXTENDED`, `wcwidth`, `termios`,
  `poll`) — so **search semantics are unchanged**: POSIX ERE, leftmost-longest,
  backreferences and `\<`/`\>` work, `\d` is still a literal `d`.
- `verify-sift-jump.sh` 13/0 and `verify-sift-live.sh` 6/0, matching the C++ baseline;
  `rows` output byte-identical across 26 regex patterns; 26 differential rendering
  comparisons identical; three adversarial audits, 0 blocking findings.
- **Two long-standing bugs were then fixed** (this is new user-visible behaviour):
  - `Home`/`End` used to type a literal `~` into the pattern; they now jump to the
    first/last match, as the runbook always claimed.
  - Resizing the popup used to **cancel the search**; it now redraws at the new size.

## Files you must cover

Read each, then stage its replacement. Do not guess at content you have not read.

**In `/home/fenrir/.tmux/`:**
1. `claude.conf` — the header comment (lines ~10-14) describing the two build systems, the
   `origin_pane()` reference to `tools/sift/src/main.cpp` (~line 205), and **the not-built
   stub message (~line 211)**, which currently tells the user to run cmake. That stub is
   load-bearing: it is what a fresh clone sees.
2. `tmux.conf` — the comment block at ~lines 75-77.
3. `cheat.txt` — line ~16 if it needs it.
4. `runbooks/sift.md` — the source-file link, the whole build section (~lines 43-60), and
   **three content corrections**:
   - the key table must stop implying `Home`/`End` were ever broken — they now work;
   - document that resizing the popup now redraws instead of cancelling;
   - the runbook currently states `\d`, `\w`, `\s` are all unavailable. **Measured on
     glibc 2.41 this is wrong**: `\w \W \s \S \b \< \>` all work (GNU extensions), and
     backreferences work. Only `\d` and lazy quantifiers are genuinely absent. Fix it.
5. `docs/adr/` — existing ADRs are 0001-0005; 0005 is "own the interaction loop for regex
   search". The language/build change is a **different** decision from that one, so
   create `0006-…` rather than rewriting 0005. Match the existing ADRs' org format and
   heading structure exactly — read two of them first. Record the real reasoning: why
   Rust, why `libc` FFI rather than the `regex` crate (dialect fidelity — tmux performs
   the actual search after the jump, so the hit list must agree with tmux's `n`/`N`), and
   what it costs (one dependency, `unsafe` blocks). Note that 0005's architectural bargain
   is preserved, not revisited. Add a short pointer in 0005 if the ADR house style uses
   supersession/related links — check before inventing one.
6. `tools/ARCHITECTURE.org` — lines ~63-74 describe the cmake divergence as "a user
   directive, not a technical" one, and the shared output directory. That divergence is
   over.
7. `tools/atlas/index.org` (~line 29: "8 nodes over 16 source files … plus the one
   non-Rust tool"), `tools/atlas/sift.org`, `tools/atlas/text-piping.org`. **The port spec
   found the atlas's `:lines:` citations for sift are stale by ~26 lines** — and the file
   is now `main.rs` at 1519 lines, so every citation needs recomputing against the new
   source, not merely shifting.
8. `CLAUDE.md` (the `.tmux` one) — line ~6, "eleven Rust crates plus `sift`, which is C++".
9. `tools/sift/src/main.rs` — **one comment fix only, no code**: the module header at
   lines ~34-36 still says "**This is a bug-for-bug port.** … nothing here is 'improved'".
   That was true when written and is now false — two bugs were deliberately fixed. Rewrite
   that paragraph to say what is now true: faithful except for two named, verified fixes.
10. **Delete** `tools/sift/src/main.cpp` and `tools/sift/CMakeLists.txt` (manifest
    `delete` lines; they stay in git history).

**In `/home/fenrir/` — a DIFFERENT git repo, treat with care:**
11. `CLAUDE.md` — lines ~24, ~29, ~32 and ~137. This file records **"plus `sift`, which is
    C++ and builds with cmake, not cargo (user directive, 2026-08-27)"**. This change
    reverses that recorded directive, with the user's explicit approval given today. Say
    so honestly — do not silently delete the old directive as if it never existed; the
    house style in these files is to record what changed and when. Also: the fresh-clone
    bootstrap (step 4) currently requires **two** build commands and names a C++20
    compiler and cmake as dependencies. After this change there is **one** command and
    those two dependencies are gone. And the warning that "`cargo clean` deletes `sift`
    too" is now obsolete — `cargo clean` + `cargo build` simply rebuilds it.

## Style

Match each file's existing voice — these documents explain *why*, cite measurements, and
record costs, not just instructions. Where you state a fact that came from a measurement
in this port (the 13/0, the byte-identical differential, the glibc regex findings), it is
appropriate to say it was measured. Do not invent numbers; every figure you cite must come
from this brief or from a file you read. Follow the repo's cross-reference style:
markdown/org links, repo-relative, descriptive labels.

## Do not

- Do not touch the working tree. Staging directory only.
- Do not run any build, and do not run `cargo` at all.
- Do not commit anything.
- Do not modify the harness scripts under `records/`.

## Output contract

```result
status: ok | failed
manifest_path: /home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/nodes/integration/staged/MANIFEST.tsv
files_staged: <count>
files_deleted: <count>
adr_created: <path or ->
home_claude_md_handled: <how you recorded the directive reversal>
working_tree_untouched: <yes|no>
cmake_references_remaining: <your own grep for cmake/C++/main.cpp across the staged set — should be only historical mentions, list them>
notes: <one line; on failure, why>
```

On failure write NO artifact — report it in the result block only. Return a terminal
result: do not background any self-check and do not end your turn waiting on anything.
