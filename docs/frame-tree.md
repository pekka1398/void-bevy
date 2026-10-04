# 座標樹統一（branch `claude/adoring-darwin-qogeug`）

目標：所有用到座標的地方都走同一棵 `void-frames` 樹，從銀河、恆星系、天體、地表，到接觸場景的浮動原點、船、相機。轉換一律經最近共同祖先（LCA）；f32 只出現在渲染邊界。

## 改之前

`FrameTree` 只有 `system` example 在用。實際程式各寫各的：

| 位置 | 自己的座標表示 |
| --- | --- |
| orbit | 星曆與船都在「恆星系質心、黃道軸」的裸 `DVec3`；`body_orientation` 自己算天體軸 |
| landing | `PlanetFrame::to_body_fixed／to_inertial` 手寫自轉轉換；`ContactWorld.origin` 浮動原點 |
| vessels | `FreeFallFrame` 手寫平移；Fleet 的 `to_inertial`／`scene_local`／`axes`；對外全部回傳質心系絕對座標 |
| view | `PathFrame`、`frame_axes`、`orbit_in_surface_frame` 又各算一次天體軸 |
| fleet-flight／app | 相機在質心系算，繪圖時每個零件「場景 → 質心系（1e11 m）→ 母星地表系 → 減相機」，繞根一圈 |
| multiscale | `SplitPosition`、`FramedState`（以字串選系統）、`FrameEphemeris`；與樹完全無關 |

同一個天體軸公式至少有四份，船畫在螢幕上要繞經 1 AU 尺度的根節點，正是 frames.md 規則 2 要避免的。

## 目標樹

```
Galaxy（根，非旋轉；所有恆星系共用此軸）
└─ System[s]          恆星系質心；平移是 SplitPosition（整數格＋f64），根層相減精確
   ├─ BodyInertial[b] 天體中心、赤道軸，不轉
   │  └─ BodySurface[b]  隨天體自轉＝地面接觸場景的座標系
   │     ├─ 場景的浮動原點（Rapier 的 f32 都相對它；給繪圖用）
   │     └─ 場景內的船（零件座標系）
   ├─ 交會氣泡（沿自由落體軌道移動，不轉）
   │  ├─ 場景的浮動原點
   │  └─ 場景內的船
   ├─ 軌道上的船（傳播器的質心＋姿態）
   └─ 相機（掛在焦點的座標系下）
```

- **只有根層用 split。** 兩個點的 LCA 是根時，先把兩個 System 的 split 位置精確相減，再接 f64；其他層全部 f64。
- **單一恆星系的世界**（目前主遊戲）就是只有一個 System、位於原點的銀河。之後 multiscale 的 `CoupledWorld` 直接當成 System 層的來源。
- **天體軸只有一份公式**（`void-frames::Spin`），其他 crate 全部改用它。

## 誰提供什麼

| 來源 | 提供 |
| --- | --- |
| 星曆（orbit `Ephemeris`、multiscale） | 天體相對自己恆星系質心的狀態；恆星系相對銀河的 split 狀態 |
| Fleet | 氣泡、浮動原點、船的運動（Dynamic 節點，`Fleet` 本身就是樹的來源，只在 Fleet 時鐘有效） |
| presentation／app | 相機節點 |

物理定律不搬：`PlanetFrame` 的重力／離心／Coriolis／潮汐、`FreeFallFrame` 的潮汐加速度仍是「在這個座標系裡的運動方程」，但座標轉換改由樹做。星曆的 `EphemerisSource` 仍是物理積分用的批次介面（每個 Dopri stage 一次取全部天體），它和樹用同一套 split 算術與同一份天體軸公式。

## 步驟與結果

八步都已完成，每步全部測試與 clippy 通過後才提交。使用中的 API 與檢查整理在 [frames.md](frames.md)。第 4、6 步改了畫面（主遊戲、multiscale example 與 lab），使用者已於 2026-10-04 完成視窗驗收。

| 步驟 | 結果 | 與原計畫的差異 |
| --- | --- | --- |
| 1. frames 核心 | `SplitPosition` 移入 `void-frames`；根是銀河，System 節點，根層精確相減；節點可移除、換父；Dynamic 節點；`Spin::body_axes` 是唯一的天體軸公式 | 無 |
| 2. 來源 | `EphemerisSource: BodyStates + FrameSource`，多了 `system_count`／`system_of`／`origin_system`；`SystemFrames` 從任何星曆建樹 | 無 |
| 3. Fleet | Fleet 本身是樹的來源；氣泡、浮動原點、船是 Dynamic 節點；場景轉換、快照、發射、淨空都走樹 | 沒有做 `state_in(frame)`：改成讓快照帶 `frame` 與區域位姿。船的節點掛在場景的接觸座標系（地面＝BodySurface），浮動原點是同層節點，因為 Rapier 位置本來就以接觸座標系記錄 |
| 4. fleet-flight／app | 零件、碰撞網格、tile、計畫軌跡直接轉到相機座標系；HUD、navball、scenery、map 的軸從樹取 | 相機不存進樹，而是每次繪圖時掛在焦點座標系下的子座標系（`Transform::into_child`），免得擋住船的 join、換 owner、移除 |
| 5. view | `PathFrame` 是星曆樹上的 BodyInertial／BodySurface；地表軌道取樣用 `Spin::body_axes` | 無 |
| 6. multiscale | `CoupledWorld` 是樹的來源；example 與 lab 經樹畫；銀河位置用 `from_galaxy`／`to_galaxy` 進出 | `FramedState` 保留：它是 System 層的物理積分狀態，可以離自己的恆星系好幾光年，放進 f64 節點會失去精度 |
| 7. 其餘 | orbit-lab 的四種繪圖座標系都是樹上的節點（新增 `TwoBody`）；landing 的 `PlanetFrame` 轉換經樹，舊火箭、著陸器、滑行預測一併改道 | seam-check 不改：它比對的是手寫平坦參考與 split 世界，那段算術就是被測對象 |
| 8. 文件 | frames.md 改寫，本頁與 status.md 更新 | 無 |

沒有改的：`PlanetFrame`／`FreeFallFrame` 的加速度（物理定律）、air 模組在積分器任意時刻的軸計算（不經 Fleet 時鐘，直接用 `Spin` 的同一公式）、landing 舊火箭與著陸器只依自轉的方向換算（同一公式）。

### golden 對照的變化

使用者說明目前不需要 TS golden 當約束，但既有測試保留，門檻都沒有放寬：

- **自轉角**：`Spin::angle` 先取精確餘數。第一圈內與 lab 逐位元相同；過了第一圈，lab 的 `2πt/period` 會捨入。`landing/tests/planet_frame.rs` 與 `view/tests/view.rs` 改為以重現的 lab 公式對 golden（原門檻），native 的精確角度另外量差異（Pebble 6.4e-15、地表軌道 1.7e-11 相對）。
- **`PlanetFrame` 來回**：lab 的 `back` 是它對自己慣性狀態做的轉換；我們的慣性狀態在 1 AU 與 lab 差不到一個 f64 間距，所以改用 lab 的慣性狀態當輸入（1e-6 m 門檻不變，實測 4.7e-9 m），另外檢查我們自己的來回在 4 個間距內。

## 驗證原則

- 既有 TS golden 門檻不放寬。天體軸改為單一公式後，若某項 golden 因此超出門檻，記錄原因並回頭檢查公式選擇，不調門檻。
- 新增精度檢查：三萬光年外的兩個恆星系間轉換、地表 1 m 外的船經 LCA 轉換、相機座標系下的零件位置，誤差與「繞根」路徑對照。
- 每一步跑 `cargo test --workspace --all-targets` 與 `cargo clippy --workspace --all-targets -- -D warnings`。

## 審查修正

- plain lab 相機的 up 仍取選中船所在地；天體焦點只改相機的中心，不以天體中心的零向量計算 up。檢查逐一聚焦所有天體，再切回船，camera transform 必須有限。
- multiscale example 聚焦探測器時保留 split 銀河位置；探測器與軌跡先做 split 相減，天體經樹得到 split 銀河位置後再相減。不能先把離星系數光年的探測器壓成 f64 相機平移。檢查跨星系近距離偏移與探測器的公尺級軌跡。
- `MODEL_VERSION` 4 → 5：自轉公式與 Fleet 座標轉換的捨入已改變，舊版精確 mark 不保證相容。舊存檔與錄影直接以模型版本不相容拒絕，不等到重播或還原時才出現狀態差異。
