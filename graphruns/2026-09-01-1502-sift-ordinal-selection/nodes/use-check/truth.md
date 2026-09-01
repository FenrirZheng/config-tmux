# use-check ground truth — computed by the orchestrator, OUTSIDE the pipeline

Fixture: `records/2026-08-27-2240-tmux-sift/assets/scripts/sift-fixture.sh`
Pattern: `[abc][abc]19[0-3]` → exactly 12 matches.
Computed with `sift rows` (the headless seam), which the use-node never touches:

```
ordinal  line  char_start  char_end  cell_start  text
  10     195       7          12         7       row193 aa193 bb193 cc193
  11     195      13          18        13       row193 aa193 bb193 cc193
  12     195      19          24        19       row193 aa193 bb193 cc193
```

**GROUND TRUTH for ordinal 12**: token `cc193`, cursor column **19**.

Why this fixture: the 12 matches span 4 lines x 3 occurrences, so an ordinal is provably
neither a line number nor a per-line index — a reader who conflates them lands on the wrong
occurrence and the check catches it.

## Field classification (never compare a volatile field for equality)

| field | class | how it is compared |
|---|---|---|
| `copy_cursor_x` = 19 | **structural** | equality |
| cursor word = `cc193` | **structural** | equality |
| `pane_in_mode` = 1 | **structural** | equality |
| absolute line index 195 | **VOLATILE** — depends on the pane's scrollback depth, which differs per throwaway server | **never compared**; informational only |
| tmux pids / socket paths | **VOLATILE** | never compared |
