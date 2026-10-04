# 零件模組（branch `claude/part-modules`）

目標：零件的行為集中在模組裡。每個模組回答「在這個環境下，這個零件施多少力、消耗什麼」，船把各零件的回答加總，再交給它的物理擁有者（軌道積分或接觸場景）。新的零件行為（降落傘、浮筒、防熱盾）只要新增一個模組，不必動 Fleet 的積分或 fleet-flight。

本 branch 先把**現有的行為**搬成模組，飛行逐位元相同；新的模組另行決定，照 AGENTS.md 先在 lab 做、視窗驗收。

branch 從 `claude/part-graph`（零件圖完成後）開出：模組讀零件圖上的零件，空氣來自環境介面，兩者都還沒合入 master。

## 改之前

### 零件行為分散在三個 crate

| 行為 | 資料 | 計算在哪 |
| --- | --- | --- |
| 引擎推力、耗油 | catalog 引擎模組（真空推力、Isp、方向） | vessels `propulsion`：點火的引擎依交叉供油分組，推力 × 油門，流量 = 推力／(Isp·g0) |
| 引擎背壓 | catalog 引擎模組的 `nozzleExitAreaM2` | fleet-flight `FleetAir::sample`：每個引擎一個縮放 max(0, 1 − 面積 × 氣壓／真空推力)，放進 `ForceSample::thrust_scales`；Fleet 再把引擎力乘上去、重算總力與力矩 |
| 阻力 | 零件形狀（半徑、高度、錐／柱） | fleet-flight `FleetAir::sample` 為每個零件做一個 `AeroElement`（露出的頂、底面積看連接決定），`CraftAir` 是軌道積分每個 stage 與接觸每步呼叫的 `AirSource` |
| 油箱 | catalog 油箱模組（容量） | vessels `burn`：同一供油群依存量比例扣 |
| 分離器 | catalog 分離器模組（接點、衝量） | vessels `Fleet::decouple` |
| 轉向 | catalog 指令模組（有沒有） | vessels `steering`：有指令零件的船，轉向力矩 = 輸入 × `FleetOptions::steering_torque` |

### 空氣是外掛的

- vessels 不知道空氣：`Fleet::set_forces(Option<Arc<dyn PartForces>>)` 由 fleet-flight 裝上 `FleetAir`；沒裝就沒有背壓也沒有阻力。
- `FleetAir` 只看建構時指定的那一個天體（主遊戲的母星）。
- 有沒有裝 `PartForces` 還決定軌道段要不要切成 `flight_chunk_seconds` 的小段（形狀、姿態、背壓每段凍結一次）。
- 存檔還原時呼叫者要再傳一次 `forces`。

目前所有 Fleet 的使用者裡，「環境有大氣」與「裝了 `FleetAir`」完全一致：fleet-flight 開空氣時兩者都有、關空氣時都沒有；vessels lab、multiscale、seam-check 與各測試的環境都沒有大氣，也沒裝 forces。

## 介面

```rust
// void-modules（新 crate，不依賴 Bevy、不依賴 Fleet）：零件的模組在它的環境裡做什麼

/// 一艘船這一段（軌道積分的一段或接觸的一步）所處的環境，在質心讀一次。
pub struct Conditions { pub air: Option<AirSample> }
impl Conditions {
    pub const VACUUM: Conditions;
    /// 有大氣的天體依序，第一個大氣裡包含 `state` 的。
    pub fn at(environment: &Environment, ephemeris: &dyn EphemerisSource, t: f64, state: State) -> Self;
    pub fn ambient_pressure_pa(&self) -> f64;
}

pub mod engine {
    pub struct Thrust { pub force: DVec3, pub point: DVec3, pub flow_kg_per_second: f64 }
    /// 真空推力 × 油門，沿零件的推力方向，再扣掉噴嘴背壓；流量維持真空額定。
    pub fn thrust(part: &Part, throttle: f64, conditions: &Conditions) -> Thrust;
}

pub mod body {
    /// 零件本體在空氣裡：露出的頂、底（接點沒被 `members` 裡的鄰居蓋住的部分）、側面與形狀的阻力係數。
    pub fn element(graph: &PartGraph, members: &[String], part: &str, centre: DVec3) -> AeroElement;
}

/// 一艘船這一段的空氣：零件本體與姿態凍結，每個積分 stage 重新讀空氣。
pub struct VesselAir { /* 環境、有大氣的天體、姿態、各零件的 AeroElement */ }
impl AirSource for VesselAir { … }
/// 沒有任何天體有大氣時為 None。
pub fn vessel_air(environment: &Arc<Environment>, graph: &PartGraph, members: &[String], centre: DVec3, rotation: DQuat) -> Option<VesselAir>;
```

```rust
// void-vessels
pub fn propulsion(graph: &PartGraph, members: &[String], throttle: f64, centre: DVec3, conditions: &Conditions) -> Propulsion;
// 移除：PartForces、ForceSample、ForcePart、Fleet::set_forces、Fleet::from_checkpoint 的 forces 參數
```

### 怎麼做

- **模組是程式，不是外掛。** Fleet 直接呼叫 `void-modules`；空氣有沒有、在哪個天體，全由世界的 `Environment` 決定。沒有大氣的環境就沒有背壓與阻力，和現在沒裝 forces 相同。
- **兩種時間尺度。** 推力類（引擎，之後的 RCS）每段凍結一次：環境在段首、在質心讀，供油與熄火時刻照現在由船依交叉供油群處理。流體類（阻力，之後的降落傘、浮力）形狀與姿態每段凍結，空氣在每個積分 stage 重新讀。
- **切段的條件。** 軌道段原本「裝了 forces 就切成小段」，改成「環境裡有天體有大氣就切」。依上一節的對照，所有現有的世界結果不變。
- **運算順序照舊。** 引擎力仍是 `(姿態 × 方向 × (推力 × 油門)) × 背壓縮放`，總力與力矩照成員順序加總；阻力元素的位置是 pose 減質心。所以結果逐位元相同。

### 不變的

- catalog、craft JSON、存檔格式、`MODEL_VERSION`（模擬規則不變）。
- 物理：推力、背壓、阻力的公式與取樣位置（背壓仍在船的質心取氣壓）；轉向力矩仍是 `FleetOptions::steering_torque`（見「待決定」）。
- labs 與舊路徑：`AssemblyFlight`、`PartJointRocket`、aero `Vehicle` 照 AGENTS.md 維持獨立。

## 步驟

每步全部測試與 clippy 通過才提交並 push 到 `claude/part-modules`。

| 步驟 | 內容 | 行為是否改變 |
| --- | --- | --- |
| 1. `void-modules` | 新 crate：`Conditions`、`engine::thrust`、`body::element`、`VesselAir`（從 fleet-flight 搬來）。headless 檢查：背壓縮放、過度膨脹停在零、真空不變、露出面積、大氣頂以上沒有阻力 | 不改（新程式，還沒有人用） |
| 2. Fleet 用模組 | `propulsion` 收 `Conditions`；Fleet 自己建 `Conditions` 與 `VesselAir`；`PartForces`、`ForceSample`、`ForcePart`、`set_forces`、`FleetAir` 退場；`from_checkpoint` 不再收 forces；切段改看環境有沒有大氣 | 不改。前後 probe 逐位元對照 |
| 3. 文件 | 本頁、vessels.md、fleet-flight.md、aero.md、status.md | 無 |

### 驗證

- 既有測試門檻不變（vessels 的 38 項 lab 檢查、Fleet、fleet-flight 的空氣與錄放、seam-check、multiscale）。
- **前後逐位元對照。** 沿用零件圖 branch 的暫時 probe：主遊戲有空氣的上升（推力、背壓、阻力、分級、rails）、交會、旋轉分離、對接、存檔還原。改前改後的輸出必須完全相同。
- 新測試：模組的 headless 檢查（第 1 步），以及有大氣與沒有大氣的世界裡 Fleet 的力和現在相同。

### 進度

| 步驟 | 結果 | 與原計畫的差異 |
| --- | --- | --- |
| 1. `void-modules` | 新 crate `crates/modules`：`Conditions`（有大氣的天體依序，第一個大氣裡包含該狀態的；沒有時是真空）、`engine::thrust`、`body::element`、`VesselAir`／`vessel_air`／`has_atmosphere`。檢查：真空推力就是額定、半油門推力與流量減半；海平面時 booster 保有 89.9%、上級的真空噴嘴只剩 24%，流量都維持真空額定；十大氣壓時推力停在零。零件單獨時兩端全開；在船裡，頂、底只露出比鄰居大的部分；鄰居不在成員裡（分離出去的）就不遮。Aurelia 海平面上 1 km、上升 300 m/s 時，阻力對氣流做負功；200 km 以上沒有空氣也沒有阻力；沒有大氣的世界 `vessel_air` 是 None。workspace 測試 319 passed、0 failed、4 ignored，clippy 無警告 | 阻力不是正好逆著氣流：機體有攻角，側面和端面的係數不同。檢查改成「做負功」 |

## 之後（不在本 branch，另行決定）

1. **第一個新模組。** 建議降落傘（願望清單第 6 項）：它是流體類模組，有自己的狀態（收起／展開），正好驗證這套介面；照流程先在 lab 做、視窗驗收。RCS 由對接那條線在做，本 branch 不碰。
2. **浮筒與水。** 環境已有海平面與深度；浮力、水阻力是流體類模組，在 `VesselAir` 旁邊加一個水的來源。
3. **背壓在引擎的位置取氣壓。** 現在在船的質心取，長的船上下差幾十公尺。改了推力會有極小的變化，要量出並記錄。

## 待決定（需要使用者）

1. **轉向力矩要不要變成指令艙的額定。** 現在是 Fleet 的一個選項，每艘有指令零件的船都一樣大。改成 catalog 指令模組的「反應輪力矩」並照零件加總，對接後有兩個指令艙的船轉向力矩會加倍（vessels lab 的對接場景、SAS 檢查會變）。
   - 建議：本 branch 不改，等第一個新模組時一起決定。
