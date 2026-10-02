# frames：樹狀座標系

實作在 `crates/frames`（`void-frames`），檢查在 `crates/frames/tests/frames.rs`。

太空遊戲的位置不可能用單一座標表達。`frames` 把「位置屬於哪個座標系」做成一棵樹，所有物理量都是「(座標系, f64 區域值)」；只有送 GPU 的最後一步才變成相對相機的 f32。

這個 crate 不依賴 Bevy，只用 glam 的 f64 型別（`DVec3`、`DQuat`，與 Bevy 0.19.1 同為 glam 0.32）。

## 精度：每一層只需要該層尺度的 f64

f64 的相對精度約 1.1e-16，誤差隨「到該層原點的距離」成長：

| 層 | 典型距離 | f64 解析度 |
| --- | --- | --- |
| tile 相對 tile 中心 | 1e4 m | 1e-12 m |
| 船／tile 中心相對天體 | 7e6 m | 1e-9 m |
| 天體相對恆星系質心 | 1.5e11 m（1 AU）～ 4.5e12 m（30 AU） | 3e-5 ～ 1e-3 m |
| 恆星系相對銀心 | 2.5e20 m | 3e4 m |

銀河層 30 km 的誤差只影響「恆星系在銀河的哪裡」，系統內部完全不經過那一層，所以 **每層 f64 就夠，不需要 i128 或更寬的型別**。前提是下面的第 2 條規則。

## 規則

1. **位置永遠帶著座標系。** `FramePoint { frame, position: DVec3 }`；需要速度時是 `FrameState { frame, position, velocity }`。沒有「全域座標」這種東西。
2. **轉換只走到最近共同祖先（LCA）。** 船到 tile 只經過天體自轉系，誤差是 1e-9 m 等級；若繞到恆星系質心再回來，會白白損失到 1e-5 m。根節點只在真的跨恆星系時才會經過。
3. **座標系是時間的函數。** 每個節點描述「相對父節點的剛體運動」，在時間 t 求值。
4. **f32 只在渲染邊界出現。** 物理與遊戲邏輯一律 f64。

## 節點

每個節點存相對父節點的運動，求值得到 `Motion`：

```rust
pub struct Motion {
    pub translation: DVec3,      // 本座標系原點，在父座標系中
    pub velocity: DVec3,         // 原點的速度，在父座標系中
    pub rotation: DQuat,         // 本座標系軸 → 父座標系軸
    pub angular_velocity: DVec3, // 本座標系相對父的角速度，在父座標系中
}
```

帶速度和角速度，是因為「地表速度 vs 軌道速度」、自轉系下的路徑、之後的 Krakensbane 都要轉換速度，不只轉換位置（對應 Principia 的 `RigidMotion`）。

節點種類，第一版：

| 種類 | 父節點 | 運動 | 對應 TS |
| --- | --- | --- | --- |
| `Root` | 無 | 恆等。暫定是恆星系質心、黃道軸；有銀河層時改掛在銀河之下 | `barycentric` |
| `BodyInertial(body)` | 質心系 | 原點 = 星曆給的天體位置與速度；軸固定為赤道軸 | `body-inertial`，`equatorialAxes` |
| `BodySurface(body)` | 該天體的 `BodyInertial` | 原點不動；軸繞極軸以 ω 旋轉 | `body-surface`，`bodyOrientation` |
| `Fixed(motion)` | 任意 | 常數剛體變換（tile、發射台） | 無 |
| `Free` | 任意 | 由模擬每步寫入（船、Krakensbane 的移動座標系） | 無 |

之後加入：`TwoBodyRotating(primary, secondary)`（TS 已有）、恆星系相對銀心。

**星曆不放在這個 crate**：`BodyInertial` 透過一個 trait 取得天體狀態，orbit crate 實作它，測試則用解析的 Kepler 軌道代替。

```rust
pub trait BodyStates {
    /// 天體在質心系的位置、速度，時間 t。t 超出星曆範圍要 panic，不能外插。
    fn body_state(&self, body: BodyId, t: f64) -> (DVec3, DVec3);
}
```

## 求值：每個時間點一份快照

```rust
let snapshot = tree.at(t, &ephemeris);          // 每幀（或每個預測樣本）一次
let m = snapshot.transform(from, to);           // 經 LCA
let p_in_to = m.apply_point(p_in_from);
let s_in_to = m.apply_state(state_in_from);     // 含 ω × r 項
```

- `Transform` 分兩段：`from` 往上到 LCA，再從 LCA 往下到 `to`。往下時先減後轉，即 `R⁻¹(p − T)`，和 orbit lab 的 `toFrame` 一樣；若預先合成成一個 `Motion` 再套用，會變成 `R⁻¹p − R⁻¹T`，兩個 1.5e11 量級的數相減，對照 TS 時相對誤差從 2.5e-13 變差到 5.2e-12。需要繼續合成時用 `to_motion()`。
- 目前沒有任何快取，每次都重新求值。等實際使用情況量出瓶頸再加。
- 樹的深度不到 10 層，LCA 直接往上走就好，不需要額外的資料結構。
- 自轉角用 `t.rem_euclid(period)` 先去掉整圈，再乘 2π。f64 的取餘是精確運算，所以 t 再大也不損失角度精度。最初設想的 `fract(t / period)` 沒有用，因為誤差在除法時就產生了。剩下的極限是 t 本身的間距：t = 1e9 s 時為 1.2e-7 s，相當於地表移動約 0.06 mm。
- `Free` 座標系記錄寫入時間，快照的時間必須完全相同，否則 panic。

## 渲染邊界（在 Bevy 那層，不在本 crate）

- 相機屬於某個座標系，也有自己的區域位置。
- 每個「錨點」實體（天體、tile、船）帶 `Anchor { frame, position, orientation }`。每幀把它轉進相機的座標系，減掉相機位置後轉成 f32，寫進頂層實體的 `Transform`。相機永遠在原點。
- 錨點底下的子實體（tile 頂點、零件）照常使用 Bevy 的 f32 階層，因為它們的 offset 本來就小。
- 這和目前 TS 版以焦點為中心繪製的做法相同，只是把它變成通用機制。
- 地圖尺度（1e13 m）的對數深度和距離壓縮，屬於 view 的工作，不在 frames 內處理。

## 檢查（`cargo test -p void-frames`）

| 檢查 | 結果 |
| --- | --- |
| 對照 orbit lab：`golden/frames.ts` 用 `BodyRotation.ts` 和 `toFrame` 產生 `tests/golden/frames.json`，涵蓋 4 種自轉 × 6 個時間 | 軸 2.8e-13，點 2.5e-13（相對） |
| 天體在 1 AU，地表 tile 和距它 1 m 的船：經 LCA vs 繞根節點 | 4.2e-10 m vs 2.9e-5 m |
| 地表靜止點轉到慣性系，速度 = ω × r | 0 |
| 月球上發射台的點轉到地球表面系，速度和位置有限差分的比較（同時檢查 `then` 與 `inverse`） | 4.9e-10（相對） |
| 經過月球來回轉換；直接轉換和分段合成的比較 | 6.4e-8、6.7e-8 m |
| t = 1e9 s 的自轉角：取餘法 vs 精確值，以及原本 `2πt/period` 的誤差 | 0 vs 1.0e-11 rad |
| 會 panic 的情況：Free 座標系未寫入就使用、寫入時間不符、旋轉四元數非單位長度、自轉參數無效 | 都會 panic |

golden data 要重新產生時，從 repo 根目錄執行 `python3 tools/regenerate-golden.py --reference-root ../void frames`。

## 待決定

- 根節點是太陽系質心。銀河層之後再加。
- `Free` 座標系由誰寫入、什麼時候寫入，等到船的物理移植時再定，屆時參考 Krakensbane。
- 雙體旋轉繪圖座標系（`two-body-rotating`）由 `void-orbit::FrameEvaluator` 提供，見 [orbit-lab](orbit-lab.md)；不屬於此樹狀框架 API。
