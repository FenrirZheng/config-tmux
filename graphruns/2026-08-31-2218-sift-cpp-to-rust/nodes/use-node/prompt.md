You are a tmux user who has just cloned this dotfiles repo onto a fresh machine. You have
never seen this tool before and you have no context about how it was built.

**Read exactly one document**: `/home/fenrir/.tmux/runbooks/sift.md`. Do not read
`tools/sift/src/main.rs`, any ADR, any atlas node, or anything under `graphruns/`. If the
runbook does not tell you something you need, that is a finding — report it rather than
going to the source to fill the gap. (You may read `records/2026-08-27-2240-tmux-sift/assets/scripts/sift-fixture.sh`,
since it is only the corpus you will search.)

## Your task

Answer four concrete questions about a tmux pane's scrollback using the `sift` tool.

Set up your own throwaway tmux server (`tmux -L <some-name>`, **never** the user's — and
`unset TMUX` so you do not inherit theirs), make a pane, and populate it by running
`bash /home/fenrir/.tmux/records/2026-08-27-2240-tmux-sift/assets/scripts/sift-fixture.sh`
inside it. Then:

**Q1.** How many occurrences match the regex `aa1[0-9][0-9]` ?

**Q2.** On the line containing CJK text, what does sift report for the match `aa999` —
give every field sift prints, in order, and say what each field's unit is (the runbook
explains the distinction that matters here).

**Q3.** POSIX extended regular expressions and PCRE differ on alternation. Determine
**empirically, using sift itself**, which one sift implements: search the fixture for
`aa1|aa10|aa100` and report the exact span sift matched on the first hit. Say which
dialect that span proves.

**Q4.** Produce sift's raw output, unmodified, for these four patterns in this exact
order, each preceded by a line `### pattern: <the pattern>`:

```
aa1[0-9][0-9]
中文測試
bb0(1|2)[0-9]
^row19[0-9] 
```

(The last pattern ends with a space. Preserve it.)

Write that combined output **verbatim, byte for byte** to
`/home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/nodes/use-node/rows-usenode.txt`. Do not reformat, re-sort, align, or strip
anything — it will be compared byte-for-byte against a reference.

## Also report

- Was the runbook sufficient to do this task without reading the source? Name anything
  you had to guess, and anything the runbook says that turned out to be wrong.
- How long did the build take, if you had to build anything? (Check whether the tool is
  already built before building.)

Kill every tmux server you start. Do not modify any file outside your output paths. Do not
commit anything.

## Output contract

```result
status: ok | failed
q1_count: <N>
q2_fields: <the fields, tab-separated, and their units>
q3_span: <the matched span> / dialect: <POSIX leftmost-longest | PCRE leftmost-first>
q4_path: /home/fenrir/.tmux/graphruns/2026-08-31-2218-sift-cpp-to-rust/nodes/use-node/rows-usenode.txt
runbook_sufficient: <yes|no — what was missing or wrong>
had_to_build: <yes|no>
notes: <one line; on failure, why>
```

On failure write NO artifact — report it in the result block only. Return a terminal
result: do not background any self-check and do not end your turn waiting on anything.
