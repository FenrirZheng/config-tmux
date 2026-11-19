#!/usr/bin/env bash
# verify.sh — automated unit + headless CLI + isolated tmux server verification.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
build_dir="$here/../target/cmake-build-tmux-zhuyin"
bin="$here/../target/release/tmux-zhuyin"

echo "── 1. Unit tests (CTest / GoogleTest) ──"
ctest --test-dir "$build_dir" --output-on-failure

echo "── 2. Headless CLI (--eval) ──"
tmp_home=$(mktemp -d "${TMPDIR:-/tmp}/tmux-zhuyin-verify-home.XXXXXX")
tmp_out=$(mktemp "${TMPDIR:-/tmp}/tmux-zhuyin-verify-out.XXXXXX")
sock="tmux-zhuyin-verify-$$"
cleanup() {
  tmux -L "$sock" kill-server 2>/dev/null || true
  rm -rf -- "$tmp_home" "$tmp_out"
}
trap cleanup EXIT

HOME="$tmp_home" "$bin" --eval '5j/ jp6<Enter><Enter>' --out "$tmp_out"
got=$(cat "$tmp_out")
if [[ $got != "中文" ]]; then
  echo "FAIL: expected '中文', got '$got'" >&2
  exit 1
fi
echo "OK: --eval '5j/ jp6<Enter><Enter>' -> '$got'"

: > "$tmp_out"
HOME="$tmp_home" "$bin" --eval '5j/ jp6<C-g>' --out "$tmp_out"
if [[ -s $tmp_out ]]; then
  echo "FAIL: expected empty output on C-g cancel" >&2
  exit 1
fi
echo "OK: --eval cancel produces empty output"

echo "── 3. Isolated tmux server integration ──"
tmux -L "$sock" -f /dev/null new-session -d -s test "cat"
pane_id=$(tmux -L "$sock" display-message -p -t test '#{pane_id}')

# Verify tmux-zhuyin.tmux registers prefix Z binding with note.
TMUX="" tmux -L "$sock" run-shell "TMUX_TMPDIR=${TMUX_TMPDIR:-/tmp} tmux -L $sock source-file /dev/null" 2>/dev/null || true
# Source the entry script against the throwaway server.
(
  export TMUX=""
  # Run with a wrapper so `tmux` inside tmux-zhuyin.tmux talks to our socket.
  wrap_dir="$tmp_home/bin"
  mkdir -p "$wrap_dir"
  cat > "$wrap_dir/tmux" <<EOF
#!/usr/bin/env bash
exec /usr/bin/tmux -L "$sock" "\$@"
EOF
  chmod +x "$wrap_dir/tmux"
  PATH="$wrap_dir:$PATH" bash "$here/tmux-zhuyin.tmux"
)

binding=$(tmux -L "$sock" list-keys -T root 'C-\')
if [[ $binding != *"launch.sh"* ]]; then
  echo "FAIL: root C-\\ binding missing launch.sh: $binding" >&2
  exit 1
fi
echo "OK: root C-\\ bound -> $binding"

# Verify load-buffer + paste-buffer -p -d delivers UTF-8 cleanly and leaves no leftover buffer.
printf '%s' "中文測試" > "$tmp_out"
tmux -L "$sock" load-buffer -b "zhuyin-verify" "$tmp_out"
tmux -L "$sock" paste-buffer -p -d -b "zhuyin-verify" -t "$pane_id"
sleep 0.1
captured=$(tmux -L "$sock" capture-pane -p -t "$pane_id")
if [[ $captured != *"中文測試"* ]]; then
  echo "FAIL: pane did not receive '中文測試': $captured" >&2
  exit 1
fi
if tmux -L "$sock" list-buffers 2>/dev/null | grep -q "zhuyin-verify"; then
  echo "FAIL: temporary buffer was not deleted" >&2
  exit 1
fi
echo "OK: paste-buffer -p -d delivered '中文測試' and cleaned up buffer"

echo "── verify.sh: ALL PASSED ──"
