# landing：著陸、起飛與兩節火箭

`crates/landing`（`void-landing`）移植 `lab/landing` 的物理與載具；`crates/rotation`（`void-rotation`）移植 `lab/rotation`。畫面、HUD、頁面不在這裡。

| TS | Rust |
| --- | --- |
| `physics/PlanetFrame.ts` | `planet_frame.rs`（以及 `ContactFrame` trait） |
| `physics/ContactWorld.ts` | `contact_world.rs`：Rapier 0.35.1（TS 用同一版編成 WebAssembly），f64 浮動原點、半步速度、f64 姿態 |
| `planet/Planets.ts`、`terrain/TerrainTiles.ts` 的 `levelForTileSize`、`terrain/TerrainView.ts` 的 `landingLodOptions` | `planets.rs` |
| `vessel/Lander.ts` | `lander.rs` |
| `vessel/CoastPrediction.ts`、`vessel/EncounterPhysics.ts` | `coast.rs` |
| `vessel/PartJointRocket.ts` | `rocket.rs` |
| `vessel/DemoRocket.ts` | `demo_rocket.rs` |
| `lab/rotation/src/RotatingFrame.ts` | `crates/rotation` |

和 TS 的結構差異：
- 星曆是共用的，呼叫時傳入（`&mut Ephemeris`），不放在物件裡。
- Rapier 的 JS `World` 對應 Rust 的 `PhysicsWorld`；JS 綁定解除的 400 m/s 限速，在 Rust 是 `normalized_max_linear_velocity = f32::MAX`。旋轉的處理照 JS 綁定：`setRotation` 會把 f32 四元數正規化，建立剛體時不會。
- 兩節火箭在 TS 用物件身分共用同一個 `ContactWorld`；Rust 由火箭持有世界的清單，各節存索引。
- 每步轉向（SAS）在 TS 是控制物件裡的回呼；Rust 是 `advance` 的另一個參數。
- `ContactFrame::terrain_body()` 明確宣告地形所屬天體；`ContactWorld` 建立時拒絕非行星地表框架或半徑不一致的地形，與 TS 的檢查相同。Fleet 的地面框架轉交 PlanetFrame，FreeFallFrame 不允許地形。
- `tiles_around` 照 TS 的掃描順序回傳（碰撞 tile 依這個順序加進 Rapier）。

## 檢查（`cargo test --release -p void-landing`）

純數學逐位元對照（`golden/planet_frame.ts`、`golden/rotation.ts`）：
- `PlanetFrame`：Aurelia（Sol 系統）與 Pebble 的座標轉換和加速度（J2、潮汐、離心、科氏）：**差 0**。
- rotation：慣性矩、慣性力矩、自由旋轉步差 0；有力矩的一步 1 ulp；10 分鐘翻滾 3.7e-11（V8 的 sin、cos）。

Rapier 部分移植 `landing-check.ts` 的全部檢查與門檻，數字和 TS（WebAssembly 的 Rapier）並列：

| 檢查 | TS | Rust |
| --- | --- | --- |
| Rapier 自由飛行 150 s | 4.22e-3 m、1.18e-4 m/s、11 次原點移動 | 相同 |
| 箱子靜置在 50 m/s 的赤道上 10 分鐘 | 移動 0 m | 相同 |
| 球在 tile 間滾動 120 s | 2205 m，tile 載入／卸載 64／35 | 2201 m，64／35 |
| 飛行↔接觸交接，10 km 的跳躍 | 7.28e-3 m，283 個樣本 | 相同 |
| 跳躍後降落 | 頂點 13.6 km，觸地 0.40 m/s，707 kg 剩餘 | 相同 |
| 兩節火箭：分離、燃燒、雙向切換、助推器獨立落地 | | 全部相同（墜毀時速度變化 243 對 225 m/s，都遠超 10 m/s 的容許值） |
| 接觸與飛行的姿態一致 | 2.18e-6° | 相同 |
| 繪製地形 = 碰撞地形（5 顆行星） | 378／338／373／373／373 個 tile | 相同 |
| 每顆行星發射與返回 | Pebble 頂點 36.56 km | 36.82 km |
| 時間加速（on rails） | 一天移動 0 m；滑行差 6.1e-5 m | 0 m；2.45e-4 m |

差異只出現在劇烈接觸（彈跳、翻滾、撞擊），那裡 native 和 WebAssembly 的 Rapier 本來就會因最後幾位數不同而分岔。
