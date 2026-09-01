Initialise a Rust crate that will replace an existing C++ tool. You are creating the
**build scaffolding only** — a crate that compiles and does nothing useful yet. A later
engineer writes the actual implementation. Do not implement the tool.

## Read first

`~/.claude/skills/rust-build-speed/SKILL.md` — the build-configuration guidance you must
apply. Read it before you write any TOML.

## The workspace

`/home/fenrir/.tmux/tools` is a cargo workspace of 11 small binary crates plus one C++
tool (`sift`) built by cmake. You are adding `sift` as the 12th cargo crate. Read
`/home/fenrir/.tmux/tools/Cargo.toml` and one existing member (`seek/Cargo.toml`) so the
new crate matches house style exactly — workspace inheritance, not duplicated literals.

## What to create

1. `tools/sift/Cargo.toml` — package `sift`, version/edition/rust-version inherited from
   `[workspace.package]` like every sibling. One dependency: **`libc`**, declared in
   `[workspace.dependencies]` and referenced as `libc = { workspace = true }`, matching
   how `unicode-width` is handled today. Pin it the way the workspace pins its other
   deps (a bare major, e.g. `"0.2"`, if that is the house pattern — check).
2. Add `"sift"` to the workspace `members` list, in a position consistent with the
   existing ordering.
3. `tools/sift/src/main.rs` — a stub that compiles: parse nothing, print nothing useful,
   just enough to prove the build works. Put a one-line comment saying it is a
   scaffold awaiting the port.
4. **Build configuration, per the skill.** Two specific judgements you must honour:
   - **Do NOT touch `[profile.release]`.** It is deliberately tuned for a shipped
     binary (`opt-level="s"`, `lto=true`, `codegen-units=1`, `panic="abort"`, `strip=true`).
     The skill's rule is to stop using the shipping profile for iteration, not to weaken it.
   - **Add a `[profile.quick]`** exactly as the skill's §1 prescribes — `inherits =
     "release"`, `lto = false`, `codegen-units = 16`, `incremental = true` — with a
     comment noting the skill's measured warning that `codegen-units = 16` **regressed**
     a build 48% when left alone with fat LTO, so those two lines travel together.
   - **Do NOT add a linker override (`mold`/`lld`) in `.cargo/config.toml`.** The skill
     records a *measured null result* on a workspace of exactly this shape — 11 small
     binaries, relinked every rebuild — where mold was 0.28s → 0.38-0.68s, i.e. slightly
     **slower**. Adding it would also put a new binary in the fresh-machine bootstrap.
     If you believe some other `.cargo/config.toml` setting from the skill earns its
     place here, you may add it, but you must justify it in your result block against a
     measurement, not a preference.

## Hard constraints — violating any of these breaks the user's live environment

- **Build only with `CARGO_TARGET_DIR=/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/target-dev`.** The tmux key binding
  `prefix /` runs `/home/fenrir/.tmux/tools/target/release/sift` directly; writing a stub
  there would break the user's live keybinding.
- **Never run a bare `cargo build --release` in `tools/`**, and **never run `cargo clean`**
  (it deletes the C++ binary too — the documented cost of the shared output directory).
- Do not touch `tools/sift/src/main.cpp` or `tools/sift/CMakeLists.txt`. They stay until
  a later gated step removes them. The C++ file lives at `src/main.cpp` and your Rust
  file at `src/main.rs`; both can coexist in that directory for now.
- Do not modify any other crate, any `.org`/`.md` doc, `claude.conf`, or `tmux.conf`.
- Do not commit anything.

## Prove it

Run, and paste the real output into your result block:

```
cd /home/fenrir/.tmux/tools
CARGO_TARGET_DIR=/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/target-dev cargo build --release -p sift
CARGO_TARGET_DIR=/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/target-dev cargo build --profile quick -p sift
```

Then confirm `/home/fenrir/.tmux/tools/target/release/sift` is **still the C++ binary** —
compare its sha256 against `/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/baseline/sift-cpp`. They must be identical. If they
are not, say so loudly: you clobbered the live binary and it must be restored from the
baseline copy.

## Output contract

End your reply with a fenced result block, exactly this shape:

```result
status: ok | failed
files_created: <comma-separated paths>
files_modified: <comma-separated paths>
release_build: <pass|fail>
quick_build: <pass|fail>
live_binary_intact: <yes|no — sha256 match against baseline/sift-cpp>
cargo_config_added: <none | path + the measurement that justifies it>
notes: <one line; on failure, why>
```

On failure write NO artifact — report it in the result block only. Return a terminal
result: do not background any self-check and do not end your turn waiting on anything.
