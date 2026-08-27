---
captured: 2026-08-27 22:35
session: 2e8c9164-aa89-4814-bb47-60e35f310169
project_dir: /home/fenrir/.tmux
cwd: /home/fenrir/.tmux
transcript: /home/fenrir/.claude/projects/-home-fenrir--tmux/2e8c9164-aa89-4814-bb47-60e35f310169.jsonl
source: ExitPlanMode (PostToolUse hook)
plan_source: /home/fenrir/.claude/plans/prefix-iridescent-cosmos.md
---

# `prefix /` → popup regex search（C++ 外部程式 `sift`）

## Context

`prefix /` 目前是 [`claude.conf:183-186`](/home/fenrir/.tmux/claude.conf) 的
`command-prompt -T search`：狀態列借一列當輸入行，打完整個 regex 按 Enter，交給
`send-keys -X search-backward`。它**非增量**是被逼的 —— 2026-08-22 在 3.5a 實測，
tmux 的 `search-*-incremental` 只吃純文字（`a.c` 不匹配 `abc`），而 regex 版
`search-backward` 沒有增量變體。所以今天打 regex 時完全沒有回饋：不知道命中幾筆、
命中在哪、pattern 打錯也要按下 Enter 才知道。

狀態列那一列在幾何上沒有任何可調空間（高度固定 1 列、寬度 = client 寬），要有
「邊打邊看命中清單」就必須換成 popup + 外部程式。本計畫新增 C++ 工具 **`sift`**
（seek 的 regex 兄弟），由 `display-popup -E` 承載，**完全取代**現有的狀態列綁定。

### 與 ADR-0001 的張力（要一起處理）

[`docs/adr/0001`](/home/fenrir/.tmux/docs/adr/0001-build-seek-on-tmux-builtin-incremental-search.org)
當初否決了「自己寫鍵盤迴圈」（Option 2）與「自繪 overlay」（Option 3），理由是
tmux 已免費提供增量過濾與全 scrollback 高亮。**那個理由在 regex 上不成立** ——
tmux 對 regex 沒有增量搜尋可借。這是驅動因素的差異，不是推翻前案，需補
`docs/adr/0005` 記錄範圍界定：`prefix Space`（純文字增量）續用 tmux 內建，
`prefix /`（regex）自己擁有互動迴圈。

### 一個要說清楚的取捨

`tools/` 至今是純 cargo workspace，`prefix Space` 的 `seek` 已經是 Rust 且共用
`tmuxlib`（tmux 呼叫、`message_literal()` 的格式注入防線都在裡面）。用 C++ 寫
`sift` 表示這些要在 C++ 端重寫一份、bootstrap 從一行 `cargo build` 變成兩個建置
系統。使用者已指定 C++，照辦；下面的設計把重寫面積壓到最小（只有 `tmux` 子行程
呼叫與 literal message 兩件事需要對應物）。

---

## 設計

### 互動契約

```
┌─ regex ⚠ visible screen only ──────────── 37 matches ─┐
│  12043  panic: error_code unwrap on None              │
│  18877  warn  errno_eagain retry 3/5                  │
│> 24190  FATAL error_open /dev/null                    │
│  31002  dbg   errno_pipe closed by peer               │
│ ↑↓ select  Enter jump  Esc cancel  C-w clear          │
└───────────────────────────────────────────────────────┘
```

- 邊打邊過濾，游標**預設停在最後一筆**（最接近畫面底部者），對齊現行
  `search-backward` 的「往上找最近一個」語意。
- pattern 非法時**不清空**上一輪結果，只在標頭顯示 dim 的 `invalid regex`。
- alt-screen（`#{alternate_on}`）時標頭掛 `⚠ visible screen only`，沿用現行綁定
  的警語；判斷移進程式內，binding 因此收斂成單一分支。
- `Esc` / `C-g` / `C-c` 取消：**不動 pane**，exit 0。

### Enter 之後（載入點）

一次 batch 的 tmux 呼叫，然後才 exit（popup 隨之關閉、pane 重繪）：

```
tmux copy-mode -t <pane> \;
     send-keys -X -t <pane> goto-line <N> \;
     send-keys -X -t <pane> search-forward <regex> \;
     [send-keys -X -t <pane> search-again] × k
```

為什麼是這個組合而不是單純 `goto-line`：**要讓 tmux 自己註冊搜尋字串**，否則
`search_present` 高亮、`n`/`N` 上下一個、以及 seek 的 `w`/`W`/`l`/`L` 接續抓取全部
失效。`goto-line` 先把游標放到選定那一行，`search-forward` 再落在該行第一個
match（並註冊 pattern），`k` 次 `search-again` 補到使用者實際選的第 k 個 occurrence。

### 為什麼用 POSIX `regcomp(REG_EXTENDED)` 而不是 `std::regex`

最終跳轉由 tmux 自己的搜尋執行，`sift` 列出來的 match 集合必須與 tmux 的一致，
否則會出現「清單裡有、跳過去卻落在別處」。tmux 的搜尋走 POSIX extended regex
（`[未驗證]`，以差分測試確認，見驗證 §3），libstdc++ 的 `std::regex` 預設是 ECMAScript
語法且慢得多。用 `<regex.h>` 兩個問題一起解。

### 掃描資料來源與行號基準

```
tmux display-message -p -t <pane> '#{history_size}	#{pane_height}	#{alternate_on}'
tmux capture-pane -p -t <pane> -S -<history_size> -E <pane_height-1>
```

**不加 `-J`**（合併換行會讓行號與 `goto-line` 的物理行脫節），不加 `-e`。擷取結果
的第 i 行即物理行 i。`goto-line` 的基準（0 或 1、以 history 頂端或畫面頂端起算）
是本計畫最關鍵的 `[未驗證]` 項，驗證 §2 專門打它。

### 不變式（沿用 ARCHITECTURE.org 的既有規約）

- **永遠 exit 0**（key binding 的非零退出會變成 tmux 錯誤 popup）。
- **pane 文字絕不進入 format context**：不 stamp 任何 pane option，所有
  `display-message` 一律帶 `-l`。這是 `seek` 已證成的那條路徑，不做 sanitize
  才不會破壞使用者要複製的文字。
- **不寫 pane title**。

---

## 檔案

### 新增

| 路徑 | 內容 |
|---|---|
| `tools/sift/CMakeLists.txt` | C++20、無外部相依（raw termios + ANSI + `<regex.h>` + libc `wcwidth`）。`RUNTIME_OUTPUT_DIRECTORY` 指向 `${CMAKE_CURRENT_SOURCE_DIR}/../target/release`，讓 `claude.conf` 的 `tools/target/release/<bin>` 路徑慣例零改動 |
| `tools/sift/src/main.cpp` | 全部邏輯（單檔；規模與 `seek/src/main.rs` 的 722 行同級） |
| `docs/adr/0005-own-the-interaction-loop-for-regex-search.org` | 記錄與 ADR-0001 的範圍界定，格式照既有四篇 |
| `runbooks/sift.md` | 安裝／驗證／排錯／回滾，結構抄 [`runbooks/seek.md`](/home/fenrir/.tmux/runbooks/seek.md) |
| `records/<stamp>-tmux-sift/assets/scripts/verify-sift-*.sh` | 驗證腳本，放在 seek 那批旁邊的同構位置 |

### 修改

| 檔案 | 改動 |
|---|---|
| `claude.conf:183-186` | 三行 `command-prompt` 版**整段換掉**成下方 guard 區塊 |
| `claude.conf:11-13`（檔頭） | `Implementation: Rust binaries in tools/, one cargo workspace` 已不成立；同時修 `tools/ARCHITECTURE.md` → `.org` 的舊路徑 |
| `cheat.txt:15` | 改成 popup 版說明 |
| `cheat.txt:48` | **本來就是錯的** —— 仍寫著 `prefix /  describe one key`，2026-08-22 覆蓋後沒同步。順手刪掉 |
| `tmux.conf:65` | `prefix /` 是 Space 的 regex 兄弟那句註解，補上「現在走 popup + sift」 |
| `tools/ARCHITECTURE.org` | Crates 表加 `sift` 一列、Key bindings 表更新 `prefix /`、開頭補一句 workspace 已非純 Rust 及 C++ 的建置方式 |
| `tools/atlas/text-piping.org` + `index.org` | `sift` 併入 text-piping 節點的 covers 清單；`index.org` 的 Coverage 計數（7 nodes / 14 files）連動更新。走 atlas-build 的 update flow |
| `runbooks/seek.md` | "What it does" 那句 regex 入口改指 `runbooks/sift.md` |

### 綁定（取代 `claude.conf:183-186`）

守衛用 **load-time `if-shell`**，與 seek 同一形式（同一段註解已說明：建置完必須
重新 source，stub 誠實說出來勝過四個死鍵）：

```tmux
# ── regex search on prefix / — popup + sift (supersedes the 2026-08-22
#    command-prompt version; rationale in docs/adr/0005) ────────────────────
if-shell '[ -x ~/.tmux/tools/target/release/sift ]' {
  bind -T prefix / display-popup -E -w 90% -h 70% -T ' regex ' \
    "~/.tmux/tools/target/release/sift '#{pane_id}'"
} {
  bind -T prefix / display-message -l 'sift: not built — cd ~/.tmux/tools && cmake -S sift -B target/cmake-build -DCMAKE_BUILD_TYPE=Release && cmake --build target/cmake-build && tmux source-file ~/.tmux.conf'
}
```

`-w 90% -h 70%` 對齊 `prefix S`（cc-fleet）與 resume picker，維持 popup 尺寸的一致性。

### 建置

```bash
cd ~/.tmux/tools
cmake -S sift -B target/cmake-build -DCMAKE_BUILD_TYPE=Release
cmake --build target/cmake-build -j
```

CMake 的建置樹刻意放在 `tools/target/` 底下 —— [`tools/.gitignore`](/home/fenrir/.tmux/tools/.gitignore)
的 `target/` 已經蓋住它，**不需要新增任何 gitignore 規則**。代價寫進 runbook：
`cargo clean` 會連 `sift` 執行檔一起掃掉，重跑 cmake 即可。

---

## 可測試接縫

照 `cc-fleet rows` 的既有先例（「印出列，供除錯與 fzf 之外使用」），`sift` 提供
無 TUI 的子指令：

```
sift rows <pane_id> <regex>   # 每列 <line_no>\t<col>\t<text>，退出 0
sift <pane_id>                # 正常 TUI
```

所有 headless 斷言都打 `rows`，不必驅動終端。

---

## 驗證

1. **建置與綁定**：`cmake --build` 後 `ls -l tools/target/release/sift`；
   `tmux source-file ~/.tmux.conf` 後 `tmux list-keys -T prefix /` 必須顯示
   `display-popup`（`command-prompt` 消失）。未 build 時 source 一次，確認落到
   stub 分支且訊息可讀。

2. **行號基準（最關鍵的 `[未驗證]`）**：throwaway server（`tmux -L probe`）印 300 行
   已知內容 `LINE-000 … LINE-299`，比對三者一致 ——
   `sift rows` 回報的 `line_no`、`send-keys -X goto-line N` 之後
   `#{copy_cursor_y}` + `#{scroll_position}` 推回的行、以及 `capture-pane` 該行文字。
   基準若非 0-based-history-top，改的是 `sift` 的偏移常數，先量再寫死。

3. **regex 語意差分**：一組刻意刁鑽的 fixture（`a.c`、`[0-9]+`、`err(or|no)_[a-z]+`、
   `^\s*#`、CJK 混排）比對 `sift rows` 的命中集合 vs. tmux 自己
   `search-backward` 逐次 `search-again` 走出來的集合。兩者必須逐筆相同 —— 這條
   同時驗掉「tmux 走 POSIX extended」這個假設。

4. **popup 開著時 send-keys 是否生效**：man 明載「Panes are not updated while a
   popup is present」，只講重繪、未講指令排隊。§2 的腳本順帶斷言：popup 內送出
   batch 後 exit，pane 最終確實停在 copy-mode 的正確位置。**若失敗**，退路是把
   batch 改成 `tmux run-shell -b` 延後投遞，屆時記進 runbook 的排錯段。

5. **tmux-in-tmux 實鍵測試**：沿用
   `records/2026-08-09-1116-tmux-seek/assets/scripts/verify-seek-live.sh` 與
   `verify-regex-search.sh` 的外層/內層 server 模式（`send-keys` 送不進 key table，
   必須有外層 client 餵真實 prefix 按鍵）。斷言：`C-b /` → 打 `[0-9]+` → Enter →
   `search_present=1` 且 `copy_cursor_x` 落在預期欄位。

6. **效能**：對一個灌滿 100000 行的 pane 量 `time sift rows`。逐鍵重跑須
   < 100 ms；超標就在 runbook 記下實測值並加擷取上限，不靜默截斷。

7. **手動**：
   - `C-b /` 打 `TODO|FIXME` → 清單 → Enter 落點 → 接 `w` 驗證 seek 串接仍活。
   - `n` / `N` 在落點後走下一個／上一個（證明 tmux 有註冊 pattern）。
   - 在 `less` 裡按 `C-b /`，標頭應出現 `⚠ visible screen only`。
   - `Esc` 取消後 pane 完全未進 copy-mode。
   - `C-b Space`（seek）行為必須完全不變。

---

## Commit

依 repo 慣例「config + service 同一顆」，但這裡程式碼量大且文件面廣，切兩顆
（檔案不相交、各自可建置）：

1. `tmux: sift — popup regex search (C++)` — `tools/sift/**` + `claude.conf` 綁定
   + `tools/ARCHITECTURE.org`
2. `tmux: docs for sift — ADR-0005, runbook, cheat sheet, atlas` — `docs/adr/0005`
   + `runbooks/` + `cheat.txt` + `tmux.conf` 註解 + `tools/atlas/**` + `records/**`

## 跨 repo 的後續（不在本次 commit 內）

`~/CLAUDE.md`（home dotfiles repo）兩處會過期，需另開一次提交：
「Fresh machine」第 4 步的 `cargo build --release`，以及 `.tmux/` 段落把 `tools/`
描述成「tracked cargo workspace」並列出十個 Rust crate 的那句。
