# VOID 開發規則

此 repository 是 VOID 的 Bevy／Rust／native Rapier 開發主線。工作規則延續 `NOTE.md`，其中 TS 路徑與完成狀態是搬遷時的歷史筆記；目前狀態以 `docs/port-audit.md` 和程式碼為準。

- 不加入 fallback：不該發生的狀態應直接報錯／panic，不掩蓋問題。
- 每項功能先在獨立 core crate 與 lab app／example 開發，做 headless 數值／行為檢查；使用者完成視窗驗收後再決定主遊戲整合。
- Git 用來保存成果，不開開發 branch；維持 master，取捨與實驗用 lab。
- 瀏覽器和 Bevy 視窗驗收由使用者操作。那個"視窗驗收"指的是最終commit或push之前的驗收 不是指一切agent的視窗操作跟截圖都禁止
- Rust 核心測試使用已存 golden 資料，沒有 Node／舊 TS 執行依賴。重新產生 TS golden 時使用 `tools/regenerate-golden.py`，見 `docs/migration.md`。
- 不放寬原 TS 門檻來掩蓋差異。native／WASM 不逐位元一致時仍需驗證行為門檻並記錄差異；Pebble 靜止傾角已有一個 ignored 測試，原因尚未確立。
- `void-app` 保持不依賴 assembly；assembly、aero、vessels、multiscale 的獨立 lab 不必為了搬遷而整合入主遊戲。
- 修改後測試所屬 crate 與受影響整合場景。跨核心變更使用 `cargo test --workspace --all-targets`，lint 使用 `cargo clippy --workspace --all-targets -- -D warnings`。
- 舊 `void` 為參考封存，後續程式開發與提交在此 repository。搬遷來源與歷史對應見 `docs/migration.md`。
