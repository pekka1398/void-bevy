# 零件圖（branch `claude/part-graph`）

目標：船只有一種表示法，就是零件圖。零件（定義加上自己的狀態：燃料、分級、點火、在船上的位置）和零件之間的連接是主要紀錄；一艘船是零件圖的一個連通分量，再加上一個物理擁有者（軌道積分或接觸場景）。分離、對接只是零件圖的操作。零件是座標樹上的節點（在船的零件座標系之下），接點、對接口、繪圖都經同一棵樹。

本 branch 先做**資料模型**（範圍 A）：物理不變，主遊戲的飛行逐位元相同。零件模組（引擎、阻力等「在這個環境下對這個零件施多少力、耗什麼」）與每個零件一個剛體（範圍 B）列在後面，另行決定。

branch 從 `claude/environment-interface`（第 5 步之後）開出：零件要成為座標樹的節點（座標樹 branch），之後的零件模組要讀環境介面，兩者都還沒合入 master。

## 改之前

### Fleet 的零件狀態分散在五個地方

| 狀態 | 存在哪 |
| --- | --- |
| 定義、燃料、分級號 | `Fleet::parts: HashMap<String, PropulsionPart>` |
| 連接 | `Fleet::connections: Vec<Connection>`（全世界一份） |
| 在船上的位置 | 各船的 `Vessel::poses`（船的零件座標系；發射、分離、對接、進出場景、軌道上燒完一段時重新以質心為原點） |
| 已分級、點火 | `Fleet::staged`、`Fleet::lit: HashSet<String>` |
| 噴嘴出口面積 | fleet-flight `FleetAir::nozzle_areas`，用引擎定義 ID 查表 |

分離（`decouple`）與對接（`join`）各自把這五份資料拆開、重組，再決定哪邊保留船 ID。

### 船的五種表示法

| 位置 | 用途 | 狀態 |
| --- | --- | --- |
| assembly `Craft`／`CompiledCraft` | 設計時的船（編輯器、存檔、發射的輸入） | 保留：它是設計，不是飛行中的船 |
| assembly `AssemblyFlight` | assembly lab 的平地飛行，自己存 fuel、lit、cuts | lab，有 golden |
| vessels `Fleet` | 主遊戲、vessels lab、multiscale lab、接縫檢查 | 本 branch 改它 |
| landing `PartJointRocket`／`DemoRocket` | 舊主遊戲（`legacy_flight`）、sas 與 landing example | 舊路徑 |
| aero `Vehicle` | aero lab 寫死的飛機與返回艙 | lab，有 golden |

### 零件 ID

`{發射時的船}/{craft 裡的零件}`，例如 `v1/p2`。分離出去的零件仍叫 `v1/p5`，即使它現在屬於 `v2`。ID 從不改名，所以本身是穩定的；只是前綴看起來像「所屬的船」。

### 零件不是座標系

- `PartSnapshot` 給船的零件座標系，加上零件在其中的 pose。
- `node_frame` 自己把 pose 和接點位置乘起來，換到 origin 座標系。
- vessels lab 的對接捕獲自己算兩個接點在 origin 座標系的距離。

## 介面

```rust
// void-assembly::graph：飛行中的零件圖
pub struct Part {
    pub id: String,
    pub definition: &'static PartDefinition,
    pub fuel_kg: f64,          // 油箱存量；沒有油箱為 0
    pub stage: Option<u32>,
    pub staged: bool,          // 已經分級（引擎點火、分離器斷開）
    pub lit: bool,             // 引擎點著；只有引擎能為 true
    pub pose: PartPose,        // 在所屬船的零件座標系裡
}

pub struct PartGraph { /* 零件（依 ID 排序）與連接 */ }
impl PartGraph {
    pub fn add(&mut self, craft: &CompiledCraft, prefix: &str) -> Vec<String>; // 發射：加入一艘船的零件與連接
    pub fn part(&self, id: &str) -> &Part;
    pub fn set_fuel(&mut self, id: &str, fuel_kg: f64);
    pub fn set_pose(&mut self, id: &str, pose: PartPose);
    pub fn stage_part(&mut self, id: &str);
    pub fn connections(&self) -> &[Connection];
    pub fn check_connection(&self, connection: &Connection);           // 接點必須存在、空著、大小相同
    pub fn connect(&mut self, connection: Connection);                 // 對接
    pub fn disconnect(&mut self, part: &str, node: &str) -> Connection; // 分離
    pub fn components(&self, ids: &[String]) -> Vec<Vec<String>>;      // 保留 ids 的順序
    pub fn free_nodes(&self, ids: &[String]) -> Vec<(String, &'static AttachNode)>;
    pub fn crossfeed_tanks(&self, members: &[String], engine: &str) -> Vec<String>; // 依 members 的順序
    pub fn mass(&self, ids: &[String]) -> f64;                         // 依 ids 的順序加總
}

// void-vessels：船是零件圖的一個連通分量加上物理擁有者
struct Vessel { id, name, root, members: Vec<String>, owner: Owner } // 不再存 poses

impl Fleet {
    pub fn parts(&self) -> &PartGraph;
    pub fn part_frame(&self, part: &str) -> FrameId;  // 在船的零件座標系之下，運動是零件的 pose
    pub fn node_frame(&self, part: &str, node: &str) -> (DVec3, DVec3); // 經樹，不再自己乘
    pub fn node_in(&self, part: &str, node: &str, frame: FrameId) -> (DVec3, DVec3); // 任一座標系
    pub fn node_gap(&self, a: &str, node_a: &str, b: &str, node_b: &str) -> f64; // 在 b 的座標系裡量
}
```

### 怎麼做

- **零件圖是唯一的紀錄。** 燃料、分級、點火、pose 都在 `Part` 上；`Fleet` 不再有 `lit`、`staged`、`PropulsionPart` 與各船的 `poses`。船只記成員的順序（質量、慣量、推力照這個順序加總，捨入與現在相同）和物理擁有者。
- **分離與對接是零件圖的操作。** `decouple` 先 `disconnect`，再依連通分量分船；`join` 先 `connect`，再合船。分船、合船時重新以質心為原點，只是改成員的 pose。
- **零件是座標樹的動態節點。** 每個零件一個 `Dynamic::Part` 節點，掛在所屬船的零件座標系下，運動直接讀零件的 pose（和船的節點讀擁有者一樣），所以不必同步。分離、對接時把節點換到新船之下。
- **零件資料補齊。** 噴嘴出口面積移到 catalog 的引擎模組（`nozzleExitAreaM2`，每個引擎都必須寫）；`FleetAir` 不再有 ID 對照表。數值不變。

### 不變的

- `Craft`、`compile`、編輯器、craft JSON。
- 物理：一艘船在接觸場景裡仍是一個 Rapier 剛體；軌道積分、rails、交會氣泡都不變。
- 零件 ID 的格式（見「待決定」）。
- labs 與舊路徑：`AssemblyFlight`、`PartJointRocket`、aero `Vehicle` 照 AGENTS.md 維持獨立。

## 步驟

每步全部測試與 clippy 通過才提交並 push 到 `claude/part-graph`。

| 步驟 | 內容 | 行為是否改變 |
| --- | --- | --- |
| 1. `PartGraph` | assembly 新增 `graph.rs`：`Part`、`PartGraph` 與操作；`components`、`free_nodes`、`crossfeed_tanks` 與現有函式結果相同 | 不改（新程式） |
| 2. Fleet 存零件圖 | `Fleet` 改存 `PartGraph`：燃料、分級、點火、pose 都進零件；`lit`、`staged`、`PropulsionPart`、`Vessel::poses` 退場（船改存 `members`）；`decouple`、`join` 改用 `disconnect`／`connect` | 不改。存檔格式改變（`FleetCheckpoint` 版本、`MODEL_VERSION`） |
| 3. 零件座標系 | `Dynamic::Part`、`part_frame`；`node_frame`、`free_nodes` 的位置、`PartSnapshot` 經樹；vessels lab 的對接捕獲用兩個接點的樹轉換 | 只有捨入（經 LCA 的轉換與手乘順序不同），量出並記錄 |
| 4. 噴嘴面積進 catalog | 引擎模組加 `nozzleExitAreaM2`，`FleetAir` 讀它 | 不改；catalog 改變，舊存檔照例拒絕 |
| 5. 文件 | 本頁、vessels.md、assembly.md、fleet-flight.md、status.md | 無 |

### 驗證

- 既有測試（vessels 的 38 項 lab 檢查、Fleet、fleet-flight、seam-check、multiscale lab）門檻不變。
- **前後逐位元對照。** 一個暫時的 probe 用同一個劇本（地面發射、點火、分級、分離、軌道燃燒、交會氣泡、對接、rails）在改之前與改之後各跑一次，每步印出所有船與零件的狀態，兩邊必須完全相同。第 3 步只允許零件座標系那幾項有捨入差，量出來記在本頁。
- 新測試：`PartGraph` 的操作（分離後的連通分量、對接佔用與大小檢查、交叉供油）、零件座標系與接點位置、存檔還原後零件狀態相同。

### 進度

| 步驟 | 結果 | 與原計畫的差異 |
| --- | --- | --- |
| 1. `PartGraph` | `void_assembly::graph`：`Part`（定義、燃料、分級、已分級、點火、pose，加上 `engine()`／`decoupler()`／`is_command()`）與 `PartGraph`（`add`、`insert`、`restore_connections`、`connect`、`disconnect`、`connection_at`、`components`、`free_nodes`、`crossfeed_tanks`、`mass`）。檢查：demo 與主遊戲火箭的零件、pose、質量、連通分量、空接點、交叉供油都和 `CompiledCraft` 相同；分離後各引擎只連到自己那半的油箱，對接後合成一個分量；未知零件、重複零件、佔用或不存在的接點、自己接自己、沒有引擎卻點火都會 panic | `crossfeed_tanks` 與 `mass` 依呼叫者給的成員順序：Fleet 依船上零件的順序加總，捨入才和現在相同 |
| 2. Fleet 存零件圖 | `Fleet::parts` 是 `PartGraph`，燃料、分級、點火、pose 都在零件上；`Vessel` 只剩名字、根零件、成員順序與物理擁有者。`lit`、`staged`、`PropulsionPart`、`Vessel::poses` 與 Fleet 自己的 `connections` 退場。`decouple` 先 `disconnect`，再依 `components` 分船；`join` 合船後 `connect`。分船、合船、軌道上燒完一段時重新以質心為原點，改的是成員的 pose。`propulsion`、`burn` 直接讀寫零件圖。`FleetCheckpoint` 版本 2 → 3：零件存分級、點火、pose，不再有 lit／staged 清單，連接經 `restore_connections` 檢查後依原順序還原。`MODEL_VERSION` 6 → 7。probe 的劇本：主遊戲上升、分級、rails；交會；旋轉分離；對接；存檔還原。它印出 927 行逐位元的狀態，改前改後 md5 相同。新測試：分級、分離後，每艘船是零件圖的一個連通分量；存檔還原後，每個零件的燃料（逐位元）、分級、點火、pose，以及連接的順序都相同。workspace 測試 313 passed、0 failed、4 ignored，clippy 無警告 | 原計畫的第 2、3 步合併：狀態進零件和 pose 進零件改的是同一批函式（分離、對接、重新置中），分開做要先寫一份過渡的同步。對接的接點檢查改由 `PartGraph::check_connection` 在 settle 之前做，panic 訊息改成零件圖的。存檔還原多檢查「點火的零件必須已分級」 |
| 3. 零件座標系 | 每個零件有一個 `Dynamic::Part` 節點（`Fleet::part_frame`），掛在船的零件座標系下，運動就是零件的 pose，所以不必同步。`put` 把成員的節點掛到船下；對接時先把 b 的零件節點移到 a 之下，再拿掉 b 的節點；存檔還原時重建。`node_frame` 改成經樹計算；新的 `node_in(part, node, frame)` 給出接點在任一座標系的位置。`PartSnapshot` 的慣性位置與姿態也經樹。新的 `node_gap` 是兩個接點在第二個零件座標系裡的距離；vessels lab、multiscale lab 的對接捕獲，以及 seam-check 與測試的接近迴圈都改用它。probe：物理逐位元相同（船的狀態、推力、燃料、連接、分級）。零件與接點的慣性位置最多差 3.05e-5 m：座標約 1.44e11 m，相對差 2.1e-16，也就是一個 ulp。姿態四元數的分量最多差 2.2e-16。Join 場景在捕獲前（間隙 0.0795 m），經樹量的間隙和慣性座標相減的結果差 1.9e-5 m，這就是慣性座標的捨入。新測試：子樹移動（frames）；零件節點在自己的船之下、位置就是 pose；接點在自己座標系裡的位置；對接、分離、還原之後節點的父節點。workspace 測試 315 passed、0 failed、4 ignored，clippy 無警告 | void-frames 的 `reparent` 原本不能移動有子節點的節點，現在連同子樹一起移，子樹的深度跟著更新：船的零件座標系每次換擁有者都要移動（原計畫沒有列）。`free_nodes` 沒有位置欄位，不用改。新增 `node_in`、`node_gap`。主遊戲與 lab 的繪圖仍用 `frame`＋`local_position` |
| 4. 噴嘴面積進 catalog | catalog 的引擎模組加上 `nozzleExitAreaM2`（必填，`Module::Engine::nozzle_exit_area_m2`，`EngineRating` 也帶著它）：engine-large、flight-booster-engine 0.12 m²，engine-small、flight-upper-engine 0.15 m²，和原本的表相同。`FleetAir` 不再有 `nozzle_areas`，直接讀零件定義。probe 輸出和第 3 步完全相同。新測試：catalog 的四個引擎都有出口面積，而且在一大氣壓下仍有推力。workspace 測試 316 passed、0 failed、4 ignored，clippy 無警告 | catalog 改變，所以舊錄影照例以「catalog changed」拒絕；模擬規則沒變，`MODEL_VERSION` 不動。`AssemblyFlight`（assembly lab）不讀出口面積 |
| 5. 文件 | 本頁；vessels.md（所有權、API、測試）；assembly.md（`graph.rs`、`nozzleExitAreaM2`）；fleet-flight.md（噴嘴氣壓、`MODEL_VERSION` 7）；frames.md（換父節點時子樹一起移）；status.md（branch 狀態）。暫時的 probe 在提交前刪除，沒有進 repository | 多改了 frames.md（第 3 步動到 void-frames） |

## 之後（不在本 branch，另行決定）

1. **零件模組。** 每個模組回答「在這個環境下，它對這個零件施多少力和力矩、消耗什麼」，船把它們加總。先把現有的引擎（推力、背壓、耗油）、油箱、分離器與 fleet-flight 的阻力搬成模組，取代 `PartForces`／`ForceSample`；RCS、對接口、降落傘、防熱盾、浮筒之後都是新模組。
2. **舊表示法退場。** `PartJointRocket` 與 `legacy_flight`、aero `Vehicle`、`AssemblyFlight` 改用零件圖，或保留為對照用的 lab。它們有各自的 golden，要逐一決定。
3. **範圍 B：每個零件一個剛體。** 零件之間用關節連接，可以彎曲、斷裂、毀損。這會改變飛行行為，照 AGENTS.md 先在 lab 做、視窗驗收。

## 待決定（已決定）

1. **零件 ID 不改格式（使用者決定）。** `v1/p5` 從不改名；前綴是發射時的船，不是所屬的船（vessels.md 的 API 表寫明）。
2. **範圍 A 之後先做零件模組（使用者決定）**，即「之後」第 1 項，在新的 branch 上做。

## 審查修正

- `FleetCheckpoint` 還原核對每艘船的成員恰為一個連通分量，拒絕連接遺失而物理擁有者仍保持整船的錯配；回歸涵蓋移除單一連接與全部連接，以及正常還原。
- `Fleet::put` 在船的父節點未變時跳過 `reparent`。一般軌道更新不再掃描整棵座標樹，實際擁有者交接仍會移動船與零件子樹。`FrameTree::reparent` 的 free frame 失效語意不變。

- PartGraph 集中核對燃料（有限、非負、不超過油箱容量）、pose（有限位置與單位四元數）、點火（已分級且有引擎）。移除公開的 `part_mut`，改用 `set_fuel`、`set_pose`、`stage_part`，ID／定義／分級號在加入後不能改。插入重複 ID 或設定非法狀態時先拒絕，不會覆寫既有零件。
- 整合版保留 master 的環境／星曆核對與固定上面級相機，`root_position_local` 直接讀 PartGraph。模型版本為 8，避免與 master 已發布的 model 7 混用；舊版本拒絕測試保留。
- 新增持續回歸：30 次分離／合船，中間反覆 checkpoint 還原，檢查所有權、連通分量、零件父節點與 checkpoint 完整相等。

補強後整合檢查：56 項針對性測試通過（PartGraph、Fleet、存檔、錄放、相機、多尺度），assembly／vessels／fleet-flight 的 all-targets Clippy 通過；主遊戲、vessels lab、multiscale lab、seam-check 的 all-targets 編譯檢查與 fmt 通過。沒有重跑全量測試。使用者完成視窗驗收並同意合併。

F6 存檔修正：JSON 寫入改用 64 KiB BufWriter，保留 flush、同步落盤與原子替換。同一份約 15 MB 存檔的寫入實測由 12.626 s 降到 54.503 ms（不含狀態擷取）。存檔與 durable 相關 13 項測試及針對性 Clippy 通過；回歸涵蓋跨緩衝容量、覆寫、完整讀回與還原後繼續模擬。
