# orbit：N 體星曆與船的軌道

`crates/orbit`（`void-orbit`）移植 `lab/orbit/src/orbit`。TS lab 已凍結（2026-10-01 的提交）：

| TS | Rust |
| --- | --- |
| `Kepler.ts` | `kepler.rs` |
| `SystemSpec.ts`（`buildSystem`、潮汐鎖定自轉） | `system.rs` |
| `Hermite.ts`（位置與速度） | `hermite.rs` |
| `Ephemeris.ts` | `ephemeris.rs`，實作 `void_frames::BodyStates` |
| `Dopri5.ts` | `dopri5.rs`（`Dopri5<N>`，維度是常數泛型） |
| `Trajectory.ts` | `trajectory.rs` |
| `VesselPropagator.ts` | `propagator.rs`：重力、J2、推力的四種控制（inertial、frenet、surface、force）、步長控制、撞擊偵測 |
| `Apsides.ts`、`Dominance.ts` | `apsides.rs` |
| `FlightPlan.ts` | `flight_plan.rs` |

沒有移植的部分：
- `Simulation.ts`：它是 lab 頁面的遊戲流程，之後由 Bevy 的 app 重新組裝。
- `ReferenceFrames.ts`：由 `void-frames` 取代。
- `frameAccelerationAt`：目前一律回傳 0。`lab/multiscale` 的 `FrameEphemeris` 會覆寫它，等移植 multiscale 時再改成 trait。

Rust 的介面和 TS 有兩點不同：
- 星曆不放在 propagator 或 flight plan 裡面，而是在呼叫時傳入（`&mut Ephemeris`），因為多個 propagator 共用同一份星曆。
- TS 用物件相同（`===`）判斷控制有沒有換，Rust 用值相等。因為求值是確定性的，結果一樣。

## 系統定義是資料

`systems/sol.json` 和 `systems/binary.json` 由 `golden/orbit.ts` 從 orbit lab 的 `SYSTEM_PRESETS` 匯出，Rust 用 serde 讀取，數值不在 Rust 裡手抄。欄位與 TS 的 `SystemSpec` 相同（camelCase），不認得的欄位直接報錯。

## 檢查（`cargo test -p void-orbit`）

| 檢查 | sol（15 個天體） | binary（8 個天體） |
| --- | --- | --- |
| 天體的衍生值（GM、週期、SOI、periapsis 比例） | < 1e-14（相對） | < 1e-14 |
| 自轉角度，含潮汐鎖定（絕對值） | < 1e-13 rad | < 1e-13 rad |
| **從 lab 的初始狀態積分 100 天** | **逐位元相同（差 0）** | **逐位元相同** |
| 從 Rust 自己建的初始狀態積分 100 天 | 0.28 m、1.1e-5 m/s | 0.084 m、2.1e-6 m/s |
| 能量漂移 100 天（lab 的值） | 2.7e-14（3.0e-14） | 2.5e-15（1.3e-14） |
| Kepler：偏近點角、狀態向量、密切軌道根數 | 4.4e-16 rad、6.9e-16、1.4e-14 | |

- 積分器本身與 TS 逐位元相同。第二列的差異全部來自 `build_system` 的初始狀態：TS 用 `Math.hypot`、`**`，Rust 用 `sqrt(dot)`、`powf`，兩者差在 ulp 等級，再沿軌道放大。真正的移植錯誤會是公里等級的差異。
- 潮汐鎖定衛星的 obliquity 很小，又是用 `acos(z)`、z ≈ 1 算出來的，條件數差，所以 ulp 差異會放大到約 1e-14 rad。這種角度用絕對誤差比較才有意義，相對誤差會被數值本身太小放大。
- 會 panic 的情況：查詢超出已積分範圍、查詢已釋放的時間、非根天體沒有軌道。

### 船（`tests/vessel.rs`，對照資料由 `golden/vessel.ts` 產生）

從 Aurelia 400 km、傾角 0.3 rad 的圓軌道出發，容差 1e-4 m、1e-7 m/s（與 orbit lab 頁面相同）：

| 情境 | 步數（與 lab 相同） | 與 lab 的差異 |
| --- | --- | --- |
| 滑行 2 天，含 8 個近／遠拱點 | 7492 | **逐位元相同**，拱點 9.3e-10 |
| surface、inertial、force 推力；撞擊 | 10、14、28、12 | **逐位元相同** |
| frenet 推力 300 s | 25 | 3.1e-5 m、9.3e-9 m/s |
| 飛行計畫：兩次燃燒，第三次被擋，撞上 Aurelia | 559 個樣本 | 拱點定位 3.0e-4 s；T+4 h 位置 3.3 mm；撞擊時間 5.9e-5 s |
| Dominance（4 個點） | | 相同 |

- frenet 的方向在 TS 用 `Math.hypot` 正規化，Rust 用 `sqrt(dot)`。這個 ulp 差異改變誤差估計，進而改變步長，所以結果不再逐位元相同，但遠小於積分器本身每步的容差。飛行計畫的燃燒也是 frenet，差異經過 559 步累積到毫米等級。
- 撞擊時間只用二分法求到 1e-4 s，撞擊點的狀態因此可以差「速度 × 1e-4 s」。這次是 38.6 km/s（質心系速度）× 5.9e-5 s = 2.3 m。
- 會 panic 的情況：推力讓質量低於乾重、撞擊後繼續積分。

## 速度

`cargo run --release -p void-orbit --example ephemeris_speed`，先熱身 10 天，再計時 100 天，與 TS 在 JIT 熱身後比較：

| | Rust release | TS（tsx） |
| --- | --- | --- |
| sol | 112 ms | 158 ms |
| binary | 20 ms | 32 ms |

快 1.4–1.6 倍，符合純量程式碼的預期。為了和 TS 逐位元一致，運算順序完全照抄，沒有 SIMD，也沒有多執行緒。之後要優化時，可以改用 SoA 加 SIMD，或把天體分組平行計算；屆時對照檢查改用容差即可，不再要求逐位元相同。
