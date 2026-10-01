# orbit：N 體星曆

`crates/orbit`（`void-orbit`）移植 `lab/orbit/src/orbit` 的這些部分。移植依據是 2026-10-01 的工作副本，包含當時尚未提交的 `Hermite.ts` 重構：

| TS | Rust |
| --- | --- |
| `Kepler.ts` | `kepler.rs` |
| `SystemSpec.ts`（`buildSystem`、潮汐鎖定自轉） | `system.rs` |
| `Hermite.ts`（位置與速度） | `hermite.rs` |
| `Ephemeris.ts` | `ephemeris.rs`，實作 `void_frames::BodyStates` |

還沒移植：`VesselPropagator`、`Dopri5`、`FlightPlan`、`Apsides`、`Dominance`、`Trajectory`、`Simulation`，以及 split-coordinate ephemeris。

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

## 速度

`cargo run --release -p void-orbit --example ephemeris_speed`，先熱身 10 天，再計時 100 天，與 TS 在 JIT 熱身後比較：

| | Rust release | TS（tsx） |
| --- | --- | --- |
| sol | 112 ms | 158 ms |
| binary | 20 ms | 32 ms |

快 1.4–1.6 倍，符合純量程式碼的預期。為了和 TS 逐位元一致，運算順序完全照抄，沒有 SIMD，也沒有多執行緒。之後要優化時，可以改用 SoA 加 SIMD，或把天體分組平行計算；屆時對照檢查改用容差即可，不再要求逐位元相同。
