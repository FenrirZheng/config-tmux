# graph.md — sift ordinal selection (t2/t3/t4)

Design artifact per `~/.claude/skills/graph-engineering/references/design-artifact.md`.
Carrier: **main loop (`/graphrun`)**. This file is the routing table; the loop routes by
reading it, never by improvising.

---

## 0. Size gate

**Above threshold.** (a) fails: ≥ 8 agent calls planned. (c) fails: the output crosses
into a side effect (a modified `sift` binary the user's `prefix /` runs, and edits to two
verification suites that gate future work) and needs an independent verifier — the
implementing agent must not approve its own C++. (d) fails: a commit gate. Only (b) holds.

## 1. Diagnosis

- **Stages** (enumerable): baseline → impl-a (ordinal column) → impl-b (ordinal mode)
  → verify-impl → { atlas ∥ tests → run-suites → docs → use-node } → map-close → gate → commit.
- **Side effects**: repo file writes (`tools/sift/src/main.cpp`, `tools/atlas/sift.org`,
  `records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-{live,jump}.sh`,
  `runbooks/sift.md`, `records/2026-09-01-1330-sift-ordinal-selection/sift-ordinal-selection.org`);
  a rebuilt `tools/target/release/sift` (the binary `prefix /` actually runs); throwaway
  tmux servers (`-L sift_live`, `-L sift_verify`, `-L sift_live_outer`, `-L sift_usenode`).
  **Irreversible-ish**: the git commit. Everything else is a working-tree edit — reviewable
  with `git diff`, revertable with `git checkout`.
- **Approval points**: one — **G1, the commit gate**, same-session horizon. Every file edit
  is staged mutation (Pattern 2) by construction: the graph edits the working tree and
  never commits until G1 approves.
- **Verification points**: (i) after each impl node, a mechanical build+suite transform run
  by the orchestrator; (ii) `verify-impl`, an independent agent with a fresh context reading
  ADR-0007 against the diff, before any scale-out; (iii) `run-suites`, the extended suites
  after `tests`; (iv) `use-node`, a fresh agent given ONLY `runbooks/sift.md` and the built
  binary, checked against ground truth the orchestrator computes outside the pipeline.
- **Shared state**: see §3. No key has two writers inside a parallel region.
- **Cycles**: one — `verify-impl` blocking findings → re-dispatch `impl-b` with the findings.
  Bounded (§7). No in-node unbounded loops.
- **Scale/budget**: 8 planned agent dispatches; §10. No `+<N>k` directive was given, so
  there is **no hard cost bound**.
- **Tier fallback**: every opus-pinned judgment node falls back to `sonnet`; every
  sonnet-pinned plumbing node falls back to `haiku`.
- **Walking skeleton**: `impl-a` is the thinnest end-to-end slice (ordinal column renders,
  nothing else changes, both suites still green). Verification (`verify-impl`) sits BEFORE
  scale-out into { atlas ∥ tests → docs → use-node }.

**Shapes**: chain (baseline→impl-a→impl-b→verify-impl); router (verify-impl's severity
route); diamond (verify-impl fans out to atlas ∥ tests-chain, map-close joins them);
controlled cycle (verify-impl→impl-b repair, cap 2); gate (G1).

## 2. Carrier

**Graph in the main loop** (`/graphrun`). The one fact that chose it: the user invoked
`/graphrun`, which forbids the Workflow carrier — and independently, the graph is one-shot,
has 8 agent nodes, and its hardest joints (a C++ build under `-Wconversion`, tmux-driven
suites whose failure modes are not enumerable in advance) are exactly the adaptive-joint
case the main-loop row is for.

**File carrying the graph**:
`/home/fenrir/.tmux/graphruns/2026-09-01-1502-sift-ordinal-selection/` —
`graph.md` (this file), `state.md`, `nodes/<id>/prompt.md`.

## 3. State schema

### Message-passing state

| key | type | written by | read by | merge |
|---|---|---|---|---|
| `baseline` | `{buildClean:bool, warnings:int, jumpPass:13, jumpFail:0, livePass:6, liveFail:0}` | baseline (transform, DONE) | impl-a, impl-b, verify-impl, docs, map-close | single writer |
| `implA` | `{files:string[], buildClean:bool, warnings:int, suites:{jumpPass,jumpFail,livePass,liveFail}}` | impl-a | impl-b, verify-impl | single writer |
| `implB` | `{files, buildClean, warnings, suites, footerText:string, keyContract:{...}}` | impl-b | atlas, verify-impl, tests, docs, map-close | single writer |
| `verdict` | `{unmet:string[], blocking:Finding[], latent:Finding[]}` | verify-impl | routeAfterVerify, map-close | single writer |
| `newCounts` | `{jumpPass:int, jumpFail:int, livePass:int, liveFail:int, ordinalAssertions:int}` | run-suites (transform) | docs, counts-patch, map-close | single writer |
| `useResult` | `{method:string, landedLine:int|null, keystrokes:string, gaps:string[]}` | use-node | use-check, map-close | single writer |
| `truth` | `{line:int, ordinal:12}` | use-check (transform) | routeAfterUse, map-close | single writer, computed OUTSIDE the pipeline |
| `IMPL_ROUND` | `int` | orchestrator only | routeAfterVerify | never written by an agent |
| `approved` | `bool` | G1 gate (the user) | commit | never written by an agent |

### External state

| artifact | written by | read by | volatility |
|---|---|---|---|
| `tools/sift/src/main.cpp` | impl-a, impl-b | atlas, verify-impl, tests | stable within a run |
| `tools/target/release/sift` (+ `target/cmake-build/`) | impl-a, impl-b, run-suites, use-check (cmake) | run-suites, use-node, use-check | stable within a run |
| `tools/atlas/sift.org` | atlas, then counts-patch | map-close | stable; **two writers, strictly phase-ordered and never concurrent** — atlas owns prose/`:lines:`/`:SOURCES:`, counts-patch owns only the two assertion-count numerals |
| `records/2026-08-27-2240-tmux-sift/assets/scripts/verify-sift-live.sh`, `…/verify-sift-jump.sh` | tests | run-suites, use-check | stable within a run |
| `runbooks/sift.md` | docs | use-node | stable within a run |
| `records/2026-09-01-1330-sift-ordinal-selection/sift-ordinal-selection.org` | map-close | — | stable within a run |
| throwaway tmux servers `-L sift_{live,verify,live_outer,usenode,altprobe}` | baseline, run-suites, tests, use-node, use-check | — | **volatile** — never compared for equality across time |
| the user's real tmux server / `$TMUX` | **nobody** | — | **volatile, out of bounds** |
| git HEAD / index | commit (gated) | — | stable |

**REVISION set** (derived from the read-by column of every mutated row — not from recall):
`main.cpp` mutated by impl-a/impl-b → resets **atlas, verify-impl, tests**;
the two suites mutated by tests → resets **run-suites, docs** (counts) ;
`runbooks/sift.md` mutated by docs → resets **use-node**.
There is no cache token under this carrier (Agent dispatch has no prefix cache); the
REVISION set is used only for mid-run revision resets (§7).

## 4. Mutation set

| node | permitted effect | gated? |
|---|---|---|
| impl-a | write `tools/sift/src/main.cpp`; run cmake (writes `target/`) | no — working tree only |
| impl-b | same | no — working tree only |
| atlas | write `tools/atlas/sift.org` | no |
| tests | write the two `verify-sift-*.sh` scripts; drive throwaway `-L` tmux servers | no |
| docs | write `runbooks/sift.md` | no |
| map-close | write `records/2026-09-01-1330-sift-ordinal-selection/sift-ordinal-selection.org` | no |
| use-node | drive a throwaway `-L sift_usenode` tmux server ONLY; **must write no repo file** | no |
| baseline / run-suites / counts-patch / use-check | orchestrator transforms: cmake build, run the suites, sed two numerals, compute ground truth via `sift rows` | no |
| **commit** | `git add <explicit paths>` + `git commit` | **YES — G1** |

`verify-impl` is **read-only by construction** — it is the only agent node outside the
mutation set, and a file write by it is a graph violation, not a prompt surprise.

**Standing prohibition for every node**: never `git add .` / `git add -A`; never touch
`$TMUX` or the user's real tmux server; never `cargo` anything under `tools/sift`.

## 5. Nodes and edges

| id | class | model | fallback | subagent_type | time bound | one line |
|---|---|---|---|---|---|---|
| `baseline` | transform | — | — | — | 5 min | DONE pre-run: build clean, jump 13/0, live 6/0 |
| `impl-a` | judgment | opus | sonnet | general-purpose | 25 min | ordinal column renders always, left of the line-number column, width = digits of N; `draw()`/`render_line()` cell budget re-derived; horizontal-scroll guarantee survives; suites still 13/0 + 6/0 |
| `build-a` | transform | — | — | — | 5 min | orchestrator: `cmake --preset release`, warning count, both suites |
| `impl-b` | judgment | opus | sonnet | general-purpose | 40 min | ordinal mode: `read_key()` `c1` hook, buffer==selection, `goto>` prompt, Backspace pop, non-digit fallthrough, Enter unchanged, footer hint |
| `build-b` | transform | — | — | — | 5 min | same as `build-a` |
| `verify-impl` | judgment (independent verifier, **read-only**) | opus | sonnet | general-purpose | 25 min | fresh context: ADR-0007's settled decisions vs. the diff; emits `unmet[]` + severity-typed findings |
| `atlas` | plumbing | sonnet | haiku | general-purpose | 20 min | `tools/atlas/sift.org`: ordinal-mode prose, re-anchored `:lines:` excerpts, recomputed `:SOURCES:` sha256 |
| `tests` | judgment | opus | sonnet | general-purpose | 40 min | extend `verify-sift-live.sh` (+ jump.sh where arithmetic) with the 7 ordinal assertions |
| `run-suites` | transform | — | — | — | 10 min | orchestrator: run both extended suites, record counts |
| `counts-patch` | transform | — | — | — | 2 min | orchestrator: update the two assertion numerals in `tools/atlas/sift.org` |
| `docs` | plumbing | sonnet | haiku | general-purpose | 20 min | `runbooks/sift.md`: key table, `goto>`, left-Alt-only, new counts; record the footer decision |
| `use-node` | plumbing (use-node) | sonnet | haiku | general-purpose | 25 min | fresh agent, given ONLY `runbooks/sift.md` + the binary: reach match #12 by ordinal |
| `use-check` | transform | — | — | — | 5 min | orchestrator: ground truth via `sift rows` on the same fixture, computed outside the pipeline |
| `map-close` | plumbing | sonnet | haiku | general-purpose | 25 min | write t2/t3/t4 Resolutions into the map org, flip TODO→DONE |
| `G1` | gate | — | — | — | — | commit gate, Pattern 2 + main-loop pause |
| `commit` | transform | — | — | — | 5 min | orchestrator, post-approval only |

### Edges (each justified by data the downstream node consumes)

```
baseline ─► impl-a ─► build-a ─► impl-b ─► build-b ─► verify-impl ─┬─► atlas ────────────────────┐
                                    ▲                              │                              │
                                    └──── repair (IMPL_ROUND) ─────┤                              │
                                                                   └─► tests ─► run-suites ─┬─► counts-patch ─┐
                                                                                            └─► docs ─► use-node ─► use-check ─┐
                                                                                                                               │
                                                              map-close ◄──────────────────────────────────────────────────────┘
                                                                   └─► G1 ─► commit
```

- `baseline→impl-a`: impl-a reads the pre-change pass counts it must not move.
- `impl-a→impl-b`: impl-b edits the same function bodies impl-a just rewrote.
- `impl-b→verify-impl`: verify-impl reads the diff.
- `verify-impl→{atlas, tests}`: routing, not data — see `routeAfterVerify`. Cut as a false
  dependency otherwise; the reason it is kept is the walking-skeleton rule (verify before
  scale-out), so unverified code is never scaled out into three downstream artifacts.
- `atlas ∥ tests`: no data path between them — a genuine parallel region.
- `tests→run-suites`: run-suites executes the scripts tests wrote.
- `run-suites→{counts-patch, docs}`: both read `newCounts`.
- `docs→use-node`: use-node reads the runbook docs wrote — and nothing else.
- `use-node→use-check`: use-check compares `useResult.landedLine` against `truth.line`.
- **join at `map-close`** — whole-set need: each Resolution must quote its own ticket's
  evidence (t2 ← verdict + atlas + build; t3 ← newCounts; t4 ← docs + useResult), so a
  partial set cannot write the map. This is the only barrier in the graph.

### Routing functions (plain code over domain-fact fields; fall-through = the safe path)

```js
// after either impl node's build transform
function routeAfterBuild(s) {
  if (!s || s.buildClean !== true) return 'repair'
  if (s.warnings !== 0)            return 'repair'
  if (s.jumpFail === 0 && s.liveFail === 0 &&
      s.jumpPass >= 13 && s.livePass >= 6) return 'proceed'
  return 'repair'                       // fall-through = repair, never proceed
}

// after the independent verifier
function routeAfterVerify(v, IMPL_ROUND, CAP /* = 2 */) {
  if (!v || !Array.isArray(v.unmet) || !Array.isArray(v.blocking)) return 'halt'
  if (v.unmet.length === 0 && v.blocking.length === 0)             return 'proceed'
  if (IMPL_ROUND < CAP)                                            return 'repair'
  return 'halt'                          // fall-through = halt at the cap gate
}

// after the use-node's ground-truth comparison
function routeAfterUse(u, truth) {
  if (!u || u.landedLine == null || !truth)     return 'halt'
  if (u.method === 'ordinal' && u.landedLine === truth.line) return 'proceed'
  return 'doc-gap'                       // → docs attempt 2; a second miss halts
}
```

Dry-run (one toy state per branch + one unmatched, asserted before dispatch):
`routeAfterBuild({buildClean:true,warnings:0,jumpFail:0,liveFail:0,jumpPass:13,livePass:6})`
→ `proceed`; `…warnings:3` → `repair`; `routeAfterBuild(null)` → `repair` ✓ (unmatched
lands safe). `routeAfterVerify({unmet:[],blocking:[]},1,2)` → `proceed`;
`{unmet:['x'],blocking:[…]},1,2` → `repair`; `…,2,2` → `halt`; `routeAfterVerify(undefined,…)`
→ `halt` ✓. `routeAfterUse({method:'ordinal',landedLine:150},{line:150})` → `proceed`;
`{method:'arrows',landedLine:150}` → `doc-gap`; `routeAfterUse(null,…)` → `halt` ✓.

Every routed field is a **domain fact the node observed** (`warnings`, `jumpFail`,
`unmet`, `landedLine`, `method`) — never a model-emitted stage name.

## 6. Joint contracts

| edge | artifact | failure path |
|---|---|---|
| `baseline→impl-a` | `{buildClean, warnings, jumpPass:13, jumpFail:0, livePass:6, liveFail:0}` | **abort** — load-bearing; a red baseline means the change cannot be attributed |
| `impl-a→build-a` | edited `main.cpp` on disk + result block `{files[], summary}` | **retry once** with the failure evidence appended; second failure halts the branch |
| `build-a→impl-b` | `{buildClean, warnings, suites}` | **abort** if cmake itself cannot run; `routeAfterBuild → repair` otherwise |
| `impl-b→build-b` | edited `main.cpp` + `{files[], footerText, keyContract}` | **retry once** |
| `build-b→verify-impl` | `{buildClean, warnings, suites}` | **abort** if cmake cannot run |
| `verify-impl→route` | `{unmet:string[], blocking:[{decision,file,line,why,severity:'biases-deliverable'}], latent:[…]}` | **abort** on unparseable — a verifier whose verdict cannot be read is not a pass |
| `verify-impl→atlas` | the verified `main.cpp` on disk | **retry once**, then drop-and-log: atlas is documentation; a dead atlas node is reported in the run report and blocks nothing but t2's completeness claim |
| `verify-impl→tests` | the verified `main.cpp` + `implB.keyContract` | **retry once**, then **abort** — t3 is load-bearing |
| `tests→run-suites` | two executable scripts on disk | **abort** — the goal condition reads them |
| `run-suites→docs` | `{jumpPass, jumpFail, livePass, liveFail, ordinalAssertions}` | **abort** on unparseable counts |
| `docs→use-node` | `runbooks/sift.md` on disk | **retry once**, then `doc-gap` |
| `use-node→use-check` | `{method, landedLine, keystrokes, gaps[]}` | **drop-and-log** — a dead use-node is reported as *use-node did not run*, and t4 is then reported unverified rather than done |
| `*→map-close` | all of the above | **abort** — a Resolution written from a partial set would be a false record |
| `map-close→G1` | the full working-tree diff + the three Resolutions | **abort** |

**Terminal return contract** (this run's report classifies mechanically):
`awaiting-approval` (G1) · `aborted{at,reason}` · `repair-requested{at,round,cap,issues}` ·
otherwise a completion status. Any `state.md` row not at `verified` (gates: `approved`)
means the run is reported **incomplete**, never done.

**Every node prompt carries**: "end with a fenced `result` block matching the shape below;
write deliverables under the paths named; **on failure write NO artifact** — report it in
the result block only; **return a terminal result — no backgrounded self-checks**."

**Hang case**: handled, not assumed away. One timer is armed per wave
(backgrounded `sleep <shortest remaining bound>`); on any wake, every `dispatched` row's
`now − dispatched-at` is checked against its bound, an exceeded row is `TaskStop`ped and
marked `failed(timeout)`, and the timer re-arms for the next-soonest deadline.

## 7. Cycle bounds

| cycle | counter | cap | progress metric (plain code) | cap gate |
|---|---|---|---|---|
| `verify-impl → impl-b` repair | `IMPL_ROUND` (integer, orchestrator-owned, **never interpolated into a prompt**) | **2** (= `/graphrun`'s attempt cap: one dispatch + one retry) | `verdict.unmet.length + verdict.blocking.length` must strictly decrease round-on-round; no decrease = no progress = halt immediately, do not spend the last round | `impl-cap` → `{status:'awaiting-approval', gate:'impl-cap', payload:{blocking, latent}}` |
| `use-check → docs` doc-gap | `DOC_ROUND` | **2** | `useResult.gaps.length` must strictly decrease | `doc-cap` |

**No cache token** — Agent dispatch has no prefix cache under this carrier, so the
token half of the token/counter pair is vacuous and is deliberately dropped
(`/graphrun` Step 2). The counters above are the cycle bounds; the REVISION set in §3
survives and is used for mid-run revision resets only.

## 8. Gate plan

| gate | pattern | horizon | fail-closed condition | payload | resumeWith |
|---|---|---|---|---|---|
| **G1 `commit`** | Pattern 2 (staged mutation — every edit is an uncommitted working-tree change) **layered under** the main-loop pause | **same-session** | `approved !== true` → halt; nothing is `git add`ed or committed. A missing, ambiguous, or partial answer halts. | the complete `git status --porcelain` + `git diff` (the intent itself, not a summary), the three Resolutions as written, the before/after suite counts, and the commit plan with its file-separability check | `{approved: true, plan: 'three-commits' \| 'single-commit'}` |
| `impl-cap` (§7) | main-loop pause | same-session | reached only when `IMPL_ROUND === 2` and blocking findings remain | `{blocking, latent, diff}` | `{residualsWaived: true}` |
| `doc-cap` (§7) | main-loop pause | same-session | reached only when `DOC_ROUND === 2` and the use-node still cannot follow the runbook | `{gaps, runbook diff}` | `{residualsWaived: true}` |

**Every limitation the deliverable records about itself is routed, never shipped as prose**:
the map's two "Not yet specified" items (ordinal-column width jitter; an in-popup indicator
beyond `goto>`) are *deliberately* deferred by ADR-0007 and the map — `map-close` leaves them
in "Not yet specified", and G1's payload names them so the deferral is an explicit waiver
rather than silence.

## 9. Goal condition

**One checkable sentence**: with `tools/sift/src/main.cpp` rebuilt by
`cmake --preset release` producing **zero compiler warnings**, `verify-sift-jump.sh` and
`verify-sift-live.sh` both exit 0 with `jumpPass ≥ 13, jumpFail = 0` and
`livePass ≥ 12, liveFail = 0` including **≥ 6 assertions naming ordinal mode**,
`runbooks/sift.md` documents `goto>` and the left-Alt restriction, and a fresh agent given
**only** `runbooks/sift.md` plus the built binary reaches match **#12** of the 20-match
fixture by ordinal and lands on the line the orchestrator computed independently from
`sift rows` — with all three of t2/t3/t4 marked `DONE` in the map with Resolutions.

Mechanised as `goal-check.sh` in this run dir. **Negative control**: it is run once
pre-work and MUST fail — and must fail on the *missing ordinal work* (checks 3–6) while
checks 1–2 (build clean, baseline suites green) PASS, proving the check is aimed at a
working target rather than crashing or mis-pathed.

## 10. Budget and tiers

- **No hard cost bound** — no `+<N>k` directive was given, so there is no token target and
  nothing enforces spend. The bound below is an **agent-count** bound and is never to be
  read as a cost bound: one agent can spend arbitrarily much inside it.
- **Sizing (enforced by tally in `state.md`)**: 8 planned agent dispatches. With the
  attempt cap of 2 per node the ceiling is 16; the **stop-and-ask threshold is 12
  dispatches** (150% of the planned sizing) — crossing it halts the run for an explicit
  user go-ahead.
- **Per-node caps (advisory — these are in-prompt bounds, i.e. requests to an agent, not
  enforced limits)**: `impl-a`/`impl-b` ≤ 6 cmake invocations each; `tests` ≤ 8 suite runs;
  `use-node` ≤ 15 tmux commands and **must not read any file but `runbooks/sift.md`**.
- **Time bounds (enforced by the wave timer, §6)**: per §5's column.
- **Tiers**: opus for `impl-a`, `impl-b`, `verify-impl`, `tests` (C++ arithmetic under
  `-Wconversion`, adversarial verification, and tmux keystroke assertions are all places a
  down-tier slips) → fallback `sonnet`. sonnet for `atlas`, `docs`, `use-node`, `map-close`
  → fallback `haiku`. `use-node` is deliberately NOT opus: it must be an ordinary reader of
  the runbook, not a model clever enough to reconstruct the feature the doc failed to explain.
- **Composer**: `agentic-prompt-composer` is deliberately **not** loaded up front. Per
  `/graphrun` Step 1.2 it is invoked only for the judgment nodes and only after the walking
  skeleton (`impl-a`) has returned once — the candidates are `verify-impl` and `tests`.

## 11. Sanity gate

Walked against the skill's step-9 checklist under `/graphrun`'s carrier mapping. **Four
fixes made during the walk; then passed.**

1. **`atlas` originally sat after `run-suites`** so it could quote the new assertion counts —
   which serialized it behind the whole tests chain and destroyed the only parallel region.
   Fix: `atlas` moved into the `verify-impl` fan-out and the two count numerals split off
   into the `counts-patch` transform. Two writers on one file, so §3 now declares them
   phase-ordered with disjoint regions rather than leaving it implicit.
2. **The footer string had two claimed owners** — it lives in `main.cpp` (impl-b's file) but
   t4 asks for the decision to be *recorded*. Fix: `impl-b` owns the string (ADR-0007's
   Consequences already commit to "the footer and runbook have to say left Alt"), `docs`
   owns recording the rationale. Declared in §5 so neither node improvises.
3. **`use-node` was outside the mutation set but drives a tmux server** — a real world
   effect. Fix: added to §4 with its effect narrowed to a throwaway `-L` socket and an
   explicit no-repo-write prohibition, so a file write by it is a graph violation.
4. **The goal condition's use-node clause had no ground truth outside the pipeline.** Fix:
   added the `use-check` transform, which recomputes match #12's line from `sift rows`
   independently; and per §3 the tmux servers are classed **volatile** so nothing about them
   is ever compared for equality.

Remaining checklist items, explicitly:

- **Goodhart check** — a plausible WRONG deliverable that still passes verify: an
  implementation that buffers digits and moves the selection but *never renders the ordinal
  column*, so every assertion driven through `capture-pane` on the popup still matches
  `goto> 12` while the user can never see which number to type — the exact "a selection
  number nobody can see is a number nobody uses" failure ADR-0007's second decision driver
  names. Constructed in under a minute → the verify stage was strengthened: `tests` must
  assert the ordinal **column** is on screen next to its row (not just the prompt), and
  `verify-impl` must check the column against `draw()`'s width budget rather than trusting
  the prompt.
- **Generator ≠ verifier**: `verify-impl` is a separate call with a fresh context and is the
  only read-only agent node; `use-node` is a second independent path that never sees the code.
- **Volatile vs structural**: `landedLine`, `jumpPass`, `warnings`, `unmet` are structural
  (equality); tmux pids/socket paths/`history_size` are volatile and are never compared.
- **Ground truth for `use-check`** is computed by the orchestrator from `sift rows`, outside
  the node that is being checked.
- **No LLM-as-router**: all three route functions are plain code over observed domain facts.
- **Hang case**: handled by the per-wave timer (§6), not assumed away.

`graph sanity gate: passed`
