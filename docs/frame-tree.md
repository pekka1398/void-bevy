# 座標樹統一（進行中，branch `claude/adoring-darwin-qogeug`）

目標：所有用到座標的地方都走同一棵 `void-frames` 樹，從銀河、恆星系、天體、地表，到接觸場景的浮動原點、船、相機。轉換一律經最近共同祖先（LCA）；f32 只出現在渲染邊界。

## 現況（改之前）

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

## 步驟（每步全部測試通過才往下）

進度：1–7 完成（4、6 的畫面待使用者視窗驗收）。orbit-lab 的四種繪圖座標系都是樹上的節點（雙體旋轉是新的節點種類 `TwoBody`，角速度取兩體連線在其平面內的轉速，與 lab 的週期定義一致，平面本身的緩慢轉動不計）；landing 的 `PlanetFrame::to_body_fixed`／`to_inertial` 經樹，舊火箭、著陸器、滑行預測因此一併改道。seam-check 比對的是手寫的平坦參考與 split 世界，那段算術就是被測對象，不改。multiscale：`CoupledWorld` 本身是樹的來源（`frames(origin)` 建出所有恆星系與天體），example 與 multiscale-lab 都經樹畫到掛在焦點座標系下的相機；銀河位置用 `Snapshot::from_galaxy`／`to_galaxy` 進出（同一套根層精確相減）。探測器的 `FramedState` 仍是「相對某恆星系質心的 split 狀態」：它是 System 層的物理積分狀態，可以離自己的恆星系好幾光年，不放進 f64 節點。view 的 PathFrame 是星曆樹上的 BodyInertial／BodySurface；地表座標系的軌道取樣改用唯一的 `Spin::body_axes`（golden 比對改為對 lab 公式的重現，原門檻不變，native 的精確角度另外量差異）。相機不是存在樹裡的節點，而是每次繪圖時掛在焦點座標系（選中船的零件座標系，或聚焦天體的 BodyInertial）下的子座標系（`Transform::into_child`）：船會被 join、換 owner，相機若存進樹會擋住這些生命週期；每次算一次則沒有這個問題，精度一樣由共同祖先決定。零件、碰撞網格、地形 tile 都帶自己的座標系（`frame` + 局部位姿），app 直接轉到相機座標系。船的節點掛在場景的接觸座標系（地面＝BodySurface），不是浮動原點下：Rapier 的位置本來就以接觸座標系記錄，掛在這裡不必多一次相減；浮動原點是同層的節點，給繪圖用。

1. **frames 核心**：`SplitPosition` 移入 `void-frames`；根改為 Galaxy，新增 System 節點與根層精確相減；節點可移除（id 不重用）；`Spin` 提供唯一的天體軸公式，orbit 的 `body_orientation` 改用它。
2. **來源**：星曆提供恆星系資訊，`Ephemeris`（單系統）與 `FrameEphemeris`（多系統）都能建樹。
3. **Fleet**：Fleet 持有樹；場景原點、船是節點；拿掉手寫的 `to_inertial`／`scene_local`／`axes`；對外狀態帶座標系，需要時用 `state_in(frame)` 表示。
4. **fleet-flight／app**：相機成為節點，零件、地形、碰撞線、預測線都直接轉到相機座標系；HUD 的高度、速度、主導天體從樹取。
5. **view**：繪圖座標系（inertial／surface）就是樹上的 BodyInertial／BodySurface；拿掉 view 自己的軸公式。
6. **multiscale**：`CoupledWorld` 成為 System 層的來源；`FramedState` 改用樹的 System 節點；example 與 lab 經樹繪圖。
7. **其餘**：orbit-lab 的繪圖座標系（含雙體旋轉）成為節點種類；landing 的舊火箭、seam-check 改用樹。
8. **文件**：frames.md 改寫、status.md 更新。

## 驗證原則

- 既有 TS golden 門檻不放寬。天體軸改為單一公式後，若某項 golden 因此超出門檻，記錄原因並回頭檢查公式選擇，不調門檻。
- 新增精度檢查：三萬光年外的兩個恆星系間轉換、地表 1 m 外的船經 LCA 轉換、相機座標系下的零件位置，誤差與「繞根」路徑對照。
- 每一步跑 `cargo test --workspace --all-targets` 與 `cargo clippy --workspace --all-targets -- -D warnings`。
