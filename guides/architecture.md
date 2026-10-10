# 架構：每件事的唯一做法

要做下面這些事時，用表裡的現成設計，不另寫一套。找不到合適的接口就先告訴使用者，再決定要擴充哪裡。

| 要做的事 | 用這個 | 位置 |
|---|---|---|
| 表示位置、在不同座標系之間換算 | 座標樹 `FrameTree`、`FrameId`、`State`、`Transform` | `void-frames` |
| 從銀河尺度到船附近的位置 | `SplitPosition`（整數格 + f64 偏移） | `void-frames` |
| 算重力（點質量、J2） | `void_orbit::gravity::{pull, body_pull, oblateness}` | `void-orbit` |
| 查某一點的重力、空氣、地面、海 | `Environment` 的 `gravity`、`air`、`ground`、`surroundings` | `void-environment` |
| 零件受的力（氣動、浮力、推力等） | `void-modules`，從 `Environment` 讀環境 | `void-modules` |
| 地形高度（繪圖和碰撞都要） | 同一個 `Terrain`，繪圖的 tile 和碰撞的 tile 都從它取樣 | `void-terrain` |
| 船和零件的狀態 | `PartGraph`；一艘船是 graph 裡相連的一組零件，分離和對接是 graph 操作 | `void-assembly`、`void-vessels` |
| 船怎麼移動 | `Fleet`；同一份船的資料依情況切換 Orbit／Bubble／Ground（`VesselMode`） | `void-vessels` |
| 改變遊戲狀態（玩家操作、放置船、分級……） | `FlightSession::execute(Action)`；錄放、存檔都走這條 | `void-fleet-flight` |
| 主遊戲的世界設定 | `world::main_game` | `void-fleet-flight` |

## 原則

- **座標**：物理和位置一律用 f64，各自在所屬的 frame 裡算。畫面要先取相對相機的量，再轉成 f32。
- **大氣分兩種**：物理用的空氣在 `void-environment`；畫面上的天空、雲在 `void-scenery` 和 app 的 shader。兩者刻意分開。
- **邏輯放核心 crate**：物理和資料模型放在對應的 core crate，`void-app` 只負責操作、畫面和流程。
- **只讀和修改分開**：`FlightSession::sim()` 只能讀；要改狀態就加一種 `Action`，這樣錄放和存檔自動跟著支援。
- **試算不留痕跡**：零件模組在試算一步時，不扣燃料、不改模組狀態，只有確定接受的那一步才更新。
- **數學**：向量、四元數、矩陣一律用 glam 的型別和方法（`DVec3`、`DQuat`、`DMat3`）。自己包一層只為了加上檢查，例如零向量直接 panic。
- **測試資料**：測試用的星球、船、世界只放在測試看得到的地方。正式程式碼裡只有 `main_game` 這一個世界。
- **存檔格式**：格式或模擬規則一改，就升 `session.rs` 的 `FORMAT_VERSION` 或 `MODEL_VERSION`。版本不同的舊存檔直接拒絕，不做轉換。
