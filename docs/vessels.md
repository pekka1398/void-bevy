# vessels：多船、交會與物理交接

`void-vessels` 移植 `lab/vessels` 的 Fleet；不依賴 Bevy。`void-vessels-lab` 是獨立 Bevy 驗收程式，不接入 `void-app`，也不依賴 aero。主遊戲原本的 `PartJointRocket` 尚未替換。

```sh
cd lab/void-bevy
cargo run -p void-vessels-lab          # 預設地面發射
cargo run -p void-vessels-lab -- 2     # 1–6：發射、交會、滑行、旋轉分離、碰撞合併、SAS
cargo test -p void-vessels -p void-vessels-lab
```

## 所有權與 API

`Fleet` 擁有全部零件和連接。船是連通群；分離移除一條邊，合併新增一條邊。每艘船的 `Owner` 只允許一種推進方式：

- Orbit：`void-orbit::VesselPropagator` 推進 float64 質心，`void-rotation` 推進姿態。
- Bubble：附近船在一個 `void-landing::ContactWorld` 裡碰撞；原點由 `FreeFallFrame` 沿自由落體軌道移動，船只承受重力差。
- Ground：相同 ContactWorld 和 compound colliders，改用 `PlanetFrame` 與真正的碰撞地形。

共用 landing 的 `EncounterPhysicsGate`：進入 2 km、離開 2.5 km，包含下一段的最近接近預測。地面高度帶 200/400 m。交會連通群依地面天體分組，其餘兩艘以上共用 bubble，落單回 orbit；優先保留包含最多成員的既有 scene。

主要介面：

| API | 能力 |
| --- | --- |
| `launch` / `launch_landed` | 任意 assembly craft；加 vessel 前綴防止零件 ID 衝突 |
| `snapshot` / `part_snapshots` / `scene_snapshots` | 目前物理擁有者的船、零件姿態、燃料和 scene 診斷 |
| `relative` | 同 scene 時先用局部 float64 求相對位置、速度，避免相減 AU 尺度座標 |
| `set_control` / `control` / `set_sas` / `sas_phase` | 各船獨立且持續保留的油門與 SAS；轉向需要 command 模組 |
| `stage` / `stages_left` / `decouple` | 級數小的先執行，同級先分離再點火；分離出去的船油門歸零 |
| `free_nodes` / `node_frame` / `join` | 連接圖操作；合併不吸附、不旋轉對齊，保留相對姿態與線／角動量 |
| `inertia` / `fuel` / `thrust` | 即時質量分布、供油與推力 |
| `clearance` / `body_fixed_state` | 地面高度帶判斷及相對地表的狀態 |
| `terrain_tiles` / `terrain_geometry` | 直接讀已載入的碰撞網格供繪圖 |
| `advance` / `advance_on_rails` / `rails_blocker` | 共用時鐘；燃燒或未睡著的地面船阻止 rails；新交會或高度帶讓 rails 提早結束 |

`propulsion`、`burn`、`step_thrust` 是兩種擁有者共用的供油／推力實作，使用 assembly 的 `crossfeed_tanks`。同群引擎共用油箱、按存量比例耗油；軌道段在熄火時切斷，接觸步按步內實際剩餘燃料平均推力。燃料改變後重心、慣量及 live part poses 跟隨更新。

`FreeFallFrame::advance_origin()` 先準備下一個固定步的原點，`ContactFrame` 的唯讀加速度查詢可使用步前／步後原點；不複製重力公式。ContactWorld 仍使用原有 float64 自由位移和 leapfrog 半步速度。

## 驗收程式

預設暫停。P 開始／暫停，R 重設；1–6 換場景，Tab 或點船切換選取，Space 分級，T SAS，Shift/Ctrl 油門、X 關閉，W/S A/D Q/E 轉向。切船保留油門和 SAS，清除前一船手動轉向。左鍵拖曳相機，滾輪縮放，F 聚焦選取船，G 取景全部船，B collider 輪廓，O scene 原點。

按鈕提供單步、10/60 s 物理推進、附近生成（照 TS 頁面，每艘再遠一些：軌道上 25 m、地面 15 m 遞增）、直接分離（選一個接點仍連著的分離器）、debug join 與倍率。分級、分離、合併後立刻 `advance(0)` 更新擁有者。1/2/4x 為物理時間，20/100/1000x 為 rails。Debug join 只允許同 scene 的空接點、相同尺寸、距離不超過 0.25 m；core `join` 提供結構與動量操作，捕獲距離屬於呼叫端的政策。沒有 docking 磁吸、RCS 或自動捕獲。

零件外觀直接共用 `void-assembly-lab::parts::RenderAssets`（新增 library target，assembly 本身也改用它），沒有重寫模型。每幀先將慣性座標減去選取船的位置，再轉成 f32。地形畫 ContactWorld 的已載入三角網格。HUD 顯示船與 scene、相對距離／速度、燃料、控制、級數、交接事件；滑行與交會場景包含獨立軌道積分的取樣最大誤差。

UI 沒有照搬 TS 的 CSS、軌跡線或圖表，物理來源與場景維持一致。Bevy 畫面和操作由使用者驗收。

## 驗證與界線

TS 對照資料：

```sh
# repo 根目錄
npx tsx lab/void-bevy/golden/vessels.ts
```

`tests/checks.rs` 是 lab 的 `vessels-check.ts` 全部 38 項，門檻照 lab。Rapier 一邊是 native、一邊是 WASM，接觸相關的數字不逐位元相同，但印出的數值幾乎都和 lab 一致（交會 196／652 s、最近 40.3 m、分離 0.1128 m/s、熄火 94.58 s、跳躍高度 14.18 km、助推級 306 s 落地等）。只有 pebble 上的兩級火箭立地一項標為 `#[ignore]`：landing 的 ContactWorld 在細長火箭以底緣搖晃時會增加能量（這裡 30 s +190 J，lab 前 5 s +47 J，TS 也有，不是移植差異；搖晃本身是混沌的，兩邊 1 s 後就分開），lab 剛好 5.5 s、傾斜 1.5° 睡著，這裡 30 s、傾斜 5.0°。這是 landing 的問題，未在 vessels 修改。

`tests/fleet.rs` 另有：五個 owning TS 場景的姿態／位置／速度／零件對照、600 s bubble 滑行與獨立軌道比較、交會進出事件、旋轉分離動量及零件保留、接觸後合併動量與姿態、兩擁有者的燃燒、耗盡與分級、共用供油群、SAS、rails、地面睡眠及一日不漂移、地面→軌道→地面跳躍、rails 高度帶攔截、耗油後 live 重心／接點。另有 Bevy 系統存取、生成／切船／分級／重設及 debug join 檢查。

保留 TS 的範圍：沒有船撞擊毀損、gimbal、SAS 順行等模式或規劃姿態律；高速交會氣泡在存在期間不重新錨定速度。主遊戲整合與真正 docking 留到各自工作。
