# view

`crates/view`（`void-view`）是 `lab/view` 的移植：從船一路拉遠到整個星系的單一視圖。

| TS | Rust |
| --- | --- |
| `ViewCamera.ts`（map fade、up 轉向、co-rotation、single／split、`OrbitCamera`） | `camera.rs` |
| `ConicPath.ts`（依角度、依時間取樣的密切橢圓） | `conic.rs` |
| `PathFrame.ts`（inertial／surface 繪圖座標系、surface 中的天體軌道）與 orbit lab 的 `PathCache.ts` | `path_frame.rs` |
| `MapLayer.ts` 的狀態（軌道何時重算、船的路徑與拱點、標籤優先順序） | `map.rs`，不含繪圖 |

`map.rs` 只產生點和標籤；畫線、放標籤由呼叫端負責（example 用 gizmos 和 UI 文字）。`PathCache` 直接存 f64 的 frame 座標，不寫 three.js 的軸。

orbit lab 的 `Simulation.ts` 也移植到 `void-orbit`（`simulation.rs`）：手動油門與姿態模式、飛行計畫的燃燒、撞擊、預測在推力後重算。view 的船就是它。

## 檢查

`cargo test -p void-view --release`、`cargo test -p void-orbit --test simulation --release`：

- `golden/view.ts`：400 組 view state（各種焦點、距離、single／split）差 1.1e-16；40 台相機各 30 次拖曳、縮放、轉動、夾角差 2.1e-13；30 個橢圓差 1e-15（相對）；14 個天體在 Aurelia surface frame 中的軌道差 4.2e-12（相對，每 97 點取一點存檔）；`PathCache` 的取樣時間與 lab 相同，寫出的頂點完全相同。
- `golden/simulation.ts`：view lab 的設定下跑一段劇本（滑行、各姿態模式燃燒、hold、加速、飛行計畫的燃燒、撞擊），13 個檢查點的位置、速度、質量、預測、撞擊點都與 lab **逐位元相同**。
- lab 的 `view-check.ts` 全部檢查，數字與 lab 一致（拉遠時 up 最大一步 0.389°、相機作為第二個 LOD 觀察者 168 → 384 塊 tile 等）。lab 的路徑頂點是 f32，誤差 0.5 m；Rust 保持 f64，誤差 1e-9 m。

## example `view`

`cargo run -p void-app --example view`（`-- --view split` 是 KSP 的兩個視圖，`-- --altitude KM` 是起始軌道高度，預設 100 km）。

- 以 Aurelia 的本體座標為繪圖座標，相機在原點：Aurelia 的地形 tile 不用轉，其他東西（星曆的 f64 黃道座標）每幀轉換並減去相機位置。
- Aurelia 是 landing lab 的 hills 地形（lab/lod 四分樹），觀察者是船和相機；其他天體是球。
- 地圖：天體的軌道、船的預測路徑、拱點，以 map weight 的透明度畫出；標籤可點擊（map 超過一半時），重疊時只留圓點。

操作：拖曳環繞、滾輪縮放、Tab／Shift+Tab 換焦點（split 只在地圖）、點標籤換焦點、M 切換飛行／地圖（split）、F 切換 path frame（inertial／surface）、Space 暫停、`,` `.` 時間加速、Shift／Ctrl 油門、Z 全開、X 關、1–7 姿態（prograde、retrograde、normal、antinormal、radial out、radial in、hold）。

lab 的線框、tile 邊界勾選與 lab-log 沒有移植。
