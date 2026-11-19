# tmux-zhuyin

位於 tmux 彈出視窗中的純終端注音輸入法（注音符號）。無需 X11/Wayland 顯示伺服器、IBus/Fcitx5 輸入法框架或 D-Bus，即可在任何 tmux 窗格輸入繁體中文：透過 **FTXUI** 繪製彈出視窗、底層使用 **libchewing（新酷音）**，確認後以 bracketed paste（`paste-buffer -p -d`）安全貼入原窗格。

## 相依套件（Dependencies）

### 1. Debian 系統套件（優先使用）

```bash
sudo apt update && sudo apt install -y \
  g++ \
  cmake \
  ninja-build \
  ccache \
  pkg-config \
  libftxui-dev \
  libchewing3-dev \
  libchewing3-data \
  libgtest-dev
```

| 套件 | 用途 | 類型 |
| --- | --- | --- |
| `libftxui-dev` (>= 5.0) | 終端 TUI 介面框架（`ftxui::screen`, `ftxui::dom`, `ftxui::component`） | 必要（可走 Conan fallback） |
| `libchewing3-dev` (>= 0.9) | 新酷音注音輸入引擎開發標頭與連結庫（`pkg-config: chewing`） | 必要 |
| `libchewing3-data` | 新酷音系統詞庫（`/usr/share/libchewing/`） | 執行期必要 |
| `libgtest-dev` | GoogleTest 單元測試框架 | 測試用（選用） |
| `cmake`, `ninja-build`, `ccache`, `pkg-config`, `g++` | 建置與編譯加速工具鏈（Ninja + ccache + PCH） | 建置期必要 |

### 2. Conan 2 Fallback（無 Debian `libftxui-dev` 時自動啟用）

若環境未安裝 Debian 的 `libftxui-dev`，`scripts/build.sh` 會自動偵測並透過 [`conanfile.txt`](conanfile.txt) 由 ConanCenter 下載編譯 `ftxui/5.0.0`（使用 `CMakeDeps` + `CMakeToolchain`）。

## 建置與測試

```bash
# 編譯 Release 執行檔至 ~/.tmux/tools/target/release/tmux-zhuyin
bash ~/.tmux/tools/tmux-zhuyin/scripts/build.sh

# 執行完整驗證（GoogleTest + Headless CLI + 獨立 tmux server 端對端測試）
bash ~/.tmux/tools/tmux-zhuyin/scripts/verify.sh

# 或直接使用 CMake Presets（Ninja + ccache + PCH）
cmake --preset release && cmake --build --preset release
ctest --test-dir ~/.tmux/tools/target/cmake-build-tmux-zhuyin --output-on-failure
```

## tmux 設定與選項

在 `~/.tmux/claude.conf`（或 `tmux.conf`）中載入：

```tmux
set -g @tmux-zhuyin-table root     # 鍵位表：root（免 prefix，預設）或 prefix
set -g @tmux-zhuyin-key 'C-\'      # 呼出快捷鍵（預設 C-\，即 Ctrl+\）
set -g @tmux-zhuyin-width 68       # 彈出視窗寬度（預設 68）
set -g @tmux-zhuyin-height 11      # 彈出視窗高度（預設 11）
set -g @tmux-zhuyin-kb default     # 鍵盤配置：default/hsu/et/et26/dvorak/ginyieh/dachen_cp26
run-shell "~/.tmux/tools/tmux-zhuyin/tmux-zhuyin.tmux"
```

## 操作快捷鍵

| 按鍵 | 動作 |
| --- | --- |
| 一般按鍵 | 注音拼字（標準大千鍵盤，預設）或英數輸入 |
| `↓` / `Space` | 開啟候選字列；`1`–`9` 直接選字，`←`/`→` 或 `PgUp`/`PgDn` 翻頁 |
| `Enter` | 階段一：確認當前組字區；階段二（組字區已空）：關閉彈窗並貼入原窗格 |
| `Ctrl+S` | 立即確認組字區並送出全部文字 |
| `Tab` / `Ctrl+\` | 切換中文（注音）／英數模式 |
| `F6` / `Ctrl+F` | 切換半形／全形 |
| `` ` ``（反引號） | 開啟常用標點符號選單 |
| `Backspace` | 刪除注音／組字字元；組字區為空時刪除已確認文字的最後一個字元 |
| `Esc` / `Ctrl+G` / `Ctrl+C` | 關閉候選列（若開啟）；否則取消並關閉彈窗（不送出文字） |
