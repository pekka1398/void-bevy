# 編譯：找出這台機器上最好的做法

## 問題

- 編譯只用 `-j 2`，16 個執行緒大多閒著。當初這樣定是怕記憶體不夠、吃到 swap 卡死整台電腦，現在已經有 `void-agent.slice`（11G、不准用 swap）擋著。
- 開關 feature、跑全部測試時要等很久，slice 的記憶體峰值會碰到約 11G。
- `void-app` 執行檔有 857MB。
- `target/` 有 100G 以上（`debug/deps` 49G、`debug/incremental` 33G），裡面大多是過時的編譯結果，從來沒清過。硬碟已經用了 85%，只剩約 70G。
- 多個 worktree 同時工作時，如果把 `target` symlink 到主目錄共用，依賴不用重編，但最後的執行檔（例如 `target/debug/void-app`）是同一個路徑，會互相蓋掉；同時編譯也要排隊等鎖。如果各用各的 target，每個都要從零編譯，硬碟也放不下好幾份。
- 沒有人量過時間和記憶體花在哪裡，所以也不知道該怎麼改。

## 要做的

範圍已縮小（使用者決定）：不做逐項對照實驗，直接套用明顯有利的做法，改動前後各量一次，寫 guide。不租伺服器。

已查過、不用再試的：
- rustc 1.99 在 x86_64 Linux 預設就用 lld 連結，不試 mold。
- 依賴已經是 `opt-level = 3` 加 `debug = "line-tables-only"`，維持原樣。

### 1. 套用

- 每個 crate 的整合測試合併成一個執行檔（`tests/<crate>/main.rs` 底下放模組）。測試一個不少（490 個），還在原來的 crate。
- Bevy 的 `dynamic_linking`，照 Bevy 文件的建議只用在開發編譯，release 維持靜態連結。`cargo run -p void-app` 和直接跑 `target/debug/void-app` 都要能開遊戲。
- 每個 worktree 各自的 target 放在 `/mnt/data`，寫進 guide。主目錄的 `target/` 不動，搬移的指令交給使用者。

### 2. 量改動前後

在自己的 target 目錄（`/mnt/data` 上）用 `-j 8` 量，改動前（`ab549af`）和改動後各一次，記牆鐘時間和該次的記憶體峰值：

| 情況 | 內容 |
|---|---|
| 完整編譯 | 空的 target，`cargo build -p void-app` |
| 改一行 | 在 `crates/vessels` 和 `crates/app` 各改一行之後重編 `void-app` |
| 測試 | 改一行之後跑全部測試 |
| check | 改一行之後 `cargo check --workspace --all-targets` |

先前已量到的完整編譯（`-j 2`、6、8）也列進對照表；`-j 2` 那次和別的編譯重疊，註明，不重量。

### 3. 寫 `guides/build.md`

- 什麼情況用什麼指令：check、只測改到的 crate、全部測試；
- `-j` 開多少，大概會用到多少記憶體；
- 多個 worktree、多個 agent 同時工作時，target 怎麼安排、編譯怎麼排隊；
- 怎麼清理 `target/`、多久清一次。

`guides/computeruse.md` 裡現在那段資源限制的說明搬到 `guides/build.md`，computeruse.md 只留一句指過去，同一件事只寫在一個地方。照 `guides/workflow.md` 的原則，只寫看程式碼看不出來、但一定要遵守的東西，內容要和實際量到的數字對得上。

### 4. AGENTS.md 第 4 條

如果結果顯示 `-j 2` 應該放寬，提出第 4 條的新寫法，附上數字，由我決定。agent 不准自己改 AGENTS.md。在我同意之前，平常的編譯維持 `-j 2`。

## 不做

- 不修遊戲本身的效能問題。
- 不租伺服器，也不碰系統設定（套件、sysctl、swap）。需要的話先告訴我。

## 驗收

- 交一份改動前後的對照表：每種情況的時間和記憶體峰值。
- `guides/build.md` 我讀得懂、照著做得出來。
- 我用平常的方式開遊戲（`cargo run -j <新值> -p void-app`），編譯和遊戲都正常。
