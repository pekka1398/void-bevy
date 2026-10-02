# void-bevy

VOID 的 Bevy／Rust／native Rapier 開發主線，從原 TS 專案的移植 lab 獨立而來。Rust Cargo workspace，每個功能一個 crate：`cargo test` 是 headless 檢查，`examples/` 是可開視窗的驗證場景。TS 各 lab 是參考實作，移植的 crate 以它們的輸出為對照。

```sh
cd void-bevy
cargo run -p void-app                   # 主遊戲
cargo run -p void-app --example system   # Sol 系統與地表探測器
cargo test                 # 全部 crate 的檢查
```

Bevy 固定在 0.19.1（需要 Rust 1.95 以上），編譯使用 crates.io。API 可查 Cargo registry 的同版原始碼；舊專案的 `vendor/bevy`／`vendor/rapier` 僅是參考，不是此 workspace 的編譯依賴。

開發規則見 [AGENTS.md](AGENTS.md)，原筆記保留於 [NOTE.md](NOTE.md)。來源歷史、搬遷與 golden 重產方式見 [docs/migration.md](docs/migration.md)。

## crate

- `app`（`void-app`）：Bevy 遊戲本體。`cargo run -p void-app` 是主遊戲（TS 主遊戲 `src/main.ts` 的移植），分步進行，見 [docs/game.md](docs/game.md)。各 lab 的視覺驗收是它的 example（`crates/app/examples/`），用 `cargo run -p void-app --example NAME` 執行。
- `frames`（`void-frames`）：樹狀座標系，不依賴 Bevy。設計與檢查結果見 [docs/frames.md](docs/frames.md)；對照資料由 `golden/frames.ts` 從 orbit lab 產生。
- `orbit`（`void-orbit`）：整個 orbit lab 的力學：Kepler、`buildSystem`、N 體星曆（Yoshida 8 階、Kahan、quintic Hermite，實作 `frames` 的 `BodyStates`）、船的傳播器（Dopri5、J2、推力、撞擊）、拱點、dominance、飛行計畫。見 [docs/orbit.md](docs/orbit.md)。
- `lod`（`void-lod`）：LOD lab 的立方體球四分樹：tile key 與相鄰、tile mesh、接縫縫合、選擇（含水平線剔除、相機像素限制、鄰居平衡、淘汰），以及 lab 自己的地形 `DemoTerrain`（`presets/planets.json` 的參數）。與 lab 逐位元相同，見 [docs/lod.md](docs/lod.md)。
- `terrain`（`void-terrain`）：landing 的地形契約與設定、scenery 的分層行星、landing 的 hills。畫面和碰撞用同一個取樣器。與 lab 逐位元相同，見 [docs/terrain.md](docs/terrain.md)。
- `rotation`（`void-rotation`）：旋轉座標系中的剛體姿態（lab/rotation）。
- `landing`（`void-landing`）：行星旋轉座標系、Rapier 接觸世界、著陸器、兩節火箭、滑行預測、行星。見 [docs/landing.md](docs/landing.md)。
- `scenery`（`void-scenery`）：scenery lab 的 CPU 部分：大氣與三張表（穿透率、多重散射、天空輻照度）、雲的天氣圖與 3D 噪聲體積、星空、軌道視角。與 lab 逐位元相同，見 [docs/scenery.md](docs/scenery.md)。著色器在 `void-app` 的 `src/scenery.rs` 與 `src/shaders/scenery/`。
- `sas`（`void-sas`）：sas lab 的姿態穩定控制器，與 lab 逐位元相同，見 [docs/sas.md](docs/sas.md)。
- `navball`（`void-navball`）：navball lab 的姿態球幾何與繪圖（RGBA 緩衝區），見 [docs/navball.md](docs/navball.md)。
- `view`（`void-view`）：view lab 的相機規則（地圖淡入、up 轉向、co-rotation、single／split）、密切軌道、繪圖座標系與地圖狀態，見 [docs/view.md](docs/view.md)。`void-orbit` 另有 orbit lab 的 `Simulation`。
- `math`（`void-math`）：和 V8 一致的 `hypot` 與 fdlibm 函數，供各 crate 共用。
- `assembly`（`void-assembly`）：零件、堆疊接點、供油、分級、JSON 存檔與平地 Rapier 試飛。Bevy 編輯器在獨立的 `assembly-lab`（`void-assembly-lab`），用 `cargo run -p void-assembly-lab` 開啟；開發期間不讓 `void-app` 依賴 assembly。見 [docs/assembly.md](docs/assembly.md)。
- `aero`（`void-aero`）：aerodynamics lab 的大氣、機身與翼面氣動力、零件加熱與燒蝕、三種試驗飛行器、Rapier 飛機與防熱艙再入。頁面在獨立的 `aero-lab`（`void-aero-lab`），用 `cargo run -p void-aero-lab` 開啟。見 [docs/aero.md](docs/aero.md)。
- `vessels`（`void-vessels`）：多船、軌道／交會氣泡／地面接觸交接、推力、分離與合併、SAS 和時間加速。獨立驗收程式 `cargo run -p void-vessels-lab`，六個場景；不接入 `void-app`。見 [docs/vessels.md](docs/vessels.md)。
- `multiscale`（`void-multiscale`）：multiscale lab 的 split 座標（整數格加 float64）、多恆星系耦合 N 體世界、隨系統質心的座標系與光年航行探針，與 lab 逐位元相同。頁面是 example `multiscale`（`cargo run -p void-app --example multiscale`）；兩船相撞場景是獨立的 `void-multiscale-lab`。見 [docs/multiscale.md](docs/multiscale.md)。
- `void-multiscale-lab`：遠方恆星系的雙船碰撞與合併，`cargo run -p void-multiscale-lab`；使用共用 world 的 FrameEphemeris 與 vessels Fleet，見 [docs/multiscale.md](docs/multiscale.md)。
- example `lod`：LOD 四分樹，預設在分層地形上（`-- --terrain lod` 是 LOD lab 自己的大陸，`-- --terrain sphere` 是依層級著色的光滑球）。操作照 lab/lod：左鍵拖曳平移、右鍵拖曳繞行星中心、Shift+左鍵轉視角、滾輪縮放；P 把相機移到探測器上方，方向鍵／PageUp／PageDown 移動探測器，`,` `.` 調整探測器的最小格像素（0 為關閉），V、H、B 切換相機 LOD、地平線剔除、線框。
- example `landing`：landing lab 的頁面：兩節示範火箭在旋轉的行星上（`-- --planet pebble|luna|terra|aurelia|aurelia-fast`，預設 Pebble），各節周圍以碰撞層級畫地形，青色線是關掉引擎後的滑行預測。Space 點火／分離、Shift／Ctrl 油門、W/S A/D Q/E 轉向、左鍵拖曳環繞、滾輪縮放、1/2/3 時間倍率、P 暫停、R 重來、B 線框。
- example `scenery`：scenery lab 的頁面：lab/lod 的 tile 用 lab 的地面與海著色器、星空、空氣與體積雲（同一個全螢幕 pass，依深度一起積分）、太陽圓盤、從地面到 200,000 km 的軌道視角、lab 的曝光與 ACES。`-- --terrain layered|lod|hills --at 緯度,經度 --preset ground|sunset|night|cloud|plane|orbit|space --tone aces|agx|neutral`。滑鼠同 lab；1–7 預設視角、`,` `.` 時間、R 時間倍率、`[` `]` 太陽赤緯、`-` `=` 海平面、Z X 曝光、T 色調映射（ACES、AgX、Neutral）、K L 雲量、A M C W O S 開關（大氣、多重散射、雲、只看天氣、海、星）。
- example `sas`：sas lab 的頁面：示範火箭的真實慣量、無阻尼的姿態積分、SAS 開關、踢、調參與 15 秒圖表。T 開關、WASD QE 轉向、K／Shift+K 踢、V 整節／上級、`-` `=` 慣量倍率、1–8 調參、0 預設、R 重來。
- example `navball`：navball lab 的頁面：320 px 與 150 px 兩顆球。WASD QE 依機體軸轉動，`[` `]` 緯度、J L／I K 速度方位與俯仰、`-` `=` 速度、R 重來。
- example `view`：view lab 的頁面：從船 8 m 拉遠到 2e13 m，地圖淡入、相機轉向北方；`-- --view split` 為 KSP 的兩個視圖。拖曳、滾輪、Tab、點標籤、M、F（path frame）、Space、`,` `.`、Shift／Ctrl、Z、X、1–7，見 [docs/view.md](docs/view.md)。

- example `system`：用 Rust 星曆畫 Sol 系統，所有物體經 `frames` 轉成相對相機的 f32。驗收重點是 Aurelia 地表上 10 m 的探測器（probe）：Aurelia 離根節點 1 AU、以 30 km/s 運動，從幾公尺外看探測器仍要完全靜止，旁邊 1 m 的橘色方塊也不能晃動。

  ```sh
  cargo run -p void-app --example system            # 從探測器旁邊開始
  cargo run -p void-app --example system -- --focus aurelia --distance 2.2e7 --inertial
  ```

  - 操作：Tab / Shift+Tab 切換焦點，左鍵拖曳環繞，滾輪縮放（依高度縮放），C 切換「隨目標轉動 / 慣性」，`,` `.` 調整時間加速（1–1e7 倍），Space 暫停。
  - 啟動參數：`--focus NAME --distance M --yaw RAD --pitch RAD --warp 0..7 --inertial`。
  - 天體是真實比例的普通球體，沒有地形 LOD。球面網格最多比真實半徑低約 3 km，所以探測器看起來可能浮在多面體表面上，或陷進去。另外在慣性模式下貼近地面時，行星網格的頂點（f32、6e6 m 量級）可能有約 0.5 m 的抖動。這些是暫時用整顆球網格造成的限制，正是之後地形 tile 要各自掛 anchor 的原因，與 frames 本身無關。
  - 紅點是本初子午線與赤道的交點，藍點是北極；天體標籤是螢幕空間文字。

日常編譯與測試不需舊 TS。對照資料要重新產生時，從此 repo 根目錄執行 `python3 tools/regenerate-golden.py --reference-root ../void <name>`（舊參考需已安裝 Node 依賴）；詳見 [搬遷說明](docs/migration.md)。serde_json 開了 `float_roundtrip`：預設的解析器可能差一個 ulp，golden 檢查看得出來。

- `void-orbit-lab`：Sol／binary、四種繪圖框架、歷史／預測／機動計畫／目標路徑，以及燃燒編輯與自動執行。`cargo run -p void-orbit-lab`，操作見 [docs/orbit-lab.md](docs/orbit-lab.md)。
- 詳細移植盤點與尚存缺口見 [docs/port-audit.md](docs/port-audit.md)。
