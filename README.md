# void-bevy

VOID 的 Bevy 移植 lab。Rust cargo workspace，每個功能一個 crate：`cargo test` 是 headless 檢查，`examples/` 是可開視窗的驗證場景。TS 各 lab 是參考實作，移植的 crate 以它們的輸出為對照。

```sh
cd lab/void-bevy
cargo run -p void-app      # 工具鏈檢查：一個打光的方塊
cargo test                 # 全部 crate 的檢查
```

Bevy 固定在 0.19.1（需要 Rust 1.95 以上）。API 以 `vendor/bevy` 的同版原始碼為準，那份是 shallow clone，被根目錄的 `vendor/` 忽略規則排除；編譯用 crates.io 的同一版本。

## crate

- `app`：目前只是開窗與 GPU 檢查。
- `frames`（`void-frames`）：樹狀座標系，不依賴 Bevy。設計與檢查結果見 [docs/frames.md](docs/frames.md)；對照資料由 `golden/frames.ts` 從 orbit lab 產生。
