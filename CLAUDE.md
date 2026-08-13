<!-- atlas:start -->
## Atlas

This repo has an atlas at `tools/atlas/` — an org-file context graph explaining
the Rust toolset (`tools/`) node by node. Before exploring that source, read
`tools/atlas/index.org` and the nodes it lists; follow their typed links instead
of re-deriving structure. Check freshness first:

    python3 ~/.claude/skills/atlas-build/scripts/atlas-stale.py --dir ~/.tmux/tools/atlas

If you edit files a node covers, update that node in the same change (the
atlas-build skill's update flow).
<!-- atlas:end -->
