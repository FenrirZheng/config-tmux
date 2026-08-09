# Dispositions — delta review 2026-08-09 16:23

Triage of the 21 should-fix items and 9 nits from [findings.md](findings.md), which the
nine-blocker pass left untouched. Recorded 2026-08-09 during implementation (task T19).

(The count is 9 nits, not the 11 quoted when the review was delivered — counted off the
file: 21 `## Should-fix` bullets, 9 `## Nits` bullets.)

Status vocabulary: **fixed** (done, with where), **open** (still needs a decision or an
edit), **declined** (deliberately not doing it, with why).

## Should-fix (21)

| # | finding | disposition |
|---|---|---|
| 1 | `tmuxlib::message()` omits `-l`, no enforcement point | **fixed** — added [`message_literal()`](../../../../../tools/tmuxlib/src/lib.rs); `message()`'s doc comment now warns and points at it; `seek` uses only the literal form |
| 2 | ARCHITECTURE.md:121 `sanitize_format` mandate unamended | **fixed** — the mandate now states the either/or (keep text out of format contexts *or* sanitize) and names `seek` as the worked example |
| 3 | stdin vs argv for `to-claude` never stated | **fixed** — spec says stdin; implemented as a piped `stdin`, never an argument |
| 4 | spawn failure of `to-claude` swallowed with its exit code | **fixed** — spawn failure is reported (`seek: to-claude not built — …`), a non-zero exit is still swallowed |
| 5 | no `run-shell` vs `-b` decision, no wl-copy wait bound | **closed, no code change** — measured 2026-08-09: `wl-copy` *forks by default* (`-f, --foreground` is opt-in), so the process `seek` waits on is the short-lived parent and returns immediately. A wait bound would guard a hang that the default invocation cannot produce. The `run-shell` vs `-b` half is settled: the verified binding block uses plain blocking `run-shell` — seek returns fast (see the fork measurement), and blocking keeps message order deterministic |
| 6 | fail-closed message scope unstated (silent empty grab on `W`) | **fixed** — the fail-closed check runs before any to-claude call, so it fires on all four keys |
| 7 | guard stub names the build but not the reload | **fixed** — the stub string and the runbook both name `tmux source-file` |
| 8 | t10's restore recipe oversells offline capability | **fixed** — corrected to "needs network twice, and is interactive", with the `unbind` lines and the gitlink step |
| 9 | cutover commit omits the doc artifacts | **fixed** — the artifact table assigns every artifact to a commit |
| 10 | a Chinese query has no whitespace, so grain rule 2 never fires | **declined** — user ruling 2026-08-09: keep as designed. `w` delivers the containing run; `l`/`L` is the grain for CJK prose; a spaced query still delivers the query |
| 11 | t8 defers the branch choice to implementation | **fixed, and reversed** — the two-branch `if-shell` is now the primary, because `-p` splits on commas |
| 12 | the three pure functions are never named in the map | **fixed** — named and implemented: grain splitter, token extractor, path classifier |
| 13 | "every path exits 0" is satisfied by a program that does nothing | **fixed** — the nine-row Verify matrix pairs every path with a positive observable |
| 14 | the alt-screen suffix is referenced but never spelled out | **fixed** — it is `(visible screen only)`, in the message set and the runbook |
| 15 | truncation clips the diagnostic, not the payload | **fixed** — the degraded message leads with `⚠ wl-copy failed` |
| 16 | the `$HOME` rider has no owner or gate | **fixed** — verified already landed as bootstrap step 5; the rider is closed |
| 17 | Out of scope holds one entry | **fixed** — the map's Out of scope now lists the accepted thumbs losses (T20 ruling) plus the snapshot-only non-goals: regex/fuzzy search, pointer movement, headless use, `plans/seek.md` |
| 18 | the deletion half of the driver is unstated | **fixed** — T20 ruled 2026-08-09 (user: accept the loss); the cutover-scope note now records the deletion rationale: one grab system owns the key surface, and thumbs' remaining exclusive capabilities were weighed and ruled not worth a second installed system |
| 19 | Troubleshooting maps no message to a cause | **fixed** — the runbook's Troubleshooting table has a row per message |
| 20 | re-press in grabber mode is indistinguishable from live-search mode | **declined** — user ruling 2026-08-09: the no-search/no-match merge stays as t5 decided it; the incremental prompt showing match state live is the tell |
| 21 | `W`/`L` receipts name only the target pane, not the text | **declined** — user ruling 2026-08-09: to-claude's receipt stays as its five call sites know it; seek's silent-exit-out-of-copy-mode and cursor-moved guard are the mis-send defence |

## Nits (9)

| # | finding | disposition |
|---|---|---|
| 1 | README index line and cheat.txt:33 dropped from t11's Resolution | **fixed** (README) / **open** (cheat.txt:33 rides with T12) |
| 2 | `tools/Cargo.toml` workspace member edit in no artifact list | **fixed** — done, and cargo fails loudly anyway |
| 3 | the map never names the reload command | **fixed** — named in the rollback recipe and the runbook |
| 4 | rollback stops at the inner repo, skipping the `$HOME` gitlink | **fixed** — the gitlink step is in the recipe |
| 5 | rollback's thumbs rebuild has no stated time bound | **fixed** — "expect minutes, not seconds; `prefix Space` is dead meanwhile" |
| 6 | seek should check `pane_in_mode` before claiming an empty cell | **fixed** — implemented; out of copy-mode it exits silently |
| 7 | no minimum tmux version recorded | **fixed** — runbook Prerequisites pins ≥ 3.4, developed on 3.5a |
| 8 | ARCHITECTURE `plan` cell nearly collides with the superseded snapshot name | **fixed** — the cell is a markdown link to the directory, with a sentence saying which |
| 9 | no ROI or stop check recorded | **declined** — the effort is nearly done; a stop check now would cost more than it saves |

## Still open after this pass

One item: cheat.txt line 33's wording, which rides with T12 (the cutover-commit rewrite).

Everything else is ruled. The four items that needed the user — #10, #18 (via T20), #20,
#21 — were put to the user 2026-08-09 and ruled as recorded above: T20 accepts the feature
loss, the other three keep the shipped behaviour. (The `run-shell` vs `-b` half of #5
closed with the verified binding block — plain blocking `run-shell`.)
