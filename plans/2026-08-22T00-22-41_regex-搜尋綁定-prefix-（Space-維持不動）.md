---
captured: 2026-08-22 00:22
session: 9749c2c2-61a7-4d8d-b9c6-9524fb1ba847
project_dir: /home/fenrir/.tmux
cwd: /home/fenrir/.tmux
transcript: /home/fenrir/.claude/projects/-home-fenrir--tmux/9749c2c2-61a7-4d8d-b9c6-9524fb1ba847.jsonl
source: ExitPlanMode (PostToolUse hook)
plan_source: /home/fenrir/.claude/plans/wise-forging-quail.md
---

# regex 搜尋綁定 `prefix /`（Space 維持不動）

## Context

使用者要 regex 搜尋。實測（tmux 3.5a，throwaway server，2026-08-22）：

- 現在 `C-b Space`（seek 搜尋入口）用的 `search-backward-incremental` 是**純文字**——`a.c` 匹配不到 `abc`（`search_present=0`）。
- `search-backward`（非增量）是 **regex**——`a.c` 命中 `abc`、`[0-9]+` 命中 `123`，皆驗證。
- 增量 + regex 不可兼得：`-incremental` 變體固定純文字；拿 `-i` prompt 每鍵重跑 `search-backward` 會讓游標逐鍵複跳，且半打完的 regex 常是非法 pattern。

**使用者已拍板**（AskUserQuestion 自由填答）：Space 保持現在的增量純文字搜尋，**regex 綁在 `C-b /`**（打完整 regex 按 Enter 跳，非增量）。

`prefix /` 目前是 tmux 預設的 describe-key prompt——本機的按鍵求助已由 `prefix ?`（curated cheat sheet）+ `prefix M-?`（full dump）承擔，覆蓋它符合既有先例（`prefix C` 覆蓋 customize-mode，claude.conf 有註記）。

## 變更

### `claude.conf` — 在 seek 區塊之後新增（不進 seek 的 if-shell guard，純 tmux 無 binary）

```tmux
# ── regex search on prefix / (2026-08-22) ──────────────────────────────────
# Type a full regex, Enter, cursor lands on the match (searching backward
# through scrollback); n / N step next / previous (stock emacs copy-mode).
# seek's w/W/l/L grab keys chain from wherever the search lands.
# Non-incremental by necessity — measured on 3.5a: the -incremental search
# commands are plain-text only, and re-running a regex search per keystroke
# would compound the cursor jumps. Space keeps the incremental plain-text
# entry above; this is its regex sibling.
# Overrides tmux's default `/` (describe-key prompt) — same precedent as
# prefix C over customize-mode; key help lives on prefix ? / M-? instead.
bind -T prefix / if-shell -F '#{alternate_on}' \
  'copy-mode ; command-prompt -T search -p "(regex ⚠ visible screen only)" "send-keys -X search-backward \"%%%\""' \
  'copy-mode ; command-prompt -T search -p "(regex)" "send-keys -X search-backward \"%%%\""'
```

要點（沿用既有慣例，皆有註解出處）：
- `%%%` 不是 `%%`：回應會代入雙引號模板，需跳脫引號（claude.conf 既有 Space 綁定的註解）。
- alt-screen 兩分支警告 prompt，抄 Space 綁定的形式（`-p` 吃逗號分隔清單，不能用含逗號的條件 format）。
- 方向 backward（往 scrollback 上找），與 Space 一致；`n`/`N`（emacs copy-mode 內建 search-again/search-reverse）走下一個/上一個。

### 文件同步

| 檔案 | 動作 |
|---|---|
| `cheat.txt` | COPY & GRAB 區加一行：`prefix /  regex search — type pattern, Enter; n/N next/prev` |
| `tmux.conf` | 第 53 行起「prefix Space (seek...) lives in claude.conf」的註解補一句 `prefix /` 是它的 regex 兄弟 |
| `runbooks/seek.md` | "What it does" 補一句：regex 版入口在 `prefix /`，落點後同樣接 w/W/l/L |

不改任何 Rust code、不動 atlas（atlas 只覆蓋 tools/ 原始碼，claude.conf 不在任何節點的 Sources）。

## 驗證

1. `tmux source-file ~/.tmux.conf` 後 `tmux list-keys -T prefix /` 應顯示新綁定（describe-key 消失）。
2. **tmux-in-tmux 自動驗證**（沿用 `verify-seek-live.sh` 的既有模式——`send-keys -t <pane>` 送不進 key table，要在外層 throwaway server 的 pane 裡跑內層 client 才能餵真實 prefix 按鍵）：內層 pane 印 `abc123`，外層送 `C-b /`、打 `[0-9]+`、Enter，斷言 `search_present=1` 且 `copy_cursor_x=3`。一個小腳本、2–3 個斷言即可，放 `records/2026-08-09-1116-tmux-seek/assets/scripts/` 旁或新 records 目錄。
3. 手動：`C-b /` 打 `TODO|FIXME` Enter → 跳到最近一個；`n` 走下一個；接 `w` 抓 token 驗證 seek 串接。
4. `C-b Space` 行為不變（增量純文字）。

## Commit

單一 commit（config + 文件），subject 風格照 repo：`tmux: regex search on prefix /`。
