# graph.md — port `sift` from C++ to Rust

Carrier: `/graphrun` main loop. This file is the routing table; the loop routes by
reading it, never by improvising.

## 0. Size gate

**Above threshold** — (a), (c) and (d) all fail:
- (a) ≤3 agent calls: no. 800 LOC of termios/ANSI/POSIX-regex TUI needs an independent
  behavioural spec, a two-stage port (non-interactive slice, then TUI), and an
  independent verifier that is not the porter.
- (c) crosses into a side effect: yes — the port replaces a **live `prefix /` keybinding**
  and lands on the same output path (`tools/target/release/sift`) that the C++ binary
  occupies, then deletes the C++ source.
- (d) human gate: yes — two. The regex-dialect/dependency choice changes user-visible
  search semantics, and the swap reverses a **recorded user directive** (`~/CLAUDE.md`:
  "sift, which is C++ and builds with cmake, not cargo (user directive, 2026-08-27)")
  across a second git repo.

Single-call insufficient because behaviour equivalence needs a verifier with a fresh
context and a spec it did not author; one call would grade its own port.

## 1. Diagnosis

- **Stages**: (spec ∥ baseline) → [G1 deps] → crate-init → port-core → verify-core →
  port-tui → verify-tui → [G2 swap] → integration → use-node
- **Side effects**: crate-init (new files under `tools/sift/`), port-core / port-tui
  (Rust sources), integration (deletes `tools/sift/src/main.cpp` + `CMakeLists.txt`,
  edits `claude.conf`, `tmux.conf`, `runbooks/sift.md`, `docs/adr/0005-*.org`,
  `tools/ARCHITECTURE.org`, `tools/atlas/{index,sift,text-piping}.org`,
  `~/.tmux/CLAUDE.md`?, and **`~/CLAUDE.md` in the separate `$HOME` dotfiles repo**).
  Irreversible-ish: the C++ source deletion (recoverable from git) and the binary
  overwrite at `tools/target/release/sift` (**not** recoverable — same path as cargo's
  output; see §3 external state).
- **Approval points**: G1 dependency/regex-dialect decision (same-session, minutes);
  G2 swap — delete C++, rewrite bootstrap docs in two repos (same-session).
- **Verification points**: verify-core and verify-tui sit between each port stage and
  the swap; use-node sits after integration, on the final deliverable.
- **Shared state**: `spec`, `depsDecision`, `coreFindings`, `tuiFindings`, `swapDiff`.
- **Cycles**: verify-core → port-core repair (cap 3); verify-tui → port-tui repair
  (cap 3). Two independent loops, two counters.
- **Scale / caps**: 8 planned dispatches, ceiling ~12 with repairs. Per-node time
  bounds in §5. **No hard cost bound** — no `+<N>k` directive was given.
- **Tier fallback**: every opus node falls back to sonnet.
- **Walking skeleton**: port-core is the thinnest end-to-end slice — the
  non-interactive `sift rows <pane> <regex>` path plus the jump arithmetic, which the
  existing harness `verify-sift-jump.sh` exercises with no TTY. The TUI is scale-out
  and comes only after verify-core passes.

Shapes: **diamond** (spec ∥ baseline → G1; whole-set need = the gate payload needs both
the regex-flag inventory and the golden baseline before the user can decide), then
**chain** with two **controlled cycles** and two **gates**.

## 2. Carrier

**Graph in the main loop** (`/graphrun`). The one fact that chose it: the run is one-shot
and its riskiest joints (does a hand-ported TUI behave identically?) cannot have their
failure modes enumerated in advance — adaptive joints beat pre-declared paths here. No
replay or cross-session resume is needed.

File carrying the graph: **`~/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/`**
(`graph.md` + `state.md` + `nodes/<id>/prompt.md`). Untracked scratch in the `.tmux`
repo; delete or gitignore it after the run.

## 3. State schema

### Message-passing state

| key | type | written by | read by | merge |
|---|---|---|---|---|
| `spec` | markdown doc | spec | G1, port-core, port-tui, verify-core, verify-tui, integration | single writer |
| `baseline` | golden outputs + `sift-cpp` binary | baseline (transform) | G1, verify-core, verify-tui, use-node | single writer |
| `depsDecision` | `{regex, termios, width}` | G1 (user) | crate-init, port-core, port-tui | never written by agents |
| `coreFindings` | `{id, axis, severity, evidence}[]` | verify-core | route, port-core (repair) | single writer per round |
| `tuiFindings` | same shape | verify-tui | route, port-tui (repair) | single writer per round |
| `swapDiff` | staged diff | integration (staged) | G2 | single writer |

### External state

| artifact | written by | read by | volatility |
|---|---|---|---|
| `tools/sift/src/main.cpp`, `CMakeLists.txt` (C++) | integration (deletes) | spec, port-core, port-tui, verify-core, verify-tui | stable within run |
| `tools/sift/{Cargo.toml,src/*.rs}` (Rust) | crate-init, port-core, port-tui | verify-core, verify-tui, use-node | stable within run |
| `tools/Cargo.toml` (workspace members, profiles) | crate-init | — | stable within run |
| `tools/.cargo/config.toml` (rust-build-speed) | crate-init | — | stable within run |
| **`tools/target/release/sift`** | crate-init, port-core, port-tui (cargo) | verify-core, verify-tui, use-node | **VOLATILE — collision.** cmake wrote the C++ binary to this exact path; the first `cargo build` overwrites it. `baseline` MUST copy it to `<run>/baseline/sift-cpp` before any cargo build. |
| `records/2026-08-27-2240-tmux-sift/assets/scripts/*.sh` | — (read-only) | baseline, verify-core, verify-tui, use-node | stable |
| `claude.conf`, `tmux.conf`, `cheat.txt`, `runbooks/sift.md`, `docs/adr/0005-*.org`, `tools/ARCHITECTURE.org`, `tools/atlas/*.org` | integration | — | stable |
| **`~/CLAUDE.md`** (separate `$HOME` git repo) | integration | — | stable |
| the live tmux server | baseline, verify-*, use-node (throwaway sockets only) | same | **volatile** — pane ids, pids, socket counts are never compared for equality |

REVISION set (read-by of every mutated row): port-core, port-tui, verify-core,
verify-tui, use-node. (No prefix cache under this carrier, so it feeds only the
mid-run revision reset of Step 4, not a cache token.)

## 4. Mutation set

| node | effect | gated? |
|---|---|---|
| `crate-init` | creates `tools/sift/Cargo.toml`, `src/`, `tools/.cargo/config.toml`; edits `tools/Cargo.toml` members + profiles | behind G1 |
| `port-core` | writes Rust sources under `tools/sift/src/` | behind G1 |
| `port-tui` | writes Rust sources under `tools/sift/src/` | behind G1 |
| `integration` | deletes the C++ source; edits config + docs in **two** git repos | behind G2 |

Everything else — `spec`, `verify-core`, `verify-tui`, `use-node` — is **read-only by
construction**. `baseline` is a transform run by the orchestrator; it writes only inside
the run dir. No node commits, pushes, or runs `cargo clean` (which would delete the C++
binary; the baseline copy is the protection).

## 5. Nodes and edges

| id | class | model → fallback | subagent_type | bound | reads | writes |
|---|---|---|---|---|---|---|
| `spec` | judgment | opus → sonnet | general-purpose | 15 min | C++ source, runbook, ADR-0005, atlas | `nodes/spec/spec.md` |
| `baseline` | **transform** (orchestrator) | — | — | 5 min | cmake build, verify scripts | `baseline/sift-cpp`, `baseline/*.txt` |
| `G1 deps` | gate | — | — | — | spec + baseline | `nodes/.../staged.md` |
| `crate-init` | plumbing | sonnet | general-purpose | 10 min | spec, depsDecision, `rust-build-speed` skill | crate skeleton + build config |
| `port-core` | judgment | opus → sonnet | general-purpose | 25 min | spec, C++ source, crate | `src/*.rs` (rows/jump path) |
| `verify-core` | judgment | opus → sonnet | general-purpose | 15 min | spec, baseline, built binary | findings only (read-only) |
| `port-tui` | judgment | opus → sonnet | general-purpose | 30 min | spec, C++ source, crate | `src/*.rs` (TUI) |
| `verify-tui` | judgment | opus → sonnet | general-purpose | 20 min | spec, baseline, built binary | findings only (read-only) |
| `G2 swap` | gate | — | — | — | staged diff | `nodes/integration/staged.md` |
| `integration` | plumbing | sonnet | general-purpose | 20 min | spec, depsDecision | docs + config, two repos |
| `use-node` | plumbing | sonnet | general-purpose | 10 min | **only** the built binary + `runbooks/sift.md` | its own result |

Judgment prompts go through `agentic-prompt-composer` **only after** the walking
skeleton (spec → crate-init → port-core → verify-core) has returned once; before that
they use graph-engineering's skeleton prompt.

### Routing (plain functions over domain-fact fields)

```
route(afterVerifyCore):
  if !coreFindings                      -> 'abort'        // unparseable / dead node
  blocking = coreFindings.filter(f => f.severity === 'biases-deliverable')
  if blocking.length && CORE_ROUND < 3  -> 'repair-core'
  if blocking.length                    -> 'halt:core-cap'
  return 'port-tui'
  // fall-through of every unmatched state -> 'abort'

route(afterVerifyTui):   same shape, TUI_ROUND, -> 'G2' on clean
route(afterUseNode):     useNodeMatchesBaseline === true ? 'done' : 'halt:use-node-fail'
```

Every fall-through lands on halt/abort, never on proceed. `severity` is a domain fact the
verifier observed, not a stage name.

## 6. Joint contracts

| edge | artifact | failure path |
|---|---|---|
| `spec → G1` | markdown spec with: CLI surface, key map, regex flags (`REG_EXTENDED`, `REG_NOTBOL`, byte offsets), width/locale rules, tmux commands issued, exit codes | **abort** (load-bearing: the gate payload and both verifiers depend on it) |
| `baseline → G1` | `baseline/sift-cpp` exists and is executable; `verify-sift-jump.sh` pass/fail counts recorded | **abort** (without a golden baseline nothing downstream can be compared) |
| `G1 → crate-init` | `{regex, termios, width}` chosen by the user | fail-closed: no explicit choice → halt |
| `crate-init → port-core` | `cargo build -p sift` succeeds on a stub | **retry once**, then abort |
| `port-core → verify-core` | binary at `tools/target/release/sift` + a changed-files list | **retry once** (attempt cap 2), then halt that branch |
| `verify-core → route` | `{findings: [{id, axis, severity: 'biases-deliverable'\|'latent', evidence}]}` | unparseable → **abort** (a verifier that cannot be read has not verified) |
| `port-tui → verify-tui` | as port-core | **retry once**, then halt |
| `verify-tui → G2` | as verify-core | **abort** on unparseable |
| `integration → G2` | staged diff of every file, **no file mutated yet** | **abort** on empty diff |
| `use-node → done` | its `sift rows` output for the fixture pattern | **abort** on null |

Every node prompt ends: *"on failure write NO artifact — report it in the result block
only; return a terminal result, no backgrounded self-checks."*

**Hang case**: one backgrounded timer per wave, armed at the shortest remaining bound;
on wake, any `dispatched` row past its bound is `TaskStop`ped and marked
`failed(timeout)`.

## 7. Cycle bounds

Two independent loops, two counters, never shared. **No cache token** — Agent dispatch
has no prefix cache, so the token half is vacuous (graphrun Step 2).

| loop | counter | cap | progress metric (computed by the orchestrator, not claimed by an agent) | cap gate |
|---|---|---|---|---|
| verify-core → port-core | `CORE_ROUND` | 3 | blocking-finding count strictly decreasing round over round | `halt:core-cap` — user decides |
| verify-tui → port-tui | `TUI_ROUND` | 3 | same | `halt:tui-cap` |

Attempt cap per node is 2 (one dispatch + one retry) and is a *different* quantity from
the round counter: a node that returns garbage twice halts its branch; a node that
returns a clean result the verifier rejects advances the round counter.

## 8. Gate plan

| gate | pattern | horizon | fail-closed condition | payload |
|---|---|---|---|---|
| **G1 deps** | 2 + 3 (staged + explicit approval) | same-session | no explicit option chosen → halt; nothing under `tools/sift/` is created | the spec's regex/termios/width inventory, the three candidate stacks with the exact behaviour each preserves or changes, and the baseline pass counts |
| **G2 swap** | 2 (staged mutation) + 3 | same-session | approval absent or partial → halt; the staged diff is discarded, C++ source untouched, `~/CLAUDE.md` untouched | the full diff: C++ deletion, `claude.conf` build-stub rewrite, `~/CLAUDE.md` directive reversal, ADR-0005 amendment, runbook + atlas updates |

G2 is layered on Pattern 2 by construction: `integration` writes its diff to
`nodes/integration/staged.md` and mutates nothing until approval, so even a gate failure
leaves the working tree intact.

## 9. Goal condition

> With the C++ source removed, `cd ~/.tmux/tools && cargo build --release` alone
> produces `tools/target/release/sift`, and
> `SIFT=~/.tmux/tools/target/release/sift bash records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-jump.sh`
> exits 0 with a pass count **equal to** the C++ baseline's and zero failures — and the
> use-node's `sift rows` output on the fixture is **byte-identical** to
> `baseline/rows-golden.txt`.

Structural fields compared for equality: pass count, fail count, `sift rows` bytes,
binary path. Volatile fields (pane ids, pids, socket paths, timings) are informational
only — never equality-compared.

## 10. Budget and tiers

- **No hard cost bound** — no `+<N>k` token directive was given.
- Agent-count sizing (enforced by tally in `state.md`): **8 planned dispatches**,
  ceiling **12** with repairs. Crossing 150% of 12 = **18** stops the run for an
  explicit go-ahead.
- Tiers: judgment (`spec`, `port-core`, `port-tui`, `verify-core`, `verify-tui`) opus
  → sonnet on quota; plumbing (`crate-init`, `integration`, `use-node`) sonnet.
- Runaway-prone node caps (**advisory** — stated in prompts, not enforced):
  `port-tui` ≤ 8 cargo invocations; every verifier ≤ 6 harness runs.
- Composer: loaded for `port-tui`, `verify-tui` only, after the skeleton returns.

## 11. Sanity gate

`graph sanity gate: passed` — with four fixes made during the walk:

1. **Binary-path collision found and routed.** The first draft compared the Rust
   binary against "the C++ binary at `tools/target/release/sift`" — but cargo writes to
   that same path, so the first build destroys the baseline. Fixed: `baseline` copies
   it to `<run>/baseline/sift-cpp` before any cargo invocation, and the external-state
   table marks the path volatile.
2. **Verifier scale-down declared, not hidden.** The skill wants a per-axis diamond for
   a large artifact; 800 LOC with a mechanical harness does not warrant one, so
   `verify-core`/`verify-tui` stay single nodes — but each carries an explicit axis list
   (regex parity · width/CJK rendering · key handling · tmux side effects · exit codes)
   and severity-typed findings, so only blocking findings abort.
3. **Goodhart check.** Plausible wrong deliverable that still passes: a Rust port that
   satisfies `verify-sift-jump.sh` (which only exercises `sift rows` and the jump
   arithmetic) while the *interactive* refilter, CJK cell-width rendering, or
   `REG_NOTBOL` overlapping-match behaviour is wrong. Strengthened: `verify-tui` is a
   separate node with the TUI axes named above, and the use-node's ground truth is a
   byte comparison against the C++ golden output rather than a pass/fail.
4. **Cross-repo mutation surfaced.** `~/CLAUDE.md` lives in a different git repo and
   records the "C++, not cargo" user directive this run reverses. It is now an
   external-state row and an explicit item in G2's payload, not an afterthought in
   integration's prompt.

Also checked and sound: one writer per key; every edge carries data the downstream node
reads; no LLM-as-router; generator ≠ verifier at both verify stages; both cycles bounded
with an orchestrator-computed metric; both gates fail closed; the hang case has a real
timer.

---

## REVISION 1 — 2026-08-31 22:40, after G1 approval

**What changed.** G1 resolved: `depsDecision = {regex: libc regcomp/regexec, termios:
libc, width: libc wcwidth}` — libc FFI throughout, behaviour-identical by construction.
And: **faithful bug-for-bug port, the two measured bugs fixed only after equivalence is
proven.**

**Why.** The user chose to keep the equivalence comparison free of known-divergence
carve-outs. That requires the fixes to land *after* both verifiers pass, but *before*
G2 — otherwise the swap ships a port whose fixes were never verified, and the runbook's
key table cannot be corrected in the same diff.

**Nodes inserted** between `verify-tui` and `G2 swap`:

| id | class | model → fallback | bound | does |
|---|---|---|---|---|
| `fix-bugs` | judgment | opus → sonnet | 15 min | fix `Home`/`End` (`ESC[1~`/`ESC[4~`) and the SIGWINCH→EINTR cancel; nothing else |
| `verify-fixes` | judgment | opus → sonnet | 10 min | confirm both fixed AND the harness still matches the baseline 13/0 — a regression check, not a re-audit |

Sizing updated in §10: **10 planned dispatches, ceiling 14, stop-and-ask at 21.**
No nodes reset — nothing downstream had run.

## REVISION 2 — 2026-08-31 22:40, live-binding hazard

**What changed.** A hazard the §3 collision row implied but did not spell out: `claude.conf`
binds `prefix /` directly at `tools/target/release/sift`. The moment cargo builds the
`sift` package, **the user's live keybinding runs the half-finished Rust binary.**

**Mitigation, binding on every node from `crate-init` through `verify-fixes`:**

- Every build and every harness run uses
  `CARGO_TARGET_DIR=<run>/target-dev` — the real `tools/target/release/sift` is not
  touched until `integration`.
- **No node runs a bare `cargo build --release` in `tools/`**, and **no node runs
  `cargo clean`** (which would delete the C++ binary as well — the documented cost of
  the shared output directory).
- Restore path if it happens anyway: `cp <run>/baseline/sift-cpp tools/target/release/sift`.
  The rescue copy doubles as the restore.

**Also flagged to `port-tui`:** `[profile.release]` sets `panic = "abort"`. A Drop-based
termios restore will **not** run on a panic. The C++ used a signal handler plus an
explicit cooked() path; the Rust port needs an equivalent that survives `panic = abort`
(panic hook or `libc::atexit`), not RAII alone.

## REVISION 3 — 2026-08-31 23:12, routing the `jump()` coverage gap

**What changed.** `verify-core` found that `verify-sift-jump.sh` — the harness carrying
the goal condition's 13/0 — **re-implements the jump with raw `tmux` commands (lines
63-69) and never calls sift's own `jump()`.** Confirmed from disk by the orchestrator.
So the port's `jump()` was, at that point, entirely unexecuted: a limitation recorded in
a deliverable is an unassigned to-do, not a disclosure, and it must be routed.

**How it is routed.** `verify-sift-live.sh` *does* drive the real path — it launches sift
in a window, types keys into it, presses Enter and asserts
`#{copy_cursor_x}|#{search_present}|#{pane_in_mode}` on the target pane. It needs the TUI,
so it could not have run before now.

- **New baseline captured**: `baseline/live-golden.txt` — C++ scores **passed 6, failed 0**.
- **§9 goal condition amended**: the Rust binary must ALSO score 6/0 on
  `verify-sift-live.sh`, not only 13/0 on `verify-sift-jump.sh`.
- **`verify-tui` acceptance is now mandatory on this point** — it must run that harness
  and it may not pass the node without it.

**Also carried forward** to `port-tui` as an inherited finding, not a rediscovery:
verify-core's **L5** — `say` takes `&str` where the C++ takes a byte-transparent
`std::string`; flagged there explicitly as a forward hazard for the TUI node.

Latent residuals L1-L6 are **not** waived here. They are persisted and go to G2 as an
explicit residual waiver — the swap gate cannot be approved by silence on them.

## REVISION 4 — 2026-09-01 00:05, live test 6 measures the wrong binary

**What changed.** `port-tui` reported honestly that `verify-sift-live.sh` test 6 —
"the REAL binding, driven by a real prefix keypress" — does `source-file ~/.tmux/claude.conf`,
and that binding points at `~/.tmux/tools/target/release/sift`, which is still the **C++**
binary. Confirmed from the harness source by the orchestrator.

So test 6 is not a false pass; it is an **uninformative** one. It passed identically for
the C++ baseline and the Rust run because both times it ran the same C++ binary. Of the
live harness's 6 assertions, **5 exercise the port** and 1 does not.

**How it is routed.** Test 6 becomes informative only once `integration` puts the Rust
binary on the real path. Therefore:

- §9 goal condition amended again: after `integration`, `verify-sift-live.sh` is re-run
  by the orchestrator, and test 6 must pass **with the Rust binary behind the binding**.
  Until that run, no claim may be made that the real `prefix /` binding drives the port.
- Recorded as a **known measurement limitation** in `verify-tui`'s brief so it is not
  rediscovered and not silently inherited as evidence.

This is the same class as the `jump()` gap in REVISION 3: a harness whose green light
came from something other than the thing under test.

## REVISION 5 — 2026-09-01 01:05, integration runs staged-then-applied

**What changed.** §1's stage list wrote `[G2 swap] → integration`, but the gate's payload
IS integration's diff — the gate cannot precede the node that produces what it gates.
§8 already required Pattern 2 ("integration writes its diff and mutates nothing until
approval"); this makes the ordering explicit rather than contradictory.

**Resolved ordering**: `integration` runs in **staged mode** — it writes the complete new
content of every affected file into `nodes/integration/staged/` plus a `MANIFEST.tsv` of
actions, and touches **no** real file. `G2` then shows the diff. Only on approval does the
**orchestrator** apply the manifest and run the real build. Integration therefore never
holds write permission on the working tree; the mutation set's `integration` row moves to
the orchestrator's post-approval apply step.

This keeps Pattern 2's property intact: if the gate fails for any reason, nothing
irreversible has happened.

## REVISION 6 — 2026-09-01 01:40, the goal condition compared a VOLATILE field

**What the use-node exposed.** Its `rows` output was **not** byte-identical to
`baseline/rows-golden.txt`. Investigated rather than reported:

- Fields 2-5 (`char_start`, `char_end`, `cell_col`, `text`) are **identical on every one
  of the 135 rows**.
- Field 1, the scrollback line index, differs by a **constant 1** on every row.

That is a pane-state offset — the use-node built its own tmux pane, whose scrollback had
one fewer preceding line than the pane the golden was captured from. It is not a
match-arithmetic difference.

**The defect is in §9, not in the port.** §3's external-state table already classified the
live tmux server as **volatile** and said its pane state may never be equality-compared.
§9 then required the use-node's `rows` output to be byte-identical — which includes a
scrollback line index that is exactly such a value. I wrote a check that compares a
volatile field for equality, the failure mode this graph's own design rules name.

**Corrected goal condition** (the comparison actually performed):

- **Structural, equality-compared**: fields 2-5 of every `rows` row, the harness pass
  counts, the binary's path. → identical / 13-0 / 6-0 / correct.
- **Volatile, offset-compared**: field 1, the scrollback line index. → constant delta of
  1 across all 135 rows, consistent with a one-line pane-state difference.

Under the corrected condition the goal is **met**. Recorded as a design error found by
the use-node, which is precisely the node type the skill says catches "true but not
enough" — here it caught "checked, but checking the wrong thing".
