# frames：座標樹

實作在 `crates/frames`（`void-frames`），檢查在 `crates/frames/tests/frames.rs`。這次統一的過程與取捨見 [frame-tree.md](frame-tree.md)。

遊戲裡的位置都屬於某個座標系：銀河、恆星系、天體、地面場景、船、相機。`frames` 把這些座標系做成一棵樹，所有位置都是「(座標系, f64 區域值)」。轉換只走到兩個座標系的最近共同祖先；只有在跨恆星系、兩者在根相遇時，才用 split 位置（整數格加 f64）精確相減。最後送 GPU 時才變成相對相機的 f32。

這個 crate 不依賴 Bevy，只用 glam 的 f64 型別（`DVec3`、`DQuat`，與 Bevy 0.19.1 同為 glam 0.32）。

## 精度：每一層只需要該層尺度的 f64

| 層 | 典型距離 | f64 解析度 |
| --- | --- | --- |
| 船／零件相對相機 | 1e1 ～ 1e4 m | 1e-15 ～ 1e-12 m |
| 船、tile 相對天體 | 7e6 m | 1e-9 m |
| 天體相對恆星系質心 | 1.5e11 m（1 AU）～ 4.5e12 m（30 AU） | 3e-5 ～ 1e-3 m |
| 恆星系相對銀河 | 3e20 m（30,000 光年） | `SplitPosition`：i128 格（2³² m）＋ f64 偏移，精確 |

規則：

1. **位置永遠帶著座標系。** 對外的快照（零件、碰撞網格、地形 tile）都附帶 `frame` 和區域位姿；「慣性座標」只是物理視角所在恆星系（`origin_frame`）裡的座標。
2. **轉換只走到最近共同祖先。** 船到相機、船到 tile 都不經過 1 AU 的那一層。
3. **座標系是時間的函數。** 每個節點描述相對父節點的剛體運動，在時間 t 求值。
4. **f32 只在渲染邊界出現。**

## 節點

每個節點求值得到相對父節點的 `Motion`（平移、原點速度、旋轉、角速度，對應 Principia 的 `RigidMotion`）。

| 種類 | 父節點 | 運動 |
| --- | --- | --- |
| `Root`（銀河） | 無 | 非旋轉；所有恆星系共用它的軸 |
| `System(s)` | 根 | 恆星系質心，`FrameSource::system_state` 給 split 位置與速度 |
| `BodyInertial(b)` | 天體所屬恆星系 | 天體中心（`body_in_system`）；軸固定為赤道軸 |
| `BodySurface(b)` | 該天體的 `BodyInertial` | 繞極軸自轉；角度是 `Spin::angle` |
| `TwoBody(p, s)` | 兩天體所屬恆星系 | 兩體質心；x 由主天體指向副天體，z 沿相對角動量；角速度取 \|r × v\| / r² 繞 z（與 orbit lab 的週期定義一致，平面本身的緩慢轉動不計） |
| `Fixed(motion)` | 恆星系以下任意節點 | 常數剛體變換 |
| `Free` | 恆星系以下任意節點 | 由模擬寫入，只在寫入的時間有效 |
| `Dynamic(key)` | 恆星系以下任意節點 | 每次求值都向來源要 `dynamic_motion(key, t)`，所以永遠跟著擁有者的即時狀態 |

節點可以移除（只能移除葉節點，id 不會再發出）；Free、Fixed、Dynamic 可以換父節點（換到別處的 Free 必須重新寫入）。

## 來源

```rust
pub trait FrameSource {
    fn system_state(&self, system: SystemId, t: f64) -> (SplitPosition, DVec3);
    fn body_in_system(&self, body: BodyId, t: f64) -> (DVec3, DVec3);
    fn dynamic_motion(&self, key: u64, t: f64) -> Motion; // 預設 panic
}
```

- **星曆**（`void-orbit::EphemerisSource: BodyStates + FrameSource`）同時有兩種視角：物理積分用的「全部天體相對 `origin_system` 質心」（`BodyStates`、`states_at`），和樹用的「每個天體相對自己的恆星系」。兩者來自同一份狀態，測試確認樹的 inertial → origin 轉換等於 `body_state`。單一恆星系的 `Ephemeris` 是位於銀河原點的一個 System。
- **`SystemFrames`**（orbit）從任何星曆建出樹的固定部分：每個恆星系、每個天體的 inertial／surface，以及物理視角所在的 `origin`。
- **`CoupledWorld`**（multiscale）本身是 `FrameSource`；`frames(origin)` 建出所有恆星系與天體。
- **`Fleet`**（vessels）本身是 `FrameSource`：氣泡的自由落體原點、場景的浮動原點、船的零件座標系都是 Dynamic 節點，只在 Fleet 的時間有效。地面場景的接觸座標系就是該天體的 `BodySurface`。

天體軸只有一份公式：`Spin::body_axes`（`void-orbit::body_orientation` 直接呼叫它）。

## 求值

```rust
let snapshot = tree.at(t, &source);            // 每幀（或每個樣本）一次
let m = snapshot.transform(from, to);          // 經最近共同祖先
let p = m.apply_point(p_in_from);
let s = m.apply_state(state_in_from);          // 含 ω × r
let camera = m.into_child(&camera_motion);     // 到一個不存在樹裡、掛在 to 下的座標系
let q = snapshot.from_galaxy(&split, to);      // 銀河 split 位置進出座標系
let g = snapshot.to_galaxy(from, p);
```

- `Transform` 分兩段：`from` 往上到共同祖先，再往下到 `to`；往下時先減後轉（`R⁻¹(p − T)`），和 orbit lab 的 `toFrame` 一樣。需要繼續合成時用 `to_motion()`，那會失去先減後轉的精度。
- 共同祖先是根時，兩個恆星系的 split 位置精確相減（`bridge`），再接 f64。`transform_via_root` 故意全程 f64，只給檢查用。
- 自轉角用 `t.rem_euclid(period)` 先去掉整圈再乘 2π：第一圈內與 lab 的 `2πt/period` 逐位元相同，之後保留 lab 公式丟掉的精度。
- 沒有快取，每次重新求值。

## 渲染邊界

相機不是存在樹裡的節點，而是每次繪圖時掛在焦點座標系下的子座標系（`Transform::into_child`）：

- 焦點是選中船的零件座標系（點在質心），或聚焦天體的 `BodyInertial`。相機原點在眼睛，軸是畫面要用的軸（主遊戲用母星的地表軸）。
- 每個要畫的東西從自己的座標系轉進相機座標系，再轉 f32。船上零件到相機的共同祖先是船所在的場景或船本身，不經過 1 AU 那層。
- 相機若存進樹，會擋住船的 join、換 owner、移除這些生命週期；每次算一次就沒有這個問題，精度一樣由共同祖先決定。
- 地圖尺度的對數深度和距離壓縮屬於 view，不在 frames 內處理。

## 誰用它

| 使用者 | 怎麼用 |
| --- | --- |
| `Fleet` | 場景座標轉換（`to_inertial`／`scene_local`／`axes`）、快照、發射、地面淨空都走樹；`frames()`、`vessel_frame`、`scene_frames`、`body_frames`、`origin_frame` 對外 |
| fleet-flight／app | `CameraSample::to_camera`；零件、碰撞網格、tile、計畫軌跡經樹畫；HUD、navball、scenery、map 的軸從樹取 |
| view | `PathFrame` 是星曆樹上的 BodyInertial／BodySurface |
| orbit-lab | `FrameEvaluator` 的四種繪圖座標系都是樹上的節點 |
| landing | `PlanetFrame::to_body_fixed`／`to_inertial` 經星曆樹（舊火箭、著陸器、滑行預測因此一併使用） |
| multiscale example／lab | 經樹畫到掛在焦點下的相機；探測器的 `FramedState` 仍是相對恆星系的 split 物理狀態 |

物理定律不搬進樹：`PlanetFrame` 的重力／離心／Coriolis／潮汐、`FreeFallFrame` 的潮汐加速度是「在這個座標系裡的運動方程」，仍在各自的 crate。

## 檢查（`cargo test -p void-frames`）

| 檢查 | 結果 |
| --- | --- |
| 對照 orbit lab：`golden/frames.ts` 用 `BodyRotation.ts` 和 `toFrame` 產生 `tests/golden/frames.json`，4 種自轉 × 6 個時間 | 軸 2.8e-13，點 2.5e-13（相對） |
| 天體在 1 AU，地表 tile 和距它 1 m 的船：經共同祖先 vs 繞根 | 4.2e-10 m vs 2.9e-5 m |
| 兩個恆星系在 30,000 光年外：跨銀河精確相減 vs 繞根 f64；銀河 split 位置進出座標系 | 0 m vs 1.2e3 m；進出與 bridge 相同 |
| 地表靜止點轉到慣性系，速度 = ω × r | 0 |
| 月球上發射台的點轉到地球表面系，速度和位置有限差分的比較 | 4.9e-10（相對） |
| 經過月球來回轉換；直接轉換和分段合成的比較 | 6.4e-8、6.7e-8 m |
| t = 1e9 s 的自轉角：取餘法 vs 原本 `2πt/period` | 5.6e-17 vs 1.0e-11 rad |
| 不在樹裡的子座標系（相機）保有共同祖先的精度 | < 1e-8 m |
| 節點移除、換父節點、Dynamic 向來源取值 | 通過 |
| 會 panic 的情況：Free 未寫入或時間不符、非單位四元數、自轉參數無效、移除有子節點的節點、使用已移除的節點、沒有 Dynamic 的來源被問 Dynamic | 都會 panic |

其他 crate 的對應檢查：orbit `system_frames_agree_with_the_ephemeris`、multiscale `system_frames_agree_with_the_physics_view`（跨恆星系，含地表軸）、vessels `vessel_frames_follow_their_physics_owner`（場景、氣泡、join、存檔還原）、fleet-flight `the_camera_frame_agrees_with_the_inertial_eye`、landing `planet_frame_matches_the_landing_lab`、orbit `four_frames_match_ts`。

golden data 要重新產生時，從 repo 根目錄執行 `python3 tools/regenerate-golden.py --reference-root ../void frames`。
