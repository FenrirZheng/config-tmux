# Why this composition — the two judgment nodes of the sift-ordinal graphrun

Prompt files (kept in the graphrun's own node dirs, which is where `/graphrun`'s carrier
contract requires them — this run dir *is* the working area, so no `prompts/` was created in
the repo root):

- [`../nodes/verify-impl/prompt.md`](../nodes/verify-impl/prompt.md)
- [`../nodes/tests/prompt.md`](../nodes/tests/prompt.md)

Composed only after the walking skeleton (`impl-a`) had returned once, per graph-engineering
step 7 — plumbing nodes (`atlas`, `docs`, `use-node`, `map-close`) and the two implementation
nodes use the skeleton prompt and did not go through the composer.

## Node A — `verify-impl`

### Diagnosis (eight axes)

| axis | reading |
|---|---|
| **Oracle?** | **Yes, and it is unusually good**: the built binary can be driven on a throwaway tmux server and observed with `capture-pane`. Plus the build's warning count and two existing suites. The ADR's "interaction, as settled" is an enumerable checklist. |
| **Solution space** | Narrow — each settled decision is implemented or it is not. |
| **Primary failure fear** | **Plausible-but-wrong.** The implementing agent already read its own code and believed it worked; the risk is a change that reads correctly and behaves wrongly. |
| **Cost / volume** | One-off, ~12 checklist items. |
| **Size** | Fits one context (940-line file, ~80-line diff, short ADR). |
| **Interaction** | **Must observe mid-task** — keystroke behaviour is only knowable by running it. |
| **Horizon & recurrence** | Short, single pass (< 5 rounds) → no context curation needed. Not recurring. |
| **Edges** | No ambiguity (ADR is settled, not up for revision). No irreversible actions — the node is read-only. No untrusted content. **Abstain beats wrong**: a false `implemented` ships a broken feature, a false `unmet` costs one repair round. |

### Primitives selected

| primitive | the diagnosis line that chose it |
|---|---|
| **until-oracle-passes** (catalog row 1) | An objective oracle exists → the loop must use it, not the model's opinion. Realised as: *every behavioural verdict must be backed by pasted `capture-pane` output*; source reading is admissible only for the two structural checks. |
| **Oracle read-only to its subject** (row 1's rider / anti-pattern 3) | Made an explicit, reasoned prohibition: the node may not edit any file, because an oracle that can edit its subject reward-hacks it. This is also enforced structurally — `verify-impl` is the one agent node outside the graph's mutation set. |
| **adversarial-verify, default-refute stance** (row 5) | Failure fear is plausible-but-wrong. Stance: *every decision is UNIMPLEMENTED until demonstrated*; uncertainty maps to `unmet`, with the tie-break stated. |
| **completeness-critic** (row 6) | Secondary fear: a settled decision silently unimplemented. Step 1 forces the full checklist to be enumerated *before* testing, with "do not shorten the list to the ones you can easily test". |
| **ReAct** (row 12) | Must observe the environment mid-task. |
| **Lens diversity, written not implied** (per-role rules) | Three named lenses — A behavioural, B "the number must be visible", C invariants/arithmetic — each stating what it ignores. |
| **Severity-typed findings** | Required by the graph's joint contract: only `biases-deliverable` blocks; `latent` flows to a waiver. Keeps the run from halting on style. |

### Deliberate departure from the diamond rule

graph-engineering says a verifier over a **large** artifact should be a per-axis diamond
rather than one checklist agent (incident I-6). Not adopted here, deliberately: I-6's failure
mode is sequential discovery over a large surface, and this surface is one ~80-line diff
against a ~12-item settled checklist. The diversity that rule buys is supplied instead by the
three named lenses inside one agent, and the weight is carried by the *oracle* (drive the
binary) rather than by skeptic count. Declared rather than silently dropped.

### The Goodhart hardening

Lens B exists because of the graph's step-9 Goodhart check: an implementation that buffers
digits correctly but renders no ordinal column would pass every keystroke-level assertion and
still be useless — exactly the "a selection number nobody can see is a number nobody uses"
driver in ADR-0007. The prompt names that scenario explicitly and asks the node to report,
in `goodhartCheck`, whether it could have happened.

## Node B — `tests`

### Diagnosis (eight axes)

| axis | reading |
|---|---|
| **Oracle?** | **Yes, and the right one is not the obvious one.** The obvious oracle ("the suite is green") is worthless for a *new* assertion — it passes identically whether it tests the feature or tests nothing. The real oracle is a **mutation/negative control**. |
| **Solution space** | Narrow — eight named behaviours from map ticket t3. |
| **Primary failure fear** | **Both**, in a specific order: vacuous assertions first (plausible-but-wrong), a missing behaviour second (completeness). |
| **Cost / volume** | One-off; each suite run costs ~20 s, so runs are worth capping. |
| **Size** | Fits one context. |
| **Interaction** | Must observe — `send-keys` pacing is empirical. |
| **Horizon & recurrence** | Short. But the *artifact* is long-lived: these suites gate all future sift work, which is why a vacuous assertion here is worse than a missing one. |
| **Edges** | **Irreversible-adjacent**: the suites are shared infrastructure, and `verify-sift-jump.sh`'s control case is what stops a misaimed `$TMUX` from driving the user's real server. Abstain beats wrong: an honestly omitted assertion is recoverable, a vacuous one poisons every future run. |

### Primitives selected

| primitive | the diagnosis line that chose it |
|---|---|
| **Manufactured external signal** (catalog row 2) | "Want to verify but the obvious oracle is inert" → manufacture one. Realised as building the **pre-ordinal binary** from `git show HEAD:tools/sift/src/main.cpp` into a scratch dir and requiring **every new assertion to FAIL against it while all 19 existing ones pass**. Both halves are demanded, because all-fail means a broken harness and all-pass means vacuous assertions. This directly discharges the user's standing rule that a checker must be shown capable of both failing and passing. |
| **until-oracle-passes** (row 1) | The stop condition is the conjunction of the two runs, not a self-assessment. |
| **completeness-critic** (row 6) | The eight behaviours are enumerated in the prompt rather than left to the node to recall from t3. |
| **ReAct** (row 12) | Keystroke timing must be observed. |
| **Fail-closed / capped loop** (guide rule 3) | Cap of 8 suite runs, with "report the failing state — do not weaken an assertion to make it green" attached, so the cap cannot be met by lowering the bar. |
| **PAL-ish grounding** (row 22) | The N = 12 fixture must be *measured* with `sift rows`, not assumed — a hit count the model guesses would silently destroy assertion 3. |
| **Escape hatch → route, don't drop** | An unassertable behaviour is reported as `UNASSERTED — <why>` so the graph can route it, rather than being silently lost or papered over. |

I verified the negative-control mechanism myself before dispatching (built
`HEAD:tools/sift/src/main.cpp` with `g++ -std=c++20 -O2` into a scratch dir; the real
`tools/target/release/sift` was untouched) — so the prompt is not sending the node at a wall.
That also confirmed the prohibition it needed: `CMakeLists.txt` pins
`RUNTIME_OUTPUT_DIRECTORY` to `tools/target/release`, so a cmake-based control build **would**
have overwritten the binary the user's `prefix /` runs.

## Guardrail self-check (composition-guide)

| check | result |
|---|---|
| Oracle > self-eval | Both nodes are built on an external oracle; neither asks the model "is this good?". |
| No naive self-refine | `verify-impl` has no refine loop at all (one pass). `tests` loops, but against the negative control, capped at 8 runs. |
| Oracle read-only to its subject | `verify-impl` may edit nothing; `tests` may not edit `main.cpp`. Both stated as reasoned prohibitions **and** enforced by the graph's mutation set. |
| Workflow > agent | The surrounding control flow *is* a workflow (`graph.md`); only the two genuinely judgment-bearing steps are agents. |
| Diversity > scale | `verify-impl`'s three lenses each state what they ignore. No fan-out of near-identical prompts anywhere. |
| No debate for its own sake | None used. |
| `find N` not used for open-ended discovery | The eight behaviours are a closed, specified set from t3 — a fixed count is correct here. |
| Cost bounds fail closed | Both prompts abort on a missing input; `tests` caps runs and forbids weakening to reach the cap. |
| Silent truncation | Forbidden explicitly in `tests`' escape hatch (report `UNASSERTED`, never omit silently). |
| Contracted joints | Both emit a literal `result` skeleton matching what `graph.md` §6 says the consumer reads (`unmet`/`blocking`/`latent`; `newCounts`/`ordinalAssertions`). |
| Smell test — manifest, fail-closed, data-not-instructions, literal skeleton, stance + tie-break, escape hatch, reasoned prohibitions, closing echo | All present in both. |
| Dry-run against a toy input | Walked: every placeholder has a filler in the manifest; every step names an observable result; each `result` block parses into the field `graph.md`'s route functions read. |
| Recipe conformance | No R1–R8 recipe was matched wholesale; the compositions are built from catalog rows, each named above. The one named rule consciously **dropped** is the per-axis verifier diamond, with its reason given under "Deliberate departure". |
