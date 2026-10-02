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
3. scenery：地面與海的著色、大氣、雲、星空，跟著行星轉。
4. 地圖：飛行計畫的路徑與機動面板。

## 與 TS 的差異

- 繪圖座標系是行星的本體座標，相機在原點（TS 是黃道慣性座標，tile 與火箭再轉進去）。地形 tile 和火箭零件本來就是本體座標，不用轉；天體與地圖每幀從黃道轉入。兩者等價，所以 lab 的「本體座標轉 render 座標」檢查改為檢查火箭姿態在空間中的方向。
- HUD 暫時是純文字（UI 之後再做）。lab 中可點的 ALT/AGL、SURFACE/ORBIT、PATH 改成按鍵 K、L、G。
- dev 面板、lab-log、線框與 tile 邊界、碰撞線還沒移植。

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

燃燒 60 s 的爬升與 lab 不逐位元相同：Rapier 在 lab 是 wasm、這裡是原生，接觸階段（斜坡上 1.2° 的站立）就有小差異，10 s 時差約 2.5 m、0.9 m/s。兩邊的火箭在無轉向時都會傾倒，之後差異放大（lab 的撞擊在 T+100 s，Rust 在 T+85 s）。這是混沌放大，不是接線錯誤；lab 的門檻兩邊都通過。scenery 的 uniform 檢查等第 3 步再移植。
