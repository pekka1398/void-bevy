# aero：空氣動力、加熱與再入

`crates/aero`（`void-aero`）移植 `lab/aerodynamics` 的物理，不依賴 Bevy。TS 主遊戲沒有用到這個 lab；這裡也還沒接進 `void-app`。

| TS | Rust |
| --- | --- |
| `Atmosphere.ts` | `atmosphere.rs`：`Atmosphere::earth()`／`Atmosphere::Vacuum`、`sample(海拔)`。1976 標準大氣到 86 km，之後等溫延伸，105–120 km 平滑降到真空 |
| `Aero.ts` | `aero.rs`：機身與翼面的力（`aerodynamic_forces`）、`wing_polar`、防熱盾遮蔽 `shielded` |
| `Thermal.ts` | `thermal.rs`：表皮／核心兩層熱容、傳導、輻射、對流、Sutton–Graves 駐點加熱、有限的燒蝕材 |
| `Vehicle.ts` | `vehicle.rs`：A-01 飛機、C-01 防熱艙、assembly 火箭（`demo_rocket()`，讀 `void-assembly` 的 `compile(&demo_craft())`），質量、重心、對角慣量 |
| `Loads.ts` | `loads.rs`：`evaluate_vehicle`、`advance_heat`（熱連結、過熱與受力上限） |
| `Flight.ts` | `flight.rs`：`AircraftFlight`，Rapier 平地 120 Hz、吸氣引擎、固定滾動起落架、撞地判定 |
| `Entry.ts` | `entry.rs`：`EntryFlight`，landing 的 `PlanetFrame` 加 orbit 的 `Dopri5<13>`，自轉球形 Terra 上的 6 自由度再入 |

和 lab 的差別：

- 錯誤一律 panic（lab 是 throw），訊息相同。
- 零件狀態（燃料、溫度）是依零件順序的 `Vec`，不是以 id 為鍵的 `Map`。
- 終止原因改成英文：`shield overheated`、`… over its aerodynamic load limit`、`destructive ground impact`、`touchdown at … m/s`。飛行器名稱也是英文。

## 檢查

`cargo test -p void-aero`。

### 與 lab 對照（`tests/golden.rs`）

對照資料由 `golden/aero.ts` 從 lab 產生。lab 的 `sin`、`cos`、`pow` 是 V8 自己的，與 fdlibm 差在最後一位，所以用到它們的值差幾個 ulp，其餘逐位元相同。每項會印出最大相對差：

| 項目 | 數量 | 最大相對差 |
| --- | --- | --- |
| 大氣（25 個高度） | 125 | 3.6e-16 |
| 翼面極曲線（兩種翼面，±180°，6 個 Mach） | 1800 | 3.8e-16 |
| 質量、重心、慣量（四種飛行器，滿油／空油） | 56 | 逐位元相同 |
| 力與力矩（四種飛行器各 30 個亂數狀態，含風、角速度、舵面、真空） | 6570 | 1.8e-15 |
| 各元素的氣流（速度、動壓、Mach、迎角、失速、CL、CD） | 3090 | 7.5e-16 |
| 各零件的熱負載與遮蔽 | 3150 | 3.8e-16 |
| 加熱步進（四組，各 20 步，含燒蝕材耗盡） | 720 | 3.3e-16 |
| 再入（有／無防護每 10 秒、一次 400 秒、陡峭翻轉例） | 1584 | 3.1e-10 |

再入的步數、接受的步數與終止時間也相同。陡峭例（100 km、6.5 km/s、−12°、迎角 160°、側傾 25°）以尾端朝前開始，約 45 秒在濃空氣中翻正；翻轉把 ulp 放大，所以之後改比物理量：到 150 秒為止，位置差 0.39 m、速度差 0.015 m/s、姿態差 0.0054 rad 以內。

飛機用原生 Rapier，lab 用 WebAssembly，兩者不會逐位元相同。30 秒內：巡航最多差 2 mm；跑道起飛（觸地滾行加上抬頭）最多差 1.3 m、0.27 m/s。

### lab 的 21 項檢查（`tests/checks.rs`）

`aero-check.ts` 全部移植，門檻相同：標準大氣與層界連續、無效輸入、真空與零空速、動壓與升力方向、共同風速不變性、極曲線、翼展方向氣流、舵面方向、靜穩定與滾轉阻尼、assembly 外形、防熱盾遮蔽、駐點熱縮放、熱傳導守恆、燒蝕材耗盡、飛機飛行與真空對照、跑道、持續飛行與起飛、撞地、行星共轉、真空再入能量守恆、再入回歸。

再入回歸的數字與 lab 相同：有防護 400 秒時 40,866 m／3,122 m/s，峰值動壓 17.17 kPa，峰值熱通量 0.648 MW/m²，剩 90.0 kg 燒蝕材；無防護 122.4 秒過熱。
