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
- lab-log：debug build 寫入 `lab/void-bevy/lab-log/flight.jsonl`（已被 .gitignore 排除），欄位順序與 lab 相同（`wall` 在前）。事件：`session`、每秒一次的 `flight-sample`（時間、模式、相機、地圖權重、co-rotation、tile 數與快取、主執行緒各階段耗時）、`focus`、`reset`、`path-frame`、`terrain-visibility`。lab 的 GPU 繪圖統計（draw call、三角形數、renderer 複本大小）沒有對應，`drawMs` 是 draw system 的 CPU 時間。
- 火箭和其他天體用 Bevy 的 StandardMaterial 與一盞平行光（1000 lux，在 Bevy 預設曝光下對應 scenery 的太陽照度 1），不經過大氣衰減，所以黃昏時火箭比地面亮。lab 也是一樣的簡化。

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
