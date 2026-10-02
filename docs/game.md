# 主遊戲（void-app）

`cargo run -p void-app` 是 VOID 本體，移植 TS 主遊戲（repo 根目錄的 `src/main.ts`，lab/flight 跑的就是它）。各功能 crate 已各自對照過 lab，這裡只負責接線。

```sh
cargo run -p void-app                                   # Aurelia（Sol 系統）、分層地形
cargo run -p void-app -- --planet terra --terrain hills # 其他行星與地形
cargo test -p void-app --test flight                    # lab/flight 的接線檢查
```

## 分步

1. **飛行核心（完成）**：Sol 系統中的 Aurelia、scenery 的分層地形（發射點在緯度 0.3、經度 0.5 rad 的陸地上）、兩節火箭（地面附近 Rapier、飛行中軌道傳播）、分離、時間加速與 on-rails、lab/view 的單一視圖（從發射台拉遠到地圖）、地圖上的軌道、滑行預測、Pe/Ap 與標籤。
2. **儀表（完成）**：lab/navball 的球（150 px，下方中央，標記跟著 SURFACE／ORBIT），lab/sas 的 SAS（T，每個物理步呼叫一次）。
3. **scenery（完成）**：tile 用 scenery 的地面與海著色器，相機上是空氣與體積雲的 post-process pass（曝光 10^0.8、ACES），星空是慣性的（黃道座標），每幀轉進行星本體座標，在有陽光的大氣中淡出。Sol 系統中太陽是地圖畫的球，大氣 pass 的太陽圓盤關閉；單獨的行星則開啟。
4. **機動（完成）**：上級分離並自由飛行後，可規劃 orbit crate 的多段燃燒（Frenet 座標的 Δv，固定或自動參考天體，Pe／Ap 定位，加速到燃燒前 30 s）。燃燒以上級全推力執行，期間由燃燒控制姿態、SAS 暫停，結束後 SAS 重新鎖定。滑行時每次預測都把計畫重新錨定到目前的飛行；地圖上的橘線是計畫的路徑。

   lab 的面板改成按鍵（HUD 中的 MANEUVER 區）：N 新增、Del 刪除、`[` `]` 選擇、↑↓ prograde、←→ normal、PgUp／PgDn radial（每次 1 m/s，Alt 為 10）、Home／End 開始時間 ±60 s（Alt 為 600 s）、Y 定在 Pe、U 定在 Ap、V 切換參考天體（自動 → 各天體固定 → 自動）、B 加速到燃燒。

## 與 TS 的差異

- 繪圖座標系是行星的本體座標，相機在原點（TS 是黃道慣性座標，tile 與火箭再轉進去）。地形 tile 和火箭零件本來就是本體座標，不用轉；天體與地圖每幀從黃道轉入。兩者等價，所以 lab 的「本體座標轉 render 座標」檢查改為檢查火箭姿態在空間中的方向。
- HUD 暫時是純文字（UI 之後再做）。lab 中可點的 ALT/AGL、SURFACE/ORBIT、PATH 改成按鍵 K、L、G。
- dev 面板的大氣、雲、海、星空開關與曝光沒有移植（HUD 與面板維持純文字）。
- lab 的除錯開關改成按鍵：F2 地形網格邊（白）、F3 tile 邊界（紅）、F4 碰撞體（綠：火箭的碰撞形狀，以及每個 Rapier 地形碰撞體自己的三角形邊，從碰撞體讀回）、F5 畫地形（關掉做效能量測，LOD 與建 tile 照常）。疊加線的顏色先除以曝光，經過空氣 pass 的曝光與色調映射後接近原色。
- lab-log：debug build 寫入 `lab-log/flight.jsonl`（已被 .gitignore 排除），欄位順序與 lab 相同（`wall` 在前）。事件：`session`、每秒一次的 `flight-sample`（時間、模式、相機、地圖權重、co-rotation、tile 數與快取、主執行緒各階段耗時）、`focus`、`reset`、`path-frame`、`terrain-visibility`。lab 的 GPU 繪圖統計（draw call、三角形數、renderer 複本大小）沒有對應，`drawMs` 是 draw system 的 CPU 時間。
- 火箭和其他天體用 Bevy 的 StandardMaterial 與一盞平行光（1000 lux，在 Bevy 預設曝光下對應 scenery 的太陽照度 1），不經過大氣衰減，所以黃昏時火箭比地面亮。lab 也是一樣的簡化。

## 空氣（TS 沒有）

TS 主遊戲在真空飛行，aero 只在它自己的 lab 裡。這裡把 aero 接進主遊戲，是刻意的新增，不是移植：

- 行星帶 `air_density_scale`（`void-landing`）：Aurelia 與 Terra 是 `Some(1.0)`，Pebble 與 Luna 是 None，也就是完全沒有空氣場，不是處處為零的場。Aurelia 在 `sol.json` 裡就是地球（5.9722e24 kg、6371 km），所以用地球大氣不需要任何調整。
- 大氣模型是 aero 的 `EarthAtmosphere`，即 US standard 分層。0–50 km 與 ISA 表逐項相符（地面 1.2250、5 km 0.73643、10 km 0.41351 kg/m³）；86 km 以上是等溫近似，105–120 km 收到真空。模型只吃海拔，不含地形高度。
- 施力路徑：`void-orbit` 的 `AirSource`（自由飛行，Dopri5 每個 stage 都問）與 contact step 的既有外力 hook（Rapier，每個剛體各自問）。`void-landing` 不認識 aero，只負責把本體座標的力轉成慣性加速度（`PlanetAir`）；`void-app` 的 `aero_field.rs` 才接上 aero。沒有設定空氣場時行為與先前完全相同。
- 每一級是一個圓柱氣動體（front_cd 0.6、rear_cd 0.8、side_cd 1.1，照 aero lab 的火箭）。兩級相接時互相遮住對接面，但助推級較粗（半徑 1.5 m vs 1.05 m），露出的那圈肩部仍然吃阻力。
- **只有力，還沒有力矩**：火箭不會自己對準氣流（風標效應），氣動阻尼也還沒有。要補的話需要在這個 trait 上加力矩，並讓 contact step 與飛行姿態積分各開一條路。
- demo 火箭原本是照「無大氣」調的（8581 m/s Δv，起飛 TWR 2.07），對有大氣的地球尺寸行星不夠用；地球到低軌道手動飛大約需要 9400–9600 m/s。燃料因此加到 booster 5350 kg、upper 1470 kg，Δv 3682 + 5918 = **9600 m/s**，起飛 7620 kg、TWR 1.61（上面級點火時 1.15）。引擎、乾質量與外型都沒動，所以碰撞形狀與繪圖不受影響；油箱在同樣的殼裡裝更多，是刻意的取捨。
- 高 TWR 在有大氣時反而吃虧（低空衝太快，max-Q 與阻力損失都更大），所以 2.07 → 1.61 是改善而不是退步。
- 推力與比衝隨環境壓力變化：`LanderSpec` 的 `thrust_newtons`／`specific_impulse_seconds` 是**真空**額定，再減掉噴嘴出口面積乘以環境壓力（`nozzle_exit_area_m2`，壓力由同一個 `AirField` 提供）。booster 出口 0.12 m²，海平面 107.8 kN、Isp 279 s，是真空的 89.9%（Merlin 1D 為 90.7%）；upper 是真空噴嘴 0.15 m²，海平面只剩 24%，所以它本來就不該在低空點。出口面積 0.0 表示完全不隨壓力變化，等同加入空氣之前的行為。過度膨脹到推力為負時流動會分離，模型停在零而不是倒推。
- 自由飛行時壓力在每段起點取樣並在該段內維持不變（與整個 control 的處理一致）；contact step 則每步都讀。
- 垂直全推力的助推級熄火：真空 2366 m/s、116.4 km；有空氣 1631 m/s、65.9 km（`tests/air.rs`）。
- 滑行預測（`predict_coast`，HUD 的青色線）照舊不含空氣：它畫的是純彈道的滑行，在大氣層內實際落點會比線上的近。阻力會讓它差多少，是留給玩家自己判斷的事，不是要補的缺口。

## 檢查（`tests/flight.rs`）

lab/flight 的 `flight-check.ts`，門檻相同：

| 檢查 | Rust | lab |
| --- | --- | --- |
| 發射時直立；30 s 後在 1.20° 的斜坡上，空間中的軸與本體座標一致 | 2.9e-8 rad；差 2.1e-13 rad | 3.3e-8 rad；1.5e-7 rad |
| 跟地面轉的相機 6 h 漂移 | 3.6e-15 | 3.6e-15 |
| 地圖路徑與本體座標預測 | 1.1e-9 m | 2.3e-10 m |
| navball 跟著按鍵 | S → (0, 1.00e-2)，D → (1.00e-2, 0) | 相同 |
| 地形契約；畫的 tile 與碰撞 tile 相同 | 1221 個高度相同 | 相同 |
| 上級機動與 FlightPlan | 3.05e-5 m、質量差 0 | 3.08e-5 m、0 |

燃燒 60 s 的爬升與 lab 不逐位元相同：Rapier 在 lab 是 wasm、這裡是原生，接觸階段（斜坡上 1.2° 的站立）就有小差異，10 s 時差約 2.5 m、0.9 m/s。兩邊的火箭在無轉向時都會傾倒，之後差異放大（lab 的撞擊在 T+100 s，Rust 在 T+85 s）。這是混沌放大，不是接線錯誤；lab 的門檻兩邊都通過。lab 的「scenery 的光線與地形在行星旋轉與相機偏移下保持本體座標」檢查不需要：這裡的繪圖座標本來就是本體座標，空氣 pass 與地面著色器直接拿本體座標的相機與太陽。

## 錄放（搬遷後新增）

主遊戲的輸入不再由各系統各自讀鍵盤。`crates/app/src/input.rs` 的 `Input` 是「這一格飛行員做了什麼」的純資料（按住的鍵、這一格按下的鍵、滑鼠拖曳與滾輪，以及這一格涵蓋的模擬秒數），`read_input` 每格從視窗填一次，之後所有系統只讀這份資料。這跟 workspace 裡其他可替換來源（propagator 讀的星曆、火箭飛過的空氣）是同一個形狀，只是往外移到了飛行員這一層。

因此模擬本身變成純函式 `step(&mut Game, &Input)`：給定狀態與輸入，其餘什麼都不依賴。視窗用鍵盤餵它，錄影檔用當時的鍵盤記錄餵它 —— 這就是兩者會一致的原因。`Game` 的建構也抽成 `new_game(planet, terrain)`，不碰任何 Bevy 資產，所以 headless 測試能直接建一個遊戲。

- `cargo run -p void-app -- --record <檔案>` 一邊飛一邊寫
- `cargo run -p void-app -- --replay <檔案>` 把它飛回來，放完自動結束。星球與地形由檔案的 header 決定，`--planet` 不會覆蓋它 —— 同樣的輸入在另一顆星球上是另一次飛行
- 兩個旗標不能同時給：那等於錄一份加了視窗時序雜訊的副本

格式是每行一個 JSON 物件（跟 lab session log 一樣，可以用手讀和改）：一行 header、每格一行 frame、每約 60 格一行 mark。

**重現工具和回歸測試是兩件事**，只有第二件會自己報錯：

- **重現**：frame。足以把遊戲放回出問題的情境去看。光有錄影就只有這個 —— 重播一次 bug 再發生一次，然後沒有任何東西說話。
- **回歸**：mark。錄的時候每約 60 格寫一次狀態摘要（上級在行星固定框架中的位置、速度、質量、級數），重播時逐一比對。這才是讓一次飛行變成會自己失敗的檢查。

`crates/app/src/main.rs` 的測試兩件都驗：

| 測試 | 內容 |
| --- | --- |
| `a_recorded_session_replays_to_the_same_flight` | 7383 格的腳本發射（SAS、節流、點火、90 s 爬升、分離、滑行）錄進檔案再飛回來：123 個 mark **完全相同**，0.00e0 m |
| `a_changed_flight_fails_its_marks` | 把其中**一格** 1/60 s 的轉向輸入拿掉，飛行結果差 69.5 m —— 是 1e-3 m 門檻的七萬倍 |

第二個測試是重點：它證明 mark 真的抓得到改變，而不是只有「重播看起來一樣」。門檻 1e-3 m 不是行為容差，是算術自己的餘裕 —— 1 AU 下框架轉換要花 33 µm，見 [landing.md](landing.md) 的接縫掃描。

腳本為什麼是接近垂直的爬升：先前版本在 7 km、160 m/s 時做 2 秒 pitch-over，火箭就翻倒掉回地面。那是真實的火箭行為（氣動力矩還沒做，SAS 也擋不住那麼早的重力轉彎），不是遊戲的缺陷，但它讓腳本飛不到該測的地方。
