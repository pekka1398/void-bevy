# 任務：太陽系各天體 scenery 與程序地形

狀態：提供開發入口與交接邊界；實際天體外觀尚未實作。基於尚未合併的多天體 worktree，不以 master 已完成計。

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

在多天體 worktree：

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
