# Fleet 飛行整合：第一輪

此工作依序調查 assembly／Fleet 承接主遊戲的缺口，建立獨立核心 `void-fleet-flight` 與 Bevy 程式 `void-fleet-flight-lab`。目前仍由使用者驗收 lab；`void-app` 保持使用 PartJointRocket，沒有加入 assembly 或 Fleet 依賴。

## 調查結果

| 項目 | 原本能否直接承接 | 第一輪處理／後續缺口 |
| --- | --- | --- |
| 自訂船、供油、分級、多船 | Fleet 已引用 assembly graph，並有部件／接點快照 | lab 直接讀 assembly 匯出的 craft JSON，以同一艘船建立地面與軌道實體；不是重寫固定兩級火箭 |
| 行星地形、ground／orbit／bubble | Fleet 已有 owner 交接與地形串流 | 使用主遊戲 GamePlanet 的行星／terrain 設定，獨立畫 LOD 地形與從 Fleet 讀回的碰撞線 |
| SAS | Fleet 已有逐船控制器，能跨 owner 重設框架 | 直接使用既有 SAS；沒有新增順行、目標等模式 |
| 時間加速 | Fleet 有 rails blocker、交會／高度帶攔截 | lab 提供 1／2／4 倍 physics、20／100／1000 倍 rails；阻擋時顯示原因，攔截後回 1 倍；尚未搬主遊戲依高度細分的 warp 上限 |
| 空氣施力 | Fleet 原本完全沒有 AirSource／AirField 接線 | 新增可選 FleetEnvironment；新核心提供各零件阻力，Orbit 每個 Dopri stage 評估、接觸世界逐步評估，bubble 與 ground 都有施力；rails 也保留空氣 |
| 噴嘴氣壓 | assembly catalog 的 Engine 只有真空 thrust／Isp，沒有出口面積 | 第一輪在 FleetAir 用顯式 engine ID → nozzle area 表，large 0.12 m²、small 0.15 m²；壓力降低引擎力、真空質量流率保持不變。未修改既有 catalog／golden；未評為正式零件資料模型定案 |
| 滑行預測 | predict_coast 原本只接受具體 Ephemeris，但 Fleet 持有 EphemerisSource | 改為接受既有 trait，可共用預測；與主遊戲一樣畫真空滑行。lab 的 C 是當下 600 s 預測快照，非持續刷新 |
| 有限燃燒機動 | FlightPlan 仍接受具體 Ephemeris；Fleet 沒有 Frenet 姿態律／機動執行入口 | 尚未接。需先釐清逐船計畫、有效引擎組合、minimum mass／供油變化、SAS 暫停／恢復及分離／合併時的計畫歸屬，不能只把 PlanEngine 的常數換成 Fleet.thrust |
| 撞擊毀損 | PartJointRocket 有 Crash，Fleet 沒有等價船／零件毀損政策 | 尚未接。這也是主遊戲替換前需要保留或明確重設的行為 |
| navball／map／scenery | renderer 部分可共用，但主遊戲 Game 持有大量固定上級／兩級假設 | 第一輪只共用地形 tile 與零件資產；沒有完整 navball／多天體 map／scenery 外觀 |
| 錄放 | 現有 session 格式只記兩級火箭的 mark，Game／step 在 app binary | 新 lab 尚未提供錄放。下一輪要把逐船 ID、生成／切船／分級與 owner 快照納入，而非假裝上級 mark 足夠 |
| 存檔／checkpoint | craft JSON 只保存組裝，Fleet live graph、owners、controls、lit／staged、pending time 沒有存讀介面 | 尚未做。應保存物理語意狀態並定義如何重建，不能直接序列化 Rapier handles；星曆、SAS、燃料／質心與時間也需包含 |

## 已建立的驗收入口

```sh
cargo run -p void-fleet-flight-lab
cargo run -p void-fleet-flight-lab -- --craft /path/to/void-craft.json
cargo run -p void-fleet-flight-lab -- --planet pebble
cargo run -p void-fleet-flight-lab -- --planet aurelia --vacuum
```

預設 Aurelia＋主遊戲 layered terrain，暫停開始。示範船是 assembly 的 craft（4590 kg、下級 90 kN），不是主遊戲的 7620 kg／120 kN 火箭。因此不以主遊戲軌跡相同或必須能進低軌道作這輪判準。外觀使用 assembly-lab 的實際資產；地形暫用 StandardMaterial，沒有複製完整 scenery 管線。

- P 暫停；Space 分級；Shift／Ctrl 節流，X 關閉；WASD QE 轉向；T SAS。
- Tab 切船（保留每船油門／SAS，清除離開船的手動轉向）；沒有 command 零件的船不提供轉向／SAS。
- N 在發射點附近生成同一 craft，每次再遠 30 m；O 直接建立同一 craft 的 400 km 軌道場景並切焦點。兩者都在同一 Fleet 時鐘中運行。
- `,` `.` 調倍率；C 畫當下真空滑行預測快照；R 回初始場景並暫停。
- 滑鼠左拖環繞、滾輪縮放；F2 繪圖線框、F3 tile 邊界、F4 實際碰撞地形線、F5 地形顯示。F4 此輪沒有零件 collider 疊圖，不把零件外觀當作 collider。

空氣與主遊戲維持 force-only 範圍：沒有氣動力矩、旋轉阻尼、熱、燒蝕或翼面。幾何用各零件尺寸、姿態與 live connections 更新；分離後原先被遮住的端面重新暴露，同半徑連接遮住端面，不同半徑保留肩部。不是完整氣動遮蔽演算法。

FleetEnvironment 會在每段起點建立純 AirSource，固定當段幾何／姿態；天體中心在短段內按起點速度線性外推，段長以 flight_chunk_seconds 限制。接觸步則取當下船的質心狀態計算加速度，與既有半步 thrust push 一起保存／交接。供油只由 Fleet 原路徑消耗一次。無環境時保留原 lab 行為，既有 golden／行為門檻不調整。

## 驗證與下一輪

本輪 `cargo test --workspace --all-targets`：226 passed、0 failed、3 ignored；workspace Clippy（`-D warnings`）、fmt 與新 lab build 通過。未開啟或操作 GUI。

Headless 檢查涵蓋自訂 craft 匯入→地面建船→分級與控制保留、氣壓推力與耗油、三種 owner 的阻力、高軌道真空對照、預測不推進 Fleet 時鐘，以及不開 OS 視窗的 Bevy 系統初始化／疊圖更新。有空氣的 16 s 滑行另做 Orbit／Bubble／rails 差分：rails 與 Orbit 相同；Bubble 在 1/60 s 下約差 0.287 m、0.0305 m/s，1/240 s 下約差 0.072 m、0.00763 m/s。測試要求四倍細分時誤差至少縮到 30%，並限制細分後的位置／速度誤差；此一階誤差來自接觸世界逐步取樣速度相依阻力，不能把它稱作逐位元一致。這些不能代替使用者的視窗操作驗收。

第一輪後優先補逐船錄放／checkpoint 與機動計畫的介面，並檢查撞擊毀損與主遊戲所有兩級假設；完成並驗收後才決定主遊戲替換。此階段不是 docking／RCS 或完整遊戲存檔。
