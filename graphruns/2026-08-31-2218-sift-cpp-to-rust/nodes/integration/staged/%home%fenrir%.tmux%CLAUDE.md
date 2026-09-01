<!-- atlas:start -->
## Atlas

This repo has an atlas at `tools/atlas/` — an org-file context graph explaining
the toolset (`tools/`) node by node — twelve Rust crates, `sift` included since
its 2026-08-31 port from C++/cmake (`docs/adr/0006-port-sift-from-cpp-to-rust.org`).
Before exploring that source, read `tools/atlas/index.org` and the nodes it
lists; follow their typed links instead of re-deriving structure.

With the atlas-nav skill installed, follow its discipline (it verifies node
freshness first). Without it, read the atlas directly and treat nodes as
helpful but unverified — where a node and the source disagree, the source wins.
If you edit files a node covers, update that node in the same change (the
atlas-build skill's update flow, when installed).
<!-- atlas:end -->
