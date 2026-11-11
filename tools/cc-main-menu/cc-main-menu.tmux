#!/usr/bin/env bash
# TPM-style entry point for cc-main-menu (tmux command palette).
#
#   run-shell ~/.tmux/tools/cc-main-menu/cc-main-menu.tmux
#
# Options (set before the run-shell line):
#   @cc-main-menu-key        key in the prefix table   (default: m — replaces tmux's
#                                                       own select-pane -m; claude.conf
#                                                       moves marking to prefix M)
#   @cc-main-menu-width      popup width               (default: 80%)
#   @cc-main-menu-height     popup height              (default: 75%)
#   @cc-main-menu-autobuild  on = build in the background when the binary is
#                            missing (network wrap clone + LTO build, ~15 s).
#                            Default off: the key just says how to build, like
#                            sift's and pane-jump's not-built stubs.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
bin="$here/../target/release/cc-main-menu"

opt() { # opt NAME DEFAULT
  local v
  v=$(tmux show-option -gqv "$1")
  printf '%s' "${v:-$2}"
}

key=$(opt @cc-main-menu-key m)
width=$(opt @cc-main-menu-width 80%)
height=$(opt @cc-main-menu-height 75%)
autobuild=$(opt @cc-main-menu-autobuild off)

if [[ -x $bin ]]; then
  # display-popup does NOT expand formats in its shell-command (measured on 3.5a,
  # recorded in claude.conf's sift block), so the caller context is captured by
  # run-shell — which does expand — and handed to launch.sh as plain argv.
  tmux bind-key -N 'Main menu: search and run any tmux command or noted key' \
    -T prefix "$key" run-shell -b \
    "$(printf '%q' "$here/scripts/launch.sh") '#{client_name}' '#{session_id}' '#{window_id}' '#{pane_id}' $(printf '%q %q' "$width" "$height")"
  exit 0
fi

build_cmd="$here/scripts/build.sh"
tmux bind-key -T prefix "$key" display-message -l \
  "cc-main-menu: not built — run $build_cmd && tmux source-file ~/.tmux.conf"

[[ $autobuild == on ]] || exit 0

# Opt-in background build. flock -n: re-sourcing mid-build starts no second one.
lock="$here/../target/.cc-main-menu-build.lock"
log="$here/../target/cc-main-menu-build.log"
mkdir -p -- "$(dirname -- "$lock")"
tmux run-shell -b "flock -n $(printf '%q' "$lock") bash -c '
  tmux display-message \"cc-main-menu: building (first run)…\"
  if $(printf '%q' "$build_cmd") >$(printf '%q' "$log") 2>&1; then
    tmux display-message \"cc-main-menu: built\"
    $(printf '%q' "$here/cc-main-menu.tmux")
  else
    tmux display-message \"cc-main-menu: build FAILED — see target/cc-main-menu-build.log\"
  fi' || true"
