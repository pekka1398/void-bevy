# void-bevy

VOID 的 Bevy 移植 lab。Rust cargo workspace，每個功能一個 crate：`cargo test` 是 headless 檢查，`examples/` 是可開視窗的驗證場景。TS 各 lab 是參考實作，移植的 crate 以它們的輸出為對照。

```sh
cd lab/void-bevy
cargo run -p void-app --example system   # Sol 系統與地表探測器
cargo test                 # 全部 crate 的檢查
```

Bevy 固定在 0.19.1（需要 Rust 1.95 以上）。API 以 `vendor/bevy` 的同版原始碼為準，那份是 shallow clone，被根目錄的 `vendor/` 忽略規則排除；編譯用 crates.io 的同一版本。

## crate

- `app`（`void-app`）：Bevy 遊戲本體，會隨各 crate 的移植逐步長大；`cargo run -p void-app` 目前只是開窗與 GPU 檢查。各 lab 的視覺驗收是它的 example（`crates/app/examples/`），用 `cargo run -p void-app --example NAME` 執行。
- `frames`（`void-frames`）：樹狀座標系，不依賴 Bevy。設計與檢查結果見 [docs/frames.md](docs/frames.md)；對照資料由 `golden/frames.ts` 從 orbit lab 產生。
- `orbit`（`void-orbit`）：整個 orbit lab 的力學：Kepler、`buildSystem`、N 體星曆（Yoshida 8 階、Kahan、quintic Hermite，實作 `frames` 的 `BodyStates`）、船的傳播器（Dopri5、J2、推力、撞擊）、拱點、dominance、飛行計畫。見 [docs/orbit.md](docs/orbit.md)。
- `lod`（`void-lod`）：LOD lab 的立方體球四分樹：tile key 與相鄰、tile mesh、接縫縫合、選擇（含水平線剔除、相機像素限制、鄰居平衡、淘汰）。與 lab 逐位元相同，見 [docs/lod.md](docs/lod.md)。

- example `system`：用 Rust 星曆畫 Sol 系統，所有物體經 `frames` 轉成相對相機的 f32。驗收重點是 Aurelia 地表上 10 m 的探測器（probe）：Aurelia 離根節點 1 AU、以 30 km/s 運動，從幾公尺外看探測器仍要完全靜止，旁邊 1 m 的橘色方塊也不能晃動。

  ```sh
  cargo run -p void-app --example system            # 從探測器旁邊開始
  cargo run -p void-app --example system -- --focus aurelia --distance 2.2e7 --inertial
  ```

  - 操作：Tab / Shift+Tab 切換焦點，左鍵拖曳環繞，滾輪縮放（依高度縮放），C 切換「隨目標轉動 / 慣性」，`,` `.` 調整時間加速（1–1e7 倍），Space 暫停。
  - 啟動參數：`--focus NAME --distance M --yaw RAD --pitch RAD --warp 0..7 --inertial`。
  - 天體是真實比例的普通球體，沒有地形 LOD。球面網格最多比真實半徑低約 3 km，所以探測器看起來可能浮在多面體表面上，或陷進去。另外在慣性模式下貼近地面時，行星網格的頂點（f32、6e6 m 量級）可能有約 0.5 m 的抖動。這些是暫時用整顆球網格造成的限制，正是之後地形 tile 要各自掛 anchor 的原因，與 frames 本身無關。
  - 紅點是本初子午線與赤道的交點，藍點是北極；天體標籤是螢幕空間文字。

對照資料要重新產生時，從 repo 根目錄執行 `npx tsx lab/void-bevy/golden/<name>.ts`。serde_json 開了 `float_roundtrip`：預設的解析器可能差一個 ulp，golden 檢查看得出來。
