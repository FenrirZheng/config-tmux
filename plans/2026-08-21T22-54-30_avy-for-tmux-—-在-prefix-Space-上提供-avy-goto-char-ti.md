---
captured: 2026-08-21 22:54
session: 9749c2c2-61a7-4d8d-b9c6-9524fb1ba847
project_dir: /home/fenrir/.tmux
cwd: /home/fenrir/.tmux
transcript: /home/fenrir/.claude/projects/-home-fenrir--tmux/9749c2c2-61a7-4d8d-b9c6-9524fb1ba847.jsonl
source: ExitPlanMode (PostToolUse hook)
plan_source: /home/fenrir/.claude/plans/wise-forging-quail.md
---

# avy for tmux — 在 `prefix Space` 上提供 avy-goto-char-timer 式跳躍（Rust 新 crate）

## Context

使用者要在 tmux 的 `prefix Space`（prefix 是 C-b）上得到 Emacs avy 的跳躍體驗：連打字元篩選可視畫面上的匹配位置、停頓後疊出單鍵標籤、按標籤直接把游標跳到該處。工具用 Rust 寫，放進 `~/.tmux/tools` 既有的 cargo workspace。

現在 `prefix Space` 是「copy-mode + tmux 增量搜尋」（seek 的跳躍段，`claude.conf:113-138`）。**使用者已拍板**（AskUserQuestion，2026-08-21）：

1. 互動模型：**timer 式**（對應 avy-goto-char-timer）— 連打任意長度字元，停頓（預設 500ms，`@avy-timeout` 可調）後出標籤；恰好 1 個匹配立即跳。
2. 舊綁定：**直接取代** — Space 只給 avy；增量搜尋仍可用 `prefix [` 進 copy-mode 後按 C-r/C-s（mode-keys emacs 內建），不另設綁定。

跳完停在 copy-mode 目標位置 → seek 的 `w/W/l/L`（抓 token/行 → clipboard/Claude）原封不動接上，avy 跳 + seek 抓組成完整的 EasyMotion 式「跳了就做」。

## 設計

### 新 crate：`avy`（單檔 `main.rs`，依 workspace 慣例）

- 命名跟 `seek` 一樣不掛 `cc-` 前綴（非 Claude 專屬工具）。
- 依賴：`tmuxlib`（workspace）+ `unicode-width`（已是 workspace dependency）。**不新增外部依賴**。
- raw mode 不用 termios/libc/crossterm：shell out `stty raw -echo min 0 time <ds>`（`time` 以 0.1s 為單位 → timer 逾時直接由 `read()` 回 0 bytes 實現，零輪詢）。這是 workspace「最小依賴」哲學下的既有精神（ace-window 用 bash `read -rsn1`，同為 stty 之上）。

### 兩個子命令

**`avy launch <pane_id>`**（由綁定呼叫）
1. 一次 `tmuxlib::display()` 讀 `#{pane_left} #{pane_top} #{pane_width} #{pane_height} #{pane_in_mode} #{scroll_position}`（tab 分隔，seek `read_state` 同款）。
2. 執行 `tmux display-popup -B -x <left> -y <?> -w <width> -h <height> -E "<self> ui <pane_id> <scroll>"` — **無邊框 popup 精準覆蓋目標 pane**（tmux 3.5a，`-B` 自 3.3 起可用；status 在底部所以 pane 座標與 client 座標對齊）。
   - ⚠️ 實測項 1：`display-popup -y` 的語意（top edge 或 bottom edge）man page 不明確 — 實作第一步先在 throwaway server 上量，量完把結論寫進程式註解（seek「Measured on tmux 3.5a」慣例）。

**`avy ui <pane_id> <scroll>`**（在 popup 內跑）
1. 捕捉兩份：`capture-pane -p -t <pane> -S <top> -E <bottom>`（純文字，供匹配與定位；-S/-E 依 scroll 平移，抄 seek `logical_line` main.rs:365-385 的算式）+ 同範圍 `-e` 版（含 SGR，供如實重繪）。**不用 `-J`**：一行 = 一個 screen row，定位以 (row, row 內 char index) 表示。
2. 渲染：清畫面 → 印 `-e` 版內容（raw mode 下用 `\r\n`）→ overlay 用游標定址 `ESC[row;colH` 蓋字。display column 由純文字版 + `UnicodeWidthChar` 算（char index → cell column，即 seek `wrapped_char_index` 的反向；蓋到寬字元的另半格時補一個空白）。
3. 事件迴圈（timer 式）：
   - 篩選段：每鍵重繪，匹配處以高亮蓋字標示。smart-case（全小寫 query 不分大小寫）。Backspace 刪字元；Enter 強制立即出標籤；Escape / C-g / C-c 取消（直接 exit 0，popup 自動關）。
   - `read()` 逾時（`@avy-timeout`，pane→global option 讀取，預設 500ms）且 query 非空、匹配 ≥1 → 進標籤段。恰好 1 個匹配 → 直接跳。
   - 標籤段：標籤鍵來自 `@avy-keys`（預設 `asdfghjkl`），匹配數 > 鍵數時用 avy 的樹狀 2 層標籤（9+81=90 個；再多就取前 90 並以 `message_literal` 提示截斷 — ARCHITECTURE「no silent caps」精神）。標籤蓋在匹配起點。按標籤跳；Backspace 回篩選段；其他鍵取消。
4. 跳躍執行（一次 `tmux_batch`）：
   - pane 不在 copy-mode：`copy-mode -t <pane>` 先進。
   - 定位：`send-keys -t <pane> -X top-line` → `-X -N <row> cursor-down` → `-X start-of-line` → `-X -N <chars> cursor-right`。這組原語 seek 的 headless 驗證腳本已在 tmux 3.5a 實測過。
   - ⚠️ 實測項 2：wrapped 行的續行 row 上 `start-of-line` 是回 screen row 開頭還是 logical line 開頭 — 先量（含 CJK 寬字案例，仿 seek main.rs:606-620 的 41×`中` 測法），結果決定 cursor-right 的計數基準。
5. 錯誤處理循 workspace 慣例：**任何路徑 exit 0**；錯誤以 `message_literal("avy: …")` 回報（pane 文字絕不進 format context，seek 同款豁免、免 sanitize）。不寫 pane title、不 stamp 任何 option。

### 純函式半 / tmux 半切分（seek main.rs:71-73 banner 慣例）

純半（全部單元測試）：query 匹配（smart-case、行內多匹配、char index）、char index → display column（含寬字/零寬）、標籤樹分配（n 匹配 × k 鍵 → 標籤序列，含 2 層）、`@avy-timeout` 解析、跳躍命令序列生成（(row, char) → send-keys 參數列，實測項 2 的結論編碼在這裡）。

tmux 半：capture、popup launch、stty、事件迴圈、batch 執行。

## 檔案變更

| 檔案 | 動作 |
|---|---|
| `tools/Cargo.toml` | members += `"avy"` |
| `tools/avy/Cargo.toml` | 新增（tmuxlib + unicode-width，抄 seek 的） |
| `tools/avy/src/main.rs` | 新增（單檔：純半 + tmux 半 + `#[cfg(test)]`） |
| `claude.conf` | seek 區塊改寫：`bind -T prefix Space run-shell ".../avy launch #{pane_id}"`，guard 改查 avy 二進位（`if-shell '[ -x ... ]'` 雙分支 + 未建置 stub 訊息，沿用 :113-138 既有形式）；copy-mode `w/W/l/L` 四鍵不動；區塊註解改寫成 avy 跳 + seek 抓的新分工 |
| `tools/ARCHITECTURE.org` | Crates 表 + Key bindings 表各加/改一列（`prefix Space` → avy） |
| `runbooks/avy.md` | 新增，照 `runbooks/seek.md` 的節結構（What it does / Prerequisites / Install / Verify / Troubleshooting / Alt-screen / Rollback / Update） |
| `runbooks/seek.md` | 標題與 What-it-does 提及 Space 已移交 avy（seek 現在只負責 copy-mode 抓取鍵） |
| `records/2026-08-21-*-tmux-avy/assets/scripts/verify-avy-headless.sh` | 新增：throwaway server 驗證腳本（見下） |
| `tools/atlas/` | atlas-build update flow：新增 avy 節點（或併入導覽類節點）、index.org 節點表 + Coverage 更新（「every crate covered」的宣稱要繼續為真）、re-hash。與程式碼同一個 commit |

## 實作順序

1. **先量兩個實測項**（throwaway server：`tmux -L avymeasure -f /dev/null new-session -d -x 40 -y 14`，仿 `records/2026-08-09-1116-tmux-seek/assets/scripts/verify-seek-headless.sh`）：popup `-y` 語意；wrapped 續行上 `start-of-line` 行為。結論寫進註解與純函式。
2. crate 骨架 + 純函式半 + 單元測試（`cargo test -p avy`）。
3. tmux 半（capture / stty 事件迴圈 / 渲染 / batch 跳躍）。
4. `cargo build --release`，claude.conf 綁定改寫，`tmux source-file ~/.tmux.conf`。
5. 驗證（下節），docs + atlas 更新，commit（repo 慣例：config + 工具同 commit，`tmux: ...` 風格 subject）。

## 驗證

**自動（headless）** — `verify-avy-headless.sh`，`CC_TMUX_SOCKET` seam + throwaway server：
- 在固定內容的 pane 上以 `send-keys` 餵 avy popup 按鍵序列，斷言 `#{copy_cursor_x}/#{copy_cursor_y}` 落在預期位置。案例至少含：同 row 多匹配、跨 row、CJK 寬字行、恰 1 匹配立即跳、Escape 取消（pane 不進 copy-mode）、已在 copy-mode 且 scroll>0 的 pane。
- 已知限制照 seek 前例記錄在腳本 header（popup 的鍵盤互動若無法完全 headless 驅動，列為 keyboard-only 案例）。

**手動**：
1. `prefix Space` → popup 無縫覆蓋 pane（顏色如實）→ 打 2-3 字 → 停頓出標籤 → 按標籤 → 游標落點正確且在 copy-mode。
2. 接著按 `w` → seek 抓 token 進 clipboard（整條 jump+grab 鏈路）。
3. 未建置情境：暫時 mv 走二進位、re-source，Space 顯示 stub 訊息。
4. alt-screen pane（htop）：只跳可視畫面，行為合理。

## 明確不做（v1）

- avy dispatch actions（跳前對目標做 kill/copy）— seek 的 w/l 已覆蓋「抓」，之後有需要再加。
- 跨 pane 跳躍（`avy-all-windows` 類比）— `prefix o`（ace-window）已管 pane 跳躍。
- scrollback 搜尋 — avy 語意本來就只管可視範圍；要翻歷史用 `prefix [` + C-r。
