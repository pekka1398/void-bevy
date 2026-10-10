# 編譯：找出這台機器上最好的做法

## 問題

- 編譯只用 `-j 2`，16 個執行緒大多閒著。當初這樣定是怕記憶體不夠、吃到 swap 卡死整台電腦，現在已經有 `void-agent.slice`（11G、不准用 swap）擋著。
- 開關 feature、跑全部測試時要等很久，slice 的記憶體峰值會碰到約 11G。
- `void-app` 執行檔有 857MB，`target/` 有 88G。
- 沒有人量過時間和記憶體花在哪裡，所以也不知道該怎麼改。

## 要做的

### 1. 量現狀

在 slice 裡量下面幾種情況的時間（牆鐘時間）和 slice 的記憶體峰值（`MemoryPeak`，每次量之前先歸零或記下起始值）。至少量兩次，避免偶然的結果：

| 情況 | 內容 |
|---|---|
| 完整編譯 | 清掉工作區 crate 和依賴之後，`cargo build -p void-app` |
| 改一行 | 在 `crates/vessels` 和 `crates/app` 各改一行之後重編 `void-app` |
| 測試 | 全部測試，以及只測單一 crate |
| check | `cargo check --workspace --all-targets` |

用 `cargo build --timings` 看 CPU 在哪些時段閒著、哪些 crate 最慢、連結花多久。

### 2. 逐項實驗

每一項都和現狀比較時間和記憶體峰值：

- `-j` 2、4、6、8；
- Bevy 的 `dynamic_linking`，只用在開發編譯；
- 把每個 crate 的測試合併成一個執行檔，例如 `tests/main.rs` 底下放模組；
- 工作區自己的 crate 也只留行號表（`debug = "line-tables-only"`）；
- 連結器：確認現在用的是哪一個，再試 mold；
- 檢查有沒有因為 feature 組合不同造成依賴重編（例如 `-p A` 和 `-p B` 各編一份 Bevy）；
- 清理 `target/` 的方法（例如 `cargo sweep`）。

如果發現其他值得試的做法，加進 spec 再試。

### 3. 選定並套用

選出這台機器上最好的組合，把設定改進專案。不能讓遊戲或測試的行為改變，遊戲的效能也不能變差。

### 4. 寫 `guides/build.md`

- 什麼情況用什麼指令：check、只測改到的 crate、全部測試；
- `-j` 開多少，大概會用到多少記憶體；
- 多個 agent 同時工作時怎麼排隊，例如共用 target 目錄時 cargo 會自己鎖住、要不要分開 target；
- 怎麼清理 `target/`。

`guides/computeruse.md` 裡現在那段資源限制的說明搬到 `guides/build.md`，computeruse.md 只留一句指過去，同一件事只寫在一個地方。照 `guides/workflow.md` 的原則，只寫看程式碼看不出來、但一定要遵守的東西，內容要和實際量到的數字對得上。

### 5. AGENTS.md 第 4 條

如果結果顯示 `-j 2` 應該放寬，提出第 4 條的新寫法，附上數字，由我決定。agent 不准自己改 AGENTS.md。

## 不做

- 不修遊戲本身的效能問題。
- 不租伺服器，也不碰系統設定（套件、sysctl、swap）。需要的話先告訴我。

## 驗收

- 交一份改動前後的對照表：每種情況的時間和記憶體峰值。
- `guides/build.md` 我讀得懂、照著做得出來。
- 我用平常的方式開遊戲（`cargo run -j <新值> -p void-app`），編譯和遊戲都正常。
