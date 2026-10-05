# 任務規格：2026-10-05 整理前

以下完整保留該輪任務文件，含舊流程、branch／commit 限制與原驗收安排。這些是已發出的任務上下文，不作目前的全局指令。後續工作遵守 [AGENTS.md](../../AGENTS.md)；現況見 [status.md](../status.md)。

---

## 原檔：docs/specs/rcs-docking.md

# RCS 與對接本輪實作

共同基線：master 41da390（MODEL_VERSION 13）。使用者已授權本輪 branch/worktree 與三個 agent。先讀 AGENTS.md、NOTE.md、docs/status.md，歷史完成狀態以程式為準。
只改自己的 worktree。不 commit/push/merge；交付可審查的 diff、設計理由、驗證結果和視窗驗收操作。不得加入 fallback 或放寬既有門檻。不跑全 workspace test/lint；只跑受影響 crates，編譯限制 -j 2，避免多份 Bevy 同時大量連結。GUI 使用 TigerVNC，禁用 xdotool；不要用 pgrep/pkill 字串比對，僅 numeric PID。GUI 與昂貴驗證開始前告知 root 以協調資源。
保持主遊戲預設行為；新增能力在獨立 core + lab 驗證。必要格式變更明確拒絕舊版，不冒稱相容。不要改用第二套零件、資源或座標模型。
先向 root 報告實作設計、涉及的共享接口和風險，再繼續實作；遇到跨任務接口修改先協調。不要因任務大就停在計畫，完成具體可驗收的一輪。

建立可配置、按穩定 module ID 尋址的 RCS 噴嘴/對接埠，消耗現有 Monopropellant 資源。噴嘴提供局部方向、施力點、推力/Isp，混控要求平移與旋轉；有限且可解釋的分配算法，不憑空產生控制力，不以理想 steering 代替 RCS。用 PartGraph 共用供油語義，處理無燃料/飽和/非對稱/多資源。SAS 與 RCS 職責清楚。
對接是實際捕獲流程：距離、埠方向、相對速度、旋轉條件，合法可用埠、防自接/重接；捕獲合併 graph/owner 時保持零件身份、世界 pose、質量/動量。解除對接與可配置小分離衝量，存檔/錄放完整保存埠和控制狀態。既有 debug join 不能冒充對接。
獨立 lab 提供近距離兩船、平移/轉向/RCS切換、對接/解除、存讀/錄放與實際噴嘴狀態 HUD。headless 驗證分配、供油、捕獲邊界、旋轉與相對速度、多船 owner/pose/動量及續跑。不要要求使用者每次從發射台重玩。
擁有 RCS/docking 模組/catalog/純核心/捕獲 graph 與 lab 控制。氣動 agent 擁有共用 wrench 與 Fleet 積分；先用既有 EngineForce/Propulsion 類似語義（f64 force、point、COM torque），共享 Fleet 修改協調，不自行寫第二個物理迴圈。檢查 git 是否有未合入舊 RCS 成果，若有評估重用但不得盲目 cherry-pick。


---

## 原檔：docs/specs/aero-wrenches.md

# 完整氣動施力與力矩本輪實作

共同基線：master 41da390（MODEL_VERSION 13）。使用者已授權本輪 branch/worktree 與三個 agent。先讀 AGENTS.md、NOTE.md、docs/status.md，歷史完成狀態以程式為準。
只改自己的 worktree。不 commit/push/merge；交付可審查的 diff、設計理由、驗證結果和視窗驗收操作。不得加入 fallback 或放寬既有門檻。不跑全 workspace test/lint；只跑受影響 crates，編譯限制 -j 2，避免多份 Bevy 同時大量連結。GUI 使用 TigerVNC，禁用 xdotool；不要用 pgrep/pkill 字串比對，僅 numeric PID。GUI 與昂貴驗證開始前告知 root 以協調資源。
保持主遊戲預設行為；新增能力在獨立 core + lab 驗證。必要格式變更明確拒絕舊版，不冒稱相容。不要改用第二套零件、資源或座標模型。
先向 root 報告實作設計、涉及的共享接口和風險，再繼續實作；遇到跨任務接口修改先協調。不要因任務大就停在計畫，完成具體可驗收的一輪。

把現有 aero 核心的力/力矩接入 PartGraph/Fleet，取代 production force-only 限制。在 modules 層建立清楚的 wrench（力、關於明確參考點的力矩）契約；engine/air/chute/RCS 能共同遵循，座標、COM、作用點一律 f64 且明確，不重複加 r×F。
氣流速度包含剛體角速度與作用點偏移；air/chute 的力與力矩須進入 bubble/contact 及 orbit 的姿態/平移積分，姿態变化不能整段使用過期的 frozen attitude 而假稱完整耦合。環境 trial evaluation 純函式；耗油、降落傘等狀態只在接受步提交。質量/慣量動態變更、rails 不適用條件、睡眠地面不被被動空氣喚醒要保住。
用既有 aero 係數/元素接入可配置翼面或穩定翼示例，展示風標效應/氣動阻尼與偏心降落傘力矩；不做熱/燒蝕/破壞。本輪先形成完整力矩通路和可驗收 flight lab，不替換主遊戲預設。headless 驗證零空氣極限、對稱零矩、偏心矩、旋轉局部風、耗散適用條件、不同 dt 收斂與 owner 接縫、存讀/錄放。
擁有 modules::air、wrench契約、Fleet 共用施力/姿態積分與 aero lab。與 RCS agent 協調 assembly model/catalog 與 Fleet interfaces，對外接口先報 root；保持 scenery world optics 不受影響。


---

## 原檔：docs/specs/solar-scenery.md

# 任務：太陽系各天體 scenery 與程序地形

狀態：提供開發入口與交接邊界；實際天體外觀尚未實作。開發入口隨多天體分支合併 master；world schema 2 / MODEL_VERSION 13。

## 目的

從個別 lab 的地球外觀，轉向同一個世界中按穩定 body ID 配置、渲染與驗收各天體。後續負責 scenery 的開發者可直接修改光學／雲／地形配置、擴充 shader 和 terrain core，不必重寫 Fleet、座標樹、存檔或場景生命週期。

## 天體清單與身份

`crates/orbit/systems/sol.json` 的九個主要天體是：

| ID | 類比／工作類型 |
| --- | --- |
| sol | 太陽；發光表面、日冕與遠景，並非可著陸地表 |
| cinder | 水星；無大氣岩石地表 |
| vesper | 金星；濃厚大氣、雲與岩石地表 |
| aurelia | 地球；既有地形、海、大氣與雲 |
| ares | 火星；薄大氣、岩石地表 |
| velvet | 木星；氣態巨行星雲帶，不能沿用岩石地面碰撞 |
| halo | 土星；氣態巨行星與環 |
| azure | 天王星；冰巨行星大氣 |
| abyss | 海王星；冰巨行星大氣 |

`selene` 是額外的月球；系統也有其他衛星。九個主要天體不是全系統天體數，也不意味它們都應有可著陸 terrain。已有 Earth/Moon fixture 是架構測試，並非九顆外觀已完成。

## 已備好的入口

- 世界配置：`crates/fleet-flight/src/world.rs` 的 `WorldDescription` / `BodyDescription` / `VisualSettings`。配置由穩定 ID 綁定，存檔和錄放保存完整內容。
- 地形：`TerrainConfig` → `Terrain::from_config`。新的程序地形型別在 `void-terrain` 增加顯式 enum 分支；renderer 的 `SurfaceSampler` 與 collider 必須使用同一份配置。不要只在 shader 位移地面卻保留舊 collider。
- 光學：`void-scenery::atmosphere_scene::AtmosphereProfile::{EarthScaled, Custom}`。Custom 可配置大氣厚度、Rayleigh RGB／尺度高度、Mie 散射／消光／尺度高度／相位，以及吸收層。半徑由 body radius + air datum 派生，不重複填寫。
- 雲：`CloudProfile` 配置雲底／雲頂、覆蓋率和消光係數；高度相對 color datum。現有 noise recipe 仍共用，需不同雲帶／天氣時在此擴充資料與 shader，不能宣稱僅調這四個值即可完成巨行星。
- 材質：`app/src/scenery.rs`、`app/src/shaders/scenery/`。地表 shader／海洋屬於 GroundMaterial；太陽發光、環與氣態表面需各自明確的 renderer recipe，不能製造假 terrain 來通過地面 renderer。
- 多天體 renderer：`app/src/multi_body.rs` 管理 per-body LUT、地面材質與 tile；`app/src/air.rs` 的 `AirLayers` 管理 per-camera 大氣 transport。各層保持 HDR，最終曝光／tone mapping 只一次。
- 觀測：frame tree → camera-relative body-local 座標；禁止先轉絕對 f32。不要用 home 替代當前觀測天體。

物理大氣目前仍是 `EarthAtmosphere` 的 density scale 模型。自訂光學參數改的是散射外觀，不會自動變成金星壓力／溫度模型。要改物理，在 environment core 加顯式模型並驗收，不以 scattering coefficient 代替密度。

## 直接使用的 lab

在主 repository：

```sh
cargo run -p void-app --example multi_body -- --two-atmospheres
cargo run -p void-app --example multi_body -- --two-atmospheres --export-world /tmp/scenery-world.json
cargo run -p void-app --example multi_body -- --world /tmp/scenery-world.json
```

JSON 是完整 `InitialWorld`（含 world、launch_body、craft、launch_site），不是 GPU 資產。lab 目前為 Aurelia/Selene 操作場景；`--two-atmospheres` 給 Selene 加上虛構的紅色測試大氣，只驗證獨立 optics 與合成，不能當成月球設定。預設月球仍無大氣。修改配置需重開；不做未版本化的 runtime shader/physics 熱切換。

Tab 切船；1/2 觀測母星／月球；Home 回船；O 生成觀測天體的軌道船；P 暫停；F6/F7 存讀；F8 停錄。後續 solar scenery lab 應提供任意 body ID 選取、近景／軌道／遠景固定視角與參數讀數，不能讓九顆天體仍只能選兩顆。

## renderer 邊界

- 已支持多個分離的大氣體積，每個有自己的 LUT、光學／雲參數及體座標；大氣可無 terrain。未觀測天體的大氣仍參與 transport。
- 遠到近的整體積合成，只支持順序明確的視角；角向投影重疊且徑向範圍也重疊時拒絕。大氣球互相穿透時也拒絕。需要這些視角時，實作共同逐射線積分／逐像素排序，不能改成任意中心距離排序。
- 現有視角仍要求觀測天體有 terrain；沒有 terrain 的大氣可在遠景渲染，但太陽／巨行星近景觀測仍待新增 renderer recipe。這是後續實作的一部分，不標為已完成。
- 未實作：多恆星光照、遮食、環陰影、雲影、移動天氣、極光、植被、地形新生物群系，以及巨行星氣體的物理進入／毀損規則。
- 共用 immutable noise 不代表共用 LUT 或配置。新增 texture/material/mesh 必須登記 per-world ownership，reset/load 釋放，舊 generation 的 task 不得污染新場景。

## 建議交付順序

1. 先做 scenery solar lab 的任意 ID 選取與 body visual recipe（SolidSurface / GasEnvelope / EmissiveStar / Rings 的實際方案先寫設計，避免未使用的 placeholder enum）。保留既有世界存檔、frame tree 和 renderer cache。
2. Selene / Cinder / Ares：補岩石地形、撞擊坑、配色；數值核對繪圖與碰撞。
3. Vesper：自訂大氣和雲；獨立 headless LUT／極端參數檢查，再視窗驗收。
4. Sol：發光與日冕，核對遠景尺寸／曝光與其他天體照明的責任。
5. Velvet / Halo / Azure / Abyss：氣態表面與雲帶；Halo 的環是獨立幾何／光照工作。
6. 多顆天體同畫面驗收、資產上限與錄放；使用者接受 lab 後才決定主遊戲整合。

## 完成證據

每顆附配置、近地或近雲層／軌道／遠景固定視角與限制說明。大氣 LUT 不得含 NaN/Inf；零散射真空極限保留；不放寬 golden。碰撞只測實際具有 solid terrain 的天體。存读檔保留自訂配置；切焦點不改 physics；反覆 reset/load 的 app-owned 資產回到可說明上限。GPU 驗收不能由 headless 通過代替。

此任務不重做零件狀態、資源、對接、Fleet owner 或星系軌道，也不要求一次完成所有九顆才能交付第一顆。

