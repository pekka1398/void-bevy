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

## 接縫掃描（搬遷後新增）

上面每項檢查都只固定一個場景。`crates/landing/tests/seams.rs` 把同樣的性質改成掃描多個狀態，種子固定並在每次執行時印出。門檻不用絕對公尺數，而是用**算術實際抵消的量級的 ulp**：行星離框架原點的距離，或飛行器離行星的距離，取較大者。Aurelia 在 1 AU，這個下限是 33 µm。

| 掃描 | 樣本 | 結果 |
| --- | --- | --- |
| 框架轉換來回 | 8000 狀態 × 2 行星 | 位置 1.7 ulp，速度 1.6 ulp |
| 旋轉框架方程 vs 慣性積分 | 300 段 10 s 弧 | 0.23 倍預算（ulp 下限 + 飛行距離的 1e-9） |
| 浮動原點移動 | 40 次移動 | 位置與速度**精確相等** |
| rails vs physics 滑行 | 5 種燃燒長度 | 5.6 ulp |
| 飛行↔接觸交接 | 23 次穿越 band 的跳躍 | 2.29e-2 m（lander.rs 單一跳躍的門檻是 0.1 m） |

浮動原點那項是精確相等，不是接近：body-fixed 位置存在 f64 記錄裡，只有 Rapier 的局部姿態是 f32，所以移動原點不花任何代價。這條斷言因此也是迴歸防護 —— 哪天回報的位置改成從 Rapier 讀，它會立刻失敗。`contact.rs` 原本允許 1e-3 m，那是 landing lab 自己的門檻，沒有動。

`void-assembly` 的 `separation_conserves_momentum_from_any_motion` 同樣把分離的動量守恆掃過 200 個翻滾狀態：線動量 6.4e-8、角動量 3.9e-8（相對量），正好是 Rapier 以 f32 存速度的下限；仍連接的零件位移精確為 0。分離是瞬時的，沒有推力、耗油或接觸，所以守恆是它唯一該做的事。

掃描抓到的是什麼：三次都是**測試自己的場景條件寫錯**，不是受測程式有錯 —— body index 取錯、觸地後仍繼續比較、以及在「距地面不到一個步進行程」的地方還宣稱是自由落體（以 170 m/s 下降時，1 秒的步進涵蓋 170 m，火箭可以在一步之內撞地並彈回離地 27 m，clearance 永遠讀不到 20 m 以下）。這是掃描的正常用途：先逼出有效狀態與判準，再談門檻。

`on_rails` 對燃燒長度的敏感也由此釐清，不是數值脆弱：掃過燃燒長度後，只要兩邊都還在滑行就一致到 5.6 ulp；低於約 90 s 的燃燒會讓無導引的拋物線在 200 s 窗口內落回地面，此時 rails 停在 band、physics 進了接觸，兩邊停的理由不同。該測試現在先斷言這個前置條件，失敗訊息直接說要加長燃燒。
