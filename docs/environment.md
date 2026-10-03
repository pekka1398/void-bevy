# 環境介面（branch `claude/environment-interface`）

目標：一個環境介面。給「樹上某個座標系裡的某個狀態」（通常是相對天體中心或地表，不是相對根），回傳那裡的重力、大氣、地形、水。阻力、加熱、浮力、引擎背壓由零件模組從這份取樣算，不再各自去找大氣模型和天體半徑。

求解器不變：軌道積分（含 rails）、交會氣泡（自由落體座標系）、地面接觸是同一艘船的不同推進方式，由系統挑選。它們讀同一份環境，各自加上自己座標系的運動方程項。

座標表示沿用 [frames.md](frames.md) 的樹；本頁只處理「那個位置的環境是什麼」。

## 改之前

### 重力：三份實作

| 位置 | 定律 | 用途 |
| --- | --- | --- |
| orbit `Field::gravity`（`propagator.rs`） | 每個天體點質量＋J2（繞自轉軸），減去座標原點加速度 | 軌道積分的每個 Dopri stage；`VesselPropagator::gravity_at` 對外 |
| landing `PlanetFrame::acceleration` | 本體點質量＋J2（繞 +z，手寫一份）；其他天體只有點質量潮汐；離心＋Coriolis | 地面場景 |
| multiscale `CoupledWorld::gravity_in` | 只有點質量（建構時要求 J2 = 0），split 位置相減 | 多恆星系探測器 |

`FreeFallFrame` 的潮汐用 `gravity_at(o + r) − gravity_at(o)`，已經和軌道積分共用同一份。

### 大氣：三條路

| 位置 | 高度零點 | 空氣速度 | 備註 |
| --- | --- | --- | --- |
| fleet-flight `FleetAir`／`CraftAir`（主遊戲） | `body.radius_meters` | 地表自轉的空氣靜止 | `AirSource` 拿不到星曆，天體中心從取樣時刻線性外插；引擎背壓用船的位置另算一次 |
| landing `PlanetAir`＋app `RocketAir`（舊火箭） | `terrain.radius_meters` | 同上 | 只用於 landing 舊路徑 |
| aero `EntryFlight` | `body.radius_meters` | 同上 | lab；golden 對照 |

大氣模型本身（`Air`、`Atmosphere::{Earth, Vacuum}`、`EarthAtmosphere`）在 aero，`aerodynamic_forces(elements, state, air, wind, controls)` 只吃已取好的 `Air`。

aero `AircraftFlight` 是平地世界（高度＝y，另有風場），不在本次範圍。

### 地形

`Terrain::height(direction)` 是從參考球（`terrain.radius_meters`）起算的高度。主要使用者：

- Fleet `launch_landed`、`clearance_over`（經 `GroundSpec.terrain`）。
- landing：滑行預測、舊火箭、著陸器。
- `ContactWorld` 用它產生碰撞 tile。

產生碰撞 tile 是建網格，不是取樣，所以不改；環境只負責把同一份 `Arc<Terrain>` 交出去。

### 水

只在畫面上有，physics 沒有任何水。

- `GamePlanet.sea_level` 給 scenery 著色；主遊戲在 `fleet_game.rs` 另外重算一份同樣的值，交給地面 shader 和畫面散射大氣。
- 只有 layered 畫海（`ocean_enabled`）。
- hills 有大氣時用 1800 m，那是地面色帶和畫面散射大氣的零點，不是海。

### 高度零點不一致

physics 的大氣高度從參考球起算。畫面散射大氣的零點卻在參考球之上：layered 是 5000 m（海平面），hills 有大氣時是 1800 m。主遊戲的 layered 發射台實測如下（Earth 大氣模型，`density_scale` = 1）：

| | 高度 | 密度 | 氣壓 |
| --- | --- | --- | --- |
| 畫面（海平面起算） | 79.7 m | 1.216 kg/m³ | 100.4 kPa |
| physics（參考球起算） | 5,079.7 m | 0.730 kg/m³ | 53.5 kPa |

physics 的發射台空氣只有預期的 60%：阻力偏小，引擎背壓損失也偏小，大氣頂也低了 5 km。修正會改變飛行行為，所以列在下方「待決定」，由使用者決定。

## 目標介面

```rust
// void-orbit::gravity：唯一的重力定律
// r = 點 − 天體中心；axis = 自轉軸；兩者在同一組軸。c = 1.5 J2 GM R²（點質量為 0）。
pub fn pull(gm: f64, c: f64, axis: DVec3, r: DVec3) -> DVec3;

// void-environment
pub struct BodyEnvironment {
    pub atmosphere: Option<Atmosphere>,
    pub air_datum_meters: f64,          // 大氣高度零點，從地形參考球起算（目前是 0）
    pub terrain: Option<Arc<Terrain>>,
    pub sea_level_meters: Option<f64>,  // 從地形參考球起算；None 表示沒有海
}

impl Environment {
    /// `frames` 指出樹上每個天體的 BodyInertial／BodySurface（Fleet 的樹本身就是
    /// SystemFrames 加上 Dynamic 節點）。`state` 是 `from` 座標系裡的狀態。
    pub fn sample<S: FrameSource + ?Sized>(
        &self,
        at: &Snapshot<S>,
        frames: &SystemFrames,
        from: FrameId,
        state: State,
        body: BodyId,
    ) -> Sample;
}

pub struct Sample {
    pub gravity: DVec3,               // `from` 的軸；所有天體；不含座標系本身的慣性項
    pub up: DVec3,                    // `from` 的軸，離開 `body` 中心
    pub radius: f64,                  // 到 `body` 中心
    pub air: Option<AirSample>,       // 沒有大氣或在大氣頂以上時為 None
    pub ground: Option<GroundSample>, // 沒有地形時為 None
    pub sea: Option<SeaSample>,       // 沒有海時為 None
}
pub struct AirSample  { pub altitude: f64, pub air: Air, pub airspeed: DVec3 } // airspeed 是相對空氣的速度，`from` 的軸
pub struct GroundSample { pub height: f64, pub clearance: f64 }               // 地形高（參考球起算）、離地高度
pub struct SeaSample  { pub depth: f64 }                                      // 海面下為正
```

### 怎麼算

- **在天體自己的座標系裡算，再轉回 `from`。**
  - 先用樹把狀態轉到 `body` 的 BodySurface。轉換只走到最近共同祖先：船在地面場景時不經過 1 AU 那層。
  - 大氣、地形、水都在 BodySurface 取：空氣隨地表轉，所以在那裡的速度就是相對空氣的速度。
  - 兩個共點物體的相對速度與觀察座標系無關（換座標系的 V + ω × r 兩邊相同、互相抵消），所以 `airspeed` 只要轉軸。
- **重力是場，不是運動方程。**
  - 每個天體 k 的拉力在它的 BodyInertial 軸裡算（自轉軸就是 +z），再轉回 `from`。
  - 同一點在 BodyInertial 和 BodySurface 的重力是同一個向量，只是軸不同。
  - 離心、Coriolis、座標原點加速度、潮汐相減，取決於「在哪個座標系積分」，所以留在求解器裡。這和座標樹「物理定律不搬進樹」的做法一致。
- **由呼叫者指定天體。**
  - Fleet 用場景所屬的天體；軌道上的船和 HUD 用 orbit 既有的 `DominanceTree`。
  - 重力仍然是所有天體的總和。
- **沒有 fallback。**
  - 「沒有大氣、沒有海、在大氣頂以上」是合法的物理狀態，回傳 None。
  - 在天體中心取樣、低於大氣模型定義域（−5 km）、非有限輸入，都直接 panic，和現在一樣。

### 熱路徑

軌道積分每個 Dopri stage 都要重力，不能每次建 snapshot 再逐一轉換。積分器保留批次取天體位置的寫法，但每個天體的拉力改呼叫同一個定律（`add_pull`）。

`PlanetFrame` 的本體與潮汐、`FreeFallFrame`（經積分器的 `gravity_at`）也都用它，再各自加上自己座標系的項。multiscale 是同一條點質量定律，但保留自己 lab 的捨入（見下方進度）。

空氣只在大氣內才需要，所以積分器裡的 `AirSource` 改走環境取樣。

### 誰提供什麼

| 來源 | 提供 |
| --- | --- |
| 星曆（`CelestialBody`） | GM、J2、自轉（重力定律的參數） |
| 星球設定（landing `LandingPlanet`、app `GamePlanet`） | 每個天體的 `BodyEnvironment`：大氣模型與密度倍率、大氣零點、地形、海平面 |
| 座標樹 | 從 `from` 到天體座標系的轉換 |
| 零件模組（aero、引擎、之後的浮力） | 讀 `Sample`，自己算力與熱 |

### 放哪個 crate

- 新增 `void-environment`，依賴 math、frames、orbit、terrain。
- 大氣模型（`atmosphere.rs`）從 aero 搬過去，aero 重新匯出，所以 aero 的 golden 測試與 aero-lab 不用改。
- landing、vessels、fleet-flight、aero 都可以依賴它，不會產生循環。
- 重力定律放在 orbit（`void_orbit::gravity`），因為積分器在 orbit 裡；環境只呼叫它。

## 步驟

每步全部測試與 clippy 通過才提交並 push 到 `claude/environment-interface`。

| 步驟 | 內容 | 行為是否改變 |
| --- | --- | --- |
| 1. 重力定律 | `void_orbit::gravity::pull`。積分器 `Field::gravity`、`PlanetFrame::acceleration`（本體＋潮汐）、multiscale `gravity_in` 都改呼叫它 | 只有捨入順序。另外，`PlanetFrame` 的潮汐會多出其他天體的 J2（積分器本來就有），量出差異並記錄 |
| 2. `void-environment` | 搬入大氣模型；`BodyEnvironment`、`Environment::sample` | 不改（新程式） |
| 3. Fleet／fleet-flight | 從 `LandingPlanet`／`GamePlanet` 建環境；`GroundSpec` 的地形改由環境提供；`clearance_over`、`launch_landed` 用 `GroundSample`。`FleetAir` 的空氣與引擎背壓用 `AirSample`。`AirSource::acceleration` 多拿星曆，取代天體中心線性外插 | 只有外插那一項。量出差異並記錄 |
| 4. landing 與 aero | `PlanetAir`／`RocketAir`、`EntryFlight` 的大氣經環境取樣 | 不改；`EntryFlight` golden 門檻不放寬 |
| 5. 待決定項目 | 依使用者決定：大氣零點、海 | 改（若決定要改） |
| 6. 文件 | 本頁、aero.md、landing.md、fleet-flight.md、status.md | 無 |

### 進度

| 步驟 | 結果 | 與原計畫的差異 |
| --- | --- | --- |
| 1. 重力定律 | `void_orbit::gravity`：`pull`、`add_pull`（累加形式）、`body_pull`、`oblateness`。積分器 `Field::gravity`、orbit-lab 起始圓軌道速度的徑向重力、`PlanetFrame` 的本體與潮汐都改用它。測試：極點與赤道的解析值、等於 J2 位能的負梯度、隨軸旋轉、多天體加總與 lab 寫法逐位元相同 | 見下 |
| 2. `void-environment` | 大氣模型從 aero 搬來（`git mv`，aero 重新匯出 `Air`、`Atmosphere`、`EarthAtmosphere`、`smooth`、`validate_air`，aero 的 golden 不受影響）。`Environment::new(bodies).with(body, BodyEnvironment)`；查詢分成 `gravity`、`surroundings`、兩者合併的 `sample` | 從星球設定建環境的函式移到第 3 步：environment 不能依賴 landing（landing 之後要用它） |

第 1 步的差異：

- **累加順序是定律的一部分。** 先把每個天體的點質量與 J2 合成一個向量再加總，會讓 orbit lab 的撞擊時間 golden 從 1e-6 s 內變成差 2.8e-5 s。撞擊時間是 1e-4 s 解析度的二分法，golden 能對到 1e-6 s，靠的就是和 lab 同樣的捨入。所以積分器用 `add_pull`：先加點質量、再加 J2，和 lab 逐位元相同。門檻沒有放寬。
- **multiscale 保留 lab 的算術。** 它的 golden 是逐位元比對；multiscale lab 用 `hypot` 和 r·r·r，orbit lab 用 r²·√r²，同一條定律的兩種捨入無法同時重現。`gravity_in` 註明它是 `pull` 的點質量情形；新測試 `gravity_is_the_shared_law` 確認兩者差在各天體拉力的 4 個 ulp 內。
- **`PlanetFrame` 的潮汐多了其他天體的 J2。** 主遊戲的 Aurelia（sol，每個天體都有 J2）地表 100 m，四個時刻、四個方向：最多 1.1e-14 m/s²。對照點質量潮汐 1.3e-6 m/s²、地表重力 9.82 m/s²，屬於捨入等級。

第 2 步的檢查（`cargo test -p void-environment`，以及 landing 的 `tests/environment.rs`）：

| 檢查 | 結果 |
| --- | --- |
| 同一點的重力從地表、場景、天體慣性、月球、origin 座標系取，轉到同一組軸 | 地表以下 5.4e-16；月球與 origin 2.0e-12（它們和 Aurelia 只在恆星系質心相遇：點經過 1 AU 座標，間距 3e-5 m，乘上重力梯度 3e-6 /s²，約 1e-11 相對） |
| 等於積分器的場（`gravity_at` 加回座標原點加速度） | 4.4e-16 相對 |
| `PlanetFrame::acceleration` ＝ 環境重力 − 星球中心加速度 ＋ 離心 ＋ Coriolis | 2.4e-16 相對 |
| 發射台上方 150 m：大氣高度、地形高、離地、海深、airspeed、up | 地表與場景座標系 ≤ 1.2e-9 m（6.4e6 m 的一個間距左右）；從 origin 7.6e-6 m；`Air` 與 `Atmosphere::sample` 完全相同 |
| 大氣頂以上、沒有描述的天體 | None |
| panic：天體中心、未知天體、非有限輸入、低於大氣定義域、重複描述、地形不在天體球面上、大氣零點非有限 | 都會 panic |

不在範圍：

- aero `AircraftFlight` 的平地世界。
- 畫面的散射大氣（app `air.rs`、scenery）：它是渲染參數，不是 physics。

## 待決定（需要使用者）

1. **大氣高度零點。** 目前 physics 從參考球起算，layered 發射台的空氣只有預期的 60%（見上表），而畫面的天空從另一個零點畫。
   - 建議：每個天體只設一個 `air_datum_meters`，physics 和畫面散射大氣都讀它：layered 為 5000 m（海平面），hills 為 1800 m（畫面目前用的值）。
   - 影響：主遊戲上升段的阻力與引擎背壓變大，大氣頂上移。`EntryFlight` 與 landing 舊路徑有自己的星球設定，維持 0。
   - 步驟 1–4 都先維持 0，行為不變；第 5 步才依決定修改，並量出發射段的差異。
2. **水先做到哪裡。**
   - 建議：先只做場，即海平面與深度。水的密度與浮力等零件模組需要時再加。
   - 只有 layered 有海。hills 的 1800 m 是大氣零點與色帶，不算海。
3. **`AirSource` 拿星曆。** 這會改 orbit 的公開 trait，好處是積分器內不再線性外插天體中心。
   - 外插誤差約為 ½ × 天體中心加速度 × Δt²：1/60 s 時是 1e-6 m 等級，10 s 時是 0.3 m 等級。
   - 建議：要改，第 3 步量出前後差異。
4. **`PlanetFrame` 的潮汐用完整定律。** 其他天體的 J2 會一起算進來，和積分器一致，這樣船在 rails 與地面之間切換時，看到的是同一個場。
   - 建議：接受，第 1 步量出差異。

## 驗證原則

- 既有 golden 門檻不放寬：propagator、`PlanetFrame`、`EntryFlight`、aero。若捨入順序讓某項超出門檻，先查原因，不調門檻。
- 新增檢查：
  - **座標系無關。** 同一個狀態從 origin、BodyInertial、BodySurface、船的零件座標系取樣，重力與 airspeed 轉到同一組軸後一致，純量完全相同。
  - **對照現有。** 環境的重力減去座標系項，等於 `PlanetFrame::acceleration` 和積分器的 `gravity_at`；`AirSample` 等於 `CraftAir` 和 `RocketAir` 取的 `Air`；`GroundSample` 等於 `Terrain::height`。
  - **panic。** 天體中心、低於大氣定義域、非有限輸入、不屬於樹的天體。
- 行為改變的步驟，各自量出差異並記錄在本頁，不和重構混在同一步。
- 每一步跑 `cargo test --workspace --all-targets` 與 `cargo clippy --workspace --all-targets -- -D warnings`。
