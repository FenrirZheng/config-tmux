# Intent: pane name prompt hints

Author: fenrir
Created: 2026-09-03
Status: accepted
Accepted: 2026-09-03

## Problem

`prefix g` (commit `af0aa50`, 2026-09-03) opens a bare `pane name:`
prompt. The user must already know a pane's title to type a fragment of it;
nothing shows which titles exist. A typo or an unknown title yields the
filter-ignored full tree (tmux drops a filter that would empty the list), and
even a correct fragment lands the cursor on the session row, so `Enter` must be
preceded by `Down Down` or a shortcut key. Two extra steps and one silent
failure mode for what should be one keystroke plus a name.

User's wording: 「增加功能 `ctrl + b g` 提示有哪些 pane name，然後我可以打字，命中
pane name，就跳過去 target tmux pane。我這是原本的功能再加上更多方便操作。」

## Proposed outcome

`prefix g` shows the pane titles that exist, the user types, and a hit on a pane
title switches the client straight to that pane. The existing jump-by-name
behaviour stays; the hint list and the direct jump are added on top.

Success: from any pane, `prefix g`, read the list, type a fragment, one
confirming key, and the target pane is active. No `Down Down`, no full tree on
a miss.

## Affected users and systems

- User: the single operator of this tmux server.
- Scope: every pane in every session (same `-a` semantics as the current
  binding), Claude and non-Claude panes alike.
- `tmux.conf`: the `bind -T prefix g` line is replaced by the new behaviour.
- `cheat.txt` row for `prefix g` and its `(Down Down, then Enter)` hint;
  `README.md` entry; `runbooks/switch-pane-by-name.md` (Option A / Option B
  sections, Verify table, Troubleshoot).
- Readers of `#{pane_title}` are unaffected: the feature only reads titles.
  Writing titles stays forbidden for Claude panes (`tools/ARCHITECTURE.org:35`).

## Constraints

- Same key, `prefix g`; the current `choose-tree` binding is superseded, not kept
  alongside.
- Titles are read, never written. `select-pane -T` stays off the table for any
  pane running `claude` (`talk ping`, `cc-attend`, `cc-fleet` key off the title).
- A jump on no match must fail closed: `switch-client -t ""` exits 0 and does
  nothing, so an empty resolution must never reach `switch-client`; it prints a
  message instead. (Amended 2026-09-03: the original wording also required a
  non-zero exit; the toolset invariant at `tools/ARCHITECTURE.org:139` is exit 0
  from anything a key binding runs, and the tool follows that.)
- Implementation is a C++ binary under `tools/`, built with cmake next to `sift`,
  owning its own interaction loop with no fzf dependency. User directive,
  2026-09-03, in this session: "i want to desgin cpp binary file to replace this
  bin/pane-jump", then "Own TUI, no fzf" at the prompt. This replaces the
  original constraint (plain tmux where possible, a helper script acceptable, a
  new crate out of scope unless tmux alone cannot do it). The reason the original
  allowed a crate at all still holds: tmux has no as-you-type filter over panes.
- tmux 3.5a is the measured baseline.

## Open questions

- Presentation: `command-prompt` has a one-line prompt. Where does the title
  list render: a `display-message` / `display-popup` before the prompt, an fzf
  popup that filters as you type (the `cc-fleet` shape), or the unfiltered
  `choose-tree` with the cursor pre-positioned on a pane row?
- Ambiguity: when a fragment matches several panes, jump to the first, or fall
  back to a list? "命中" suggests a unique match jumps and a multi-match lists.
- Match rule: substring, case-insensitive (`#{mi:}`), as today?
- Does a miss (zero matches) show the full list again, or an error message?
