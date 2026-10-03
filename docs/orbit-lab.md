# orbit lab：操作與驗收

獨立執行，不依賴 `void-app` 或 assembly：

```sh
cargo run -p void-orbit-lab
cargo run -p void-orbit-lab -- --system binary
```

使用原 TS orbit 的 Simulation／FlightPlan 力學，提供 Sol／binary 系統、barycentric／body-inertial／body-surface／two-body-rotating 繪圖框架。框架在每個路徑點自己的時間求值，先以 f64 減焦點位置，再轉 f32 公里繪圖。

## 操作

- 上／下選設定，左／右變更；Shift 放大為 10 倍、Alt 為 100 倍。Enter 精確輸入數字，Enter 確認、Esc 取消。滑鼠在左面板滾動可看全部設定。
- 設定包括系統、框架與參考天體／雙體、焦點、天體歷史／船歷史／預測範圍、導航參考、起始軌道平面、plan coast、目標、選中燃燒、燃燒參考／開始時間／prograde／normal／radial Δv。時間單位秒，Δv 單位 m/s。
- N 加燃燒，Delete 刪除，Y／U 對齊 Pe／Ap，B 加速到選中燃燒前 30 秒。已排定燃燒依原 Simulation 自動執行；燃料不足或撞擊阻擋會顯示原因。
- 1–7 選姿態模式，Z 全推力、X 切推力，Shift／Ctrl 調節油門。
- Space 暫停，R 在目前模擬時間重設船；逗號／句號調整時間倍率。
- Tab／Shift+Tab 切換焦點，可點標籤；拖曳繞視角、滾輪縮放，F 配合目前焦點縮放。

歷史／預測路徑依時間漸暗，計畫燃燒區段有不同顏色；圖中包含 Pe／Ap、撞擊、燃燒位置、計畫與目標末端圓環、目標末端距離及各天體赤道／子午線。沒有更換或另寫軌道積分器。

## 數值驗收與界線

`cargo test -p void-orbit -p void-orbit-lab` 包含四框架 TS golden、原 TS 60 天雙體離軸門檻（1e-4 m）、旋轉表面固定點、實際 TS SceneView 的四框架路徑頂點／漸暗顏色、目標末端距離、warp 停止／燃燒執行與移除、重設／平面／燃料阻擋、數字編輯與 Bevy query 存取檢查。

對照資料由 `golden/reference_frames.ts` 與 `golden/orbit_scene.ts` 產生；重產需舊 TS 專案與 Node 依賴，Rust tests 直接使用已存 JSON。SceneView 對照使用 headless DOM 標記 stub，不開 GUI。

Bevy 場景使用簡單球體與方向光；這是力學觀察場景，沒有搬入 scenery。光照不是 TS point-light 的逐像素複製。面板與字型、滑鼠選取／拖曳、視覺外觀由使用者在本機執行視窗驗收；agent 實作時沒有開啟或操作 GUI。
