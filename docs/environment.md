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

- `GamePlanet.sea_level` 給 scenery 著色；主遊戲在 `fleet_game.rs` 另外重算一份同樣的值，交給地面 shader 和雲。
- 只有 layered 畫海（`ocean_enabled`）。
- hills 有大氣時用 1800 m，那是地面色帶和雲的基準，不是海。

### 高度零點

大氣模型（`Atmosphere::sample`）的高度從海平面起算，定義域從 −5 km 開始。layered 地形的高度從海床下的參考球起算（lod 要求高度不為負），海平面在參考球之上 5000 m（`SEA_LEVEL`）。

physics 的大氣與畫面的散射大氣（`earth_like_atmosphere(terrain.radius_meters)`：藍天與霧）都從參考球起算，兩者一致；只有雲（海平面上 1.5–8 km）、海面和地面色帶用海平面。所以 layered 的海邊，空氣等於 5 km 高山。主遊戲的 layered 發射台（海平面上 79.7 m）實測如下（Earth 大氣模型，`density_scale` = 1）：

| 大氣零點 | 發射台高度 | 密度 | 氣壓 |
| --- | --- | --- | --- |
| 海平面 | 79.7 m | 1.216 kg/m³ | 100.4 kPa |
| 參考球（目前） | 5,079.7 m | 0.730 kg/m³ | 53.5 kPa |

發射台的空氣只有海平面的 60%：阻力偏小，引擎背壓損失也偏小，大氣頂（模型的 120 km）只在海平面上 115 km。hills 沒有海，它的 1800 m 只是雲和色帶的基準。要不要改會影響飛行行為，所以列在下方「待決定」，由使用者決定。

## 介面

```rust
// void_orbit::gravity：唯一的重力定律
// r = 點 − 天體中心；axis = 自轉軸；兩者在同一組軸。c = oblateness(body) = 1.5 J2 GM R²（點質量為 0）。
pub fn pull(gm: f64, c: f64, axis: DVec3, r: DVec3) -> DVec3;
pub fn add_pull(a: &mut DVec3, gm: f64, c: f64, axis: DVec3, r: DVec3); // a += pull：先點質量、再 J2
pub fn body_pull(body: &CelestialBody, r: DVec3) -> DVec3;               // 星曆（黃道）軸

// void-environment
pub struct BodyEnvironment {
    pub atmosphere: Option<Atmosphere>,
    pub air_datum_meters: f64,          // 大氣高度零點，從地形參考球起算（有海的地形是海平面，否則 0）
    pub terrain: Option<Arc<Terrain>>,  // 必須在天體的球面上（半徑相同）
    pub sea_level_meters: Option<f64>,  // 從地形參考球起算；None 表示沒有海
}
impl BodyEnvironment {
    pub fn airless(terrain: Arc<Terrain>) -> Self;
}

impl Environment {
    pub fn new(ephemeris: &dyn EphemerisSource) -> Self;          // 所有天體的重力
    pub fn with(self, body: usize, place: BodyEnvironment) -> Self;
    pub fn body(&self, body: usize) -> Option<&BodyEnvironment>;
    pub fn frames(&self) -> &SystemFrames;                        // 呼叫者沒有自己的樹時用

    // `frames` 指出樹上每個天體的 BodyInertial／BodySurface（Fleet 的樹就是 SystemFrames 加上
    // Dynamic 節點，見 `Fleet::system_frames`）。`state`／`position` 在 `from` 裡，答案用 `from` 的軸。
    pub fn gravity(&self, at: &Snapshot<S>, frames: &SystemFrames, from: FrameId, position: DVec3) -> DVec3;
    pub fn surroundings(&self, at, frames, from, state: State, body: usize) -> Surroundings;
    pub fn ground(&self, at, frames, from, position: DVec3, body: usize) -> Option<GroundSample>;
    pub fn sample(&self, at, frames, from, state: State, body: usize) -> Sample; // gravity ＋ surroundings
    pub fn surroundings_local(&self, body: usize, local: State) -> Surroundings;  // 狀態已在本體座標
}

pub struct Sample {
    pub gravity: DVec3,               // 所有天體；不含座標系本身的慣性項
    pub surroundings: Surroundings,
}
pub struct Surroundings {
    pub body: usize,
    pub up: DVec3,                    // 查詢的軸，離開天體中心
    pub radius: f64,                  // 到天體中心
    pub air: Option<AirSample>,       // 沒有大氣或在大氣頂以上時為 None
    pub ground: Option<GroundSample>, // 沒有地形時為 None
    pub sea: Option<SeaSample>,       // 沒有海時為 None
}
pub struct AirSample { pub altitude: f64, pub air: Air, pub airspeed: DVec3 } // airspeed：相對空氣，查詢的軸
pub struct GroundSample { pub height: f64, pub clearance: f64 }              // 地形高（參考球起算）、離地高度
pub struct SeaSample { pub depth: f64 }                                      // 海面下為正
```

### 誰在用

| 使用者 | 怎麼用 |
| --- | --- |
| orbit 積分器、`PlanetFrame`、`FreeFallFrame`、orbit-lab 起始速度 | `gravity::add_pull`／`pull`／`body_pull`，再加自己座標系的項 |
| `Fleet` | 建構時收世界的 `Arc<Environment>`；接觸 tile、落地發射、離地高度（`ground`）讀它的地形；把它交給 `PartForces` |
| fleet-flight `FleetAir`（主遊戲） | 引擎背壓與 `CraftAir` 的空氣都用 `surroundings`，從星曆的 origin 座標系、經環境自己的 `frames()` 查 |
| landing `planet_environment` | 從 `LandingPlanet` 建世界的環境：fleet-flight（新飛行、存檔還原）與舊火箭共用 |
| landing `PlanetAir`＋app `RocketAir`（舊火箭） | `PlanetAir` 經 `PlanetFrame`（座標樹）換到本體座標；`RocketAir` 用 `surroundings_local` |
| aero `EntryFlight` | 以選定的大氣建自己的環境，用 `surroundings_local`；沒有空氣時用 `Air::VACUUM` |
| multiscale `gravity_in` | 同一條點質量定律，保留自己 lab 的捨入（逐位元 golden） |

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
- **建構時核對世界描述。** `Fleet::new` 呼叫 `Environment::assert_compatible`，比較天體順序／ID、質量與 GM、半徑、自轉、J2 與參考半徑、父天體、恆星系數量／歸屬與物理原點。錯配直接 panic；存檔還原也經過這個檢查。星曆取樣時間與即時位置由來源管理，不要求相同，也不在每個積分 stage 重複核對。
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
| 5. 待決定項目 | 依使用者決定：大氣零點、海 | 改（若決定要改）。使用者決定：大氣從海平面起算、水先只做場 |
| 6. 文件 | 本頁、aero.md、landing.md、fleet-flight.md、status.md | 無 |

### 進度

| 步驟 | 結果 | 與原計畫的差異 |
| --- | --- | --- |
| 1. 重力定律 | `void_orbit::gravity`：`pull`、`add_pull`（累加形式）、`body_pull`、`oblateness`。積分器 `Field::gravity`、orbit-lab 起始圓軌道速度的徑向重力、`PlanetFrame` 的本體與潮汐都改用它。測試：極點與赤道的解析值、等於 J2 位能的負梯度、隨軸旋轉、多天體加總與 lab 寫法逐位元相同 | 見下 |
| 2. `void-environment` | 大氣模型從 aero 搬來（`git mv`，aero 重新匯出 `Air`、`Atmosphere`、`EarthAtmosphere`、`smooth`、`validate_air`，aero 的 golden 不受影響）。`Environment::new(bodies).with(body, BodyEnvironment)`；查詢分成 `gravity`、`surroundings`、兩者合併的 `sample` | 從星球設定建環境的函式移到第 3 步：environment 不能依賴 landing（landing 之後要用它） |
| 3. Fleet／fleet-flight | `Fleet::new(ephemeris, environment, …)`：世界的 `Arc<Environment>` 由呼叫者建立，`GroundSpec` 不再帶地形，接觸 tile、發射位置、離地高度都讀環境的地形；`clearance_over` 改用 `Environment::ground`。`FleetEnvironment` 改名 `PartForces`（`ForceSample`、`ForcePart`、`set_forces`），`sample` 多拿環境。`FleetAir` 的引擎背壓與 `CraftAir` 的空氣都經 `surroundings` 取；`AirSource::acceleration` 多拿星曆。新飛行與存檔還原用同一個函式建世界的環境（第 4 步移到 landing 的 `planet_environment`）。`MODEL_VERSION` 4 → 5 | 新增只查地形的 `Environment::ground`：離地檢查不能因為大氣模型的 −5 km 定義域而 panic。阻力在星曆軸算（只跟氣流與姿態有關），不再先轉到地表軸。存檔格式不變：仍存地形設定，還原時檢查與環境的地形相同。海平面當時還沒進 `FleetFlight`（第 5 步加入） |
| 4. landing、app、aero | landing 提供 `planet_environment`（fleet-flight 的新飛行與存檔還原、舊火箭共用）。`PlanetAir` 改存 `PlanetFrame`，每個 stage 經座標樹換到本體座標，不再線性外推；app 的 `RocketAir` 經 `Environment::surroundings_local` 取空氣。`EntryFlight` 以選定的大氣建自己的環境，經 `surroundings_local` 取空氣，沒有空氣時用模型本來的真空狀態（新的 `Air::VACUUM`） | 新增 `surroundings_local`：本體座標的狀態不需要樹。修正舊火箭 contact step 的壓力位置（見下） |
| 5. 大氣零點與海 | `TerrainConfig::sea_level_meters`（layered 為 `SEA_LEVEL`，hills 沒有海）；`LandingPlanet::air_datum_meters` 是海平面，沒有海時為 0。`planet_environment` 把兩者交給環境，所以主遊戲、存檔還原、舊火箭都從海平面起算，`Surroundings::sea` 也有值。主遊戲與 `legacy_flight` 的散射大氣底移到參考球＋大氣零點，雲的基準跟著減去零點（雲仍在海平面上 1.5–8 km）。`MODEL_VERSION` 5 → 6 | 海平面由地形提供，不另設欄位：layered 的大陸與海盆本來就以 `SEA_LEVEL` 劃分。scenery example 不改，維持 lab 的參數（layered 的天空仍從參考球起算） |
| 6. 文件 | 本頁的介面與使用者表、aero.md、fleet-flight.md、vessels.md、game.md、landing.md、status.md | 原計畫沒列 vessels.md 與 game.md：`PartForces` 改名與舊火箭的熄火數字在那裡 |

第 1 步的差異：

- **累加順序是定律的一部分。** 先把每個天體的點質量與 J2 合成一個向量再加總，會讓 orbit lab 的撞擊時間 golden 從 1e-6 s 內變成差 2.8e-5 s。撞擊時間是 1e-4 s 解析度的二分法，golden 能對到 1e-6 s，靠的就是和 lab 同樣的捨入。所以積分器用 `add_pull`：先加點質量、再加 J2，和 lab 逐位元相同。門檻沒有放寬。
- **multiscale 保留 lab 的算術。** 它的 golden 是逐位元比對；multiscale lab 用 `hypot` 和 r·r·r，orbit lab 用 r²·√r²，同一條定律的兩種捨入無法同時重現。`gravity_in` 註明它是 `pull` 的點質量情形；新測試 `gravity_is_the_shared_law` 確認兩者差在各天體拉力的 4 個 ulp 內。
- **`PlanetFrame` 的潮汐多了其他天體的 J2。** 主遊戲的 Aurelia（sol，每個天體都有 J2）地表 100 m，四個時刻、四個方向：最多 1.1e-14 m/s²。對照點質量潮汐 1.3e-6 m/s²、地表重力 9.82 m/s²，屬於捨入等級。landing lab 的 `PlanetFrame` golden（點質量潮汐）：Aurelia 的加速度從差 0 變成 8.5e-15 相對，門檻 1e-14 不變；把其他天體的 J2 拿掉就回到差 0，所以差異全是 J2 潮汐，不是捨入。Pebble 仍差 0。

第 2 步的檢查（`cargo test -p void-environment`，以及 landing 的 `tests/environment.rs`）：

| 檢查 | 結果 |
| --- | --- |
| 同一點的重力從地表、場景、天體慣性、月球、origin 座標系取，轉到同一組軸 | 地表以下 5.4e-16；月球與 origin 2.0e-12（它們和 Aurelia 只在恆星系質心相遇：點經過 1 AU 座標，間距 3e-5 m，乘上重力梯度 3e-6 /s²，約 1e-11 相對） |
| 等於積分器的場（`gravity_at` 加回座標原點加速度） | 4.4e-16 相對 |
| `PlanetFrame::acceleration` ＝ 環境重力 − 星球中心加速度 ＋ 離心 ＋ Coriolis | 2.4e-16 相對 |
| 發射台上方 150 m：大氣高度、地形高、離地、海深、airspeed、up | 地表與場景座標系 ≤ 1.2e-9 m（6.4e6 m 的一個間距左右）；從 origin 7.6e-6 m；`Air` 與 `Atmosphere::sample` 完全相同 |
| 大氣頂以上、沒有描述的天體 | None |
| panic：天體中心、未知天體、非有限輸入、低於大氣定義域、重複描述、地形不在天體球面上、大氣零點非有限 | 都會 panic |

第 3 步的差異（同一個 build 裡，用舊的線性外推 `CraftAir` 副本和新的 `FleetAir` 飛同一段；兩者重跑都逐位元相同）：

| 起點與時間 | 位置差 | 速度差 |
| --- | --- | --- |
| 10 km、100 m/s，60 s | 9.4e-9 m | 5.0e-11 m/s |
| 30 km、2 km/s，120 s | 7.4e-7 m | 2.7e-8 m/s |
| 80 km、7.6 km/s，600 s | 9.3e-10 m | 3.8e-12 m/s |

`clearance_over` 的離地高度與改之前逐位元相同（同樣的減法順序）；新檢查 `grounds_and_checkpoints_need_the_environments_terrain`：地面所在天體在環境裡沒有地形、存檔還原到地形不同的環境，都會 panic。

第 4 步的差異：

- **`EntryFlight` golden（門檻不變：相對 1e-9、accepted steps 完全相同）。** 高度從 lab 的 `hypot` 改為環境的 √(x²+y²+z²)，差一個 ulp。最差相對差 3.1e-10 → 4.1e-10；陡峭再入翻轉後（翻轉放大 ulp，改用物理門檻 1 m）與 lab 相距 0.390 m → 0.067 m。
- **舊火箭 contact step 的壓力位置是錯的。** 它把相對浮動原點的 Rapier 位置當成本體座標傳給 `pressure_pa`，算出約 −6400 km 的高度；舊 `RocketAir` 遇到低於 −5 km 就回傳沒有空氣，所以發射台上一直用真空推力。環境沒有這個截斷，直接 panic（`air_costs_the_ascent_speed_and_height`），因此改成傳引擎的本體座標位置（`ContactWorld::position`）。結果：有空氣的熄火 1631 m/s、65.9 km → 1613 m/s、64.8 km（真空不變，2366 m/s、116.4 km）。
- `PlanetAir` 不再外推天體中心，和第 3 步相同性質的差異；舊火箭的檢查都在原門檻內。

第 5 步的差異（layered Aurelia；同一個 build，只把大氣零點改回 0 對照）：

| 檢查 | 大氣零點 0 | 海平面（5000 m） |
| --- | --- | --- |
| 主遊戲火箭（Fleet、`flight_rocket`）發射台推力 | 113.6 kN | 108.0 kN |
| 同上，SAS 垂直全推力 30／60／90 s（海平面上） | 147／246／443 m/s，2.4／8.3／18.0 km | 117／184／269 m/s，2.0／6.5／13.2 km |
| 同上，助推級熄火（135.75 s） | 1623 m/s，海平面上 60.3 km | 1005 m/s，海平面上 36.8 km |
| 舊火箭垂直全推力熄火（`tests/air.rs`，參考球起算） | 1613 m/s，64.8 km | 988 m/s，41.2 km（真空不變，2366 m/s、116.4 km） |
| 舊火箭預設重力轉彎（`trace_one_gravity_turn`） | 遠拱點 101 km，未入軌 | 遠拱點 39 km，未入軌 |
| 舊火箭重力轉彎參數掃描（`tune_the_gravity_turn`，ignored，81 組；參考球起算） | 最好的（垂直 45 s、間隔 14 s、脈衝 30 格、7 次）：近拱點 −1890 km、遠拱點 322 km，未入軌 | 最好的（75 s、14 s、30 格、5 次）：近拱點 −3880 km、遠拱點 412 km，未入軌 |
| 舊火箭錄放檢查的腳本發射（123 s） | 參考球上 29.6 km | 參考球上 19.4 km（海平面上 14.4 km） |

- 空氣變成海平面的量，阻力與背壓都變大，差異會隨「慢 → 在濃空氣裡待更久」累積，所以熄火速度少了約 38%。demo 火箭本來就是照地球海平面空氣估的 Δv（[game.md](game.md)：手動入低軌道約需 9400–9600 m/s），能否手動入軌留給視窗驗收。
- 舊火箭的重力轉彎腳本在兩種零點下都入不了軌：參數掃描在大氣零點 0 時最好的近拱點也在地下 1890 km。掃描是 ignored 的診斷，不是門檻；改成海平面空氣後最好的組合垂直段從 45 s 變成 75 s。
- 錄放檢查（`a_recorded_session_replays_to_the_same_flight`）的前提「飛行要真的飛過」原本寫成參考球上 20 km。它檢查的是重播一致，不是上升性能；改為從海平面量、10 km（遠超過 400 m 的接觸帶，且經過兩次分級）。重播本身的比對門檻沒動。
- `ambient_pressure_drives_the_nozzle`、`there_is_no_drag_above_the_atmosphere` 的高度改從大氣零點量，門檻不變（海平面一大氣壓、11 km 的氣壓、大氣頂）。
- 最深的海盆：20 萬個方向的格點加上最低點附近細掃，最低在參考球上 406.9 m，也就是海平面下 4593 m，在大氣模型 −5 km 的定義域內。地形高度下限是 0（海平面下 5000 m），所以地表以上不會超出定義域。
- 新檢查 `the_air_starts_at_the_sea_where_the_terrain_has_one`（landing）：layered 的環境有海、大氣零點是海平面，海平面上 80 m 的空氣等於模型 80 m 的值、深度 −80 m；hills 沒有海、零點 0；沒有空氣的環境仍有海。主遊戲的 renderer-free 檢查另核對散射大氣底與雲的基準。

不在範圍：

- aero `AircraftFlight` 的平地世界。
- 畫面的散射大氣（app `air.rs`、scenery）：它是渲染參數，不是 physics；只有它的底跟著大氣零點移（第 5 步）。
- **水的物理：之後做，不是不做（使用者決定）。** 浮力、水阻力、濺落、水中的引擎、水下畫面都還沒有；現在落進海裡，船仍會停在海床上，引擎和阻力照空氣算。環境已經提供要用的參考：`BodyEnvironment::sea_level_meters` 與 `Surroundings::sea`（`SeaSample::depth`，海面下為正），做水的零件模組時直接讀它，照新功能的流程先在 lab 做。

## 待決定（已決定）

1. **大氣高度零點。**（已決定：照建議，第 5 步）改之前 physics 和畫面的散射大氣都從參考球起算，layered 發射台的空氣只有海平面的 60%（見上表）。
   - 建議：layered 的 `air_datum_meters` 設為海平面 5000 m，畫面散射大氣的底也移到同一個零點，兩者繼續一致。hills 沒有海，維持 0。
   - 影響：主遊戲上升段的阻力與引擎背壓變大，大氣頂上移 5 km，發射台的天空變成海平面的樣子。最深的海盆（海平面下約 4.5 km）仍在大氣模型 −5 km 的定義域內，第 5 步會確認地形最低點。`EntryFlight` 與 landing 舊路徑有自己的星球設定，維持 0。
   - 步驟 1–4 都先維持 0，行為不變；第 5 步才依決定修改，並量出發射段的差異。
2. **水先做到哪裡。**（已決定：先只做場，第 5 步；水的物理之後做，見「不在範圍」）改之前 physics 沒有水：落進 layered 的海，船穿過畫出來的海面，停在海床上（最深約 4.5 km），引擎和阻力照空氣算。`FleetFlight` 的環境目前也沒有海平面（`sea_level_meters` 為 None）。
   - 建議：先只做場，即海平面與深度：主遊戲 layered 傳入 5000 m，行為不變，之後的 HUD、濺落、浮力讀它。水的密度、浮力、水阻力等零件模組需要時再加，照新功能的流程先在 lab 做。
   - 只有 layered 有海。hills 的 1800 m 是雲和色帶的基準，不算海。
3. **`AirSource` 拿星曆。**（已採用，第 3 步） 這會改 orbit 的公開 trait，好處是積分器內不再線性外插天體中心。
   - 外插誤差約為 ½ × 天體中心加速度 × Δt²：1/60 s 時是 1e-6 m 等級，10 s 時是 0.3 m 等級。
   - 建議：要改，第 3 步量出前後差異。
4. **`PlanetFrame` 的潮汐用完整定律。**（已採用，第 1 步） 其他天體的 J2 會一起算進來，和積分器一致，這樣船在 rails 與地面之間切換時，看到的是同一個場。
   - 建議：接受，第 1 步量出差異。

## 驗證原則

- 既有 golden 門檻不放寬：propagator、`PlanetFrame`、`EntryFlight`、aero。若捨入順序讓某項超出門檻，先查原因，不調門檻。
- 新增檢查：
  - **座標系無關。** 同一個狀態從 origin、BodyInertial、BodySurface、船的零件座標系取樣，重力與 airspeed 轉到同一組軸後一致，純量完全相同。
  - **對照現有。** 環境的重力減去座標系項，等於 `PlanetFrame::acceleration` 和積分器的 `gravity_at`；`AirSample` 等於 `CraftAir` 和 `RocketAir` 取的 `Air`；`GroundSample` 等於 `Terrain::height`。
  - **panic。** 天體中心、低於大氣定義域、非有限輸入、不屬於樹的天體。
- 行為改變的步驟，各自量出差異並記錄在本頁，不和重構混在同一步。
- 每一步跑 `cargo test --workspace --all-targets` 與 `cargo clippy --workspace --all-targets -- -D warnings`。

## 合併審查與驗收

本輪使用者已完成視窗驗收並同意合併。整合保留 master 的相機 NaN、multiscale split 精度與舊模型拒絕修正。另補上 Fleet 建構／存檔還原的環境與星曆核對，防止不同世界的天體參數與座標系混用。相機改為固定追蹤 craft root（主遊戲的上面級指令艙），分離前後不再切換質心焦點；模型版本升到 7。

整合後 42 項針對性測試通過；相機修改後 19 項相機／存檔／錄放測試及相關 Clippy 通過。fmt、主遊戲與 multiscale example 的編譯檢查通過。本輪沒有重跑全量測試。
