# VOID 獨立專案基準（2026-10-02）

此 workspace 現在是 Bevy／Rust／native Rapier 的開發主線。以 `git subtree split` 抽出原 `lab/void-bevy`，保留 37 個相關提交；沒有重新執行 `cargo init`，也沒有把歷史壓成一個 initial commit。

## 來源與對應

- 舊 repository：`https://github.com/pekka1398/void.git`；本機參考為 `/home/pekka/Desktop/void`。
- 原提交：`ad2120b77f6a2ef8f99bd43a74fba897e730a443`。
- 抽出後提交：`24bc28ac2d3c8d387efae65efebaf7842266faad`。
- 原子目錄 tree：`605b4231576113cc35c32cf14d9cfe988b3d4d7c`，與抽出後 HEAD tree 完全一致。
- 抽出時 233 個追蹤檔案已逐一 SHA-256 比對相符。過去 commit hash 因路徑與祖先改變而更新；作者、訊息和相關變更歷史保留。
- `NOTE.md` 原樣由舊工作目錄搬入，包含當時尚未提交的筆記。生效開發規則見 `AGENTS.md`；TS 時期完成狀態與路徑是歷史記錄。
- 新 repository 維持 `master`，沒有設定遠端，也沒有向舊 origin 推送。

## 封存與恢復

封存位置：`/home/pekka/Desktop/void-archives/20261002-185903`。

- `void-history.bundle`：舊 repository 所有 refs 可達的 Git 歷史，已用 `git bundle verify` 驗證。
- `void-reference.zip`：舊工作目錄（含 TS sources/checks、vendor 參考、反編譯資料、目前筆記），排除各層 `.git`、`target`、`node_modules`、`dist`。
- `NOTE.md` 與 `working-tree.patch`：另存尚未提交的工作修改。
- `source-state.json`、`refs.txt`、`git-config.txt`、`files.json`、`extraction.json`、`SHA256SUMS`：來源／檔案清單、設定、歷史對應與校驗。

可用 `git clone void-history.bundle void-restored` 恢復 Git repository；工作目錄的未提交／未追蹤內容從 zip 恢復。zip 中的 symlink 按 Unix symlink 保存，還原工具需支援 symlink。Node 依賴與 build 輸出從保留的 manifests／lockfiles 重建。

舊 `void` 原始碼與 Git 歷史保留作參考，後續開發與提交在此 workspace。90 GB 的既有 Cargo `target/` 已移到新位置重用，未複製進封存；它不是原始碼。沒有把快取設為指向舊 workspace 的 symlink。

## Rust 執行與驗收

```sh
cargo run -p void-app
cargo run -p void-orbit-lab
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --bins --examples
```

所有 local path dependency 都位於本 workspace，Cargo.lock、系統 preset、模型資料、WGSL、golden JSON／bin 一併保留。Bevy／Rapier 用 crates.io，日常編譯與測試不用舊 TS 或 Node。沿用既有編譯快取的搬遷驗證不代表全新環境從零下載測試。視窗驗收由使用者進行。

搬遷時的移植盤點與當時缺口見 `port-audit.md`，目前狀態見 `status.md`；獨立專案不代表所有 TS lab 觀察工具均已搬齊，也不要求把原本獨立的 labs 先整合進主遊戲。

## 重產 TS golden

日常 Rust tests 使用已提交資料。需要重新產生時，保留或解壓舊 TS 參考，在其根目錄依 lockfile 安裝 Node 依賴，然後從新 workspace 執行：

```sh
python3 tools/regenerate-golden.py --reference-root ../void reference_frames
python3 tools/regenerate-golden.py --reference-root ../void orbit_scene
```

其他名稱對應 `golden/*.ts` 的檔名（不含 `.ts`）。runner 在暫存目錄還原舊路徑布局，以 symlink 讀取指定 TS checkout，執行的是本 repository 的 golden script；所有產生結果寫入本 workspace 的 `crates/`，不修改舊參考。不會下載 Node packages 或靜默選擇另一份 TS 實作；缺少來源／依賴即失敗。原始腳本保留舊相對 import，請用 runner 執行。

## 搬遷驗證結果

- 新位置 `cargo metadata --offline --no-deps`：21 packages，所有 local path dependencies 均在新 workspace。
- `cargo test --offline --workspace --all-targets`：196 passed、0 failed、1 ignored（原 Pebble 靜止傾角問題）。
- `cargo clippy --offline --workspace --all-targets -- -D warnings` 通過。
- `cargo build --offline --workspace --bins --examples` 通過。
- runner 重產 reference_frames／orbit_scene／assembly_visuals／vessels，輸出與搬遷前已提交資料完全相同。
- 舊 ZIP：47,347 entries，全數 CRC 檢查通過，約 4.12 GiB。原 repository HEAD 與工作修改狀態未改變。
- 使用 Rust 1.99.0／Cargo 1.99.0；測試沿用已移動的快取，沒有從零下載，也未啟動 GUI。
- 新 repo 的 `bevy-migration-baseline` tag 標記此次獨立設定 commit；新 repo 的 bundle 與驗證 logs 另存於同一封存目錄。
