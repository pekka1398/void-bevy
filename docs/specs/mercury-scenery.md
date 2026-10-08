# Cinder：水星觀感程序地形

本輪基線 `183b9ba`，分支 `work/mercury-scenery`。開發流程引用根 AGENTS.md；本任務使用者明確允許直接桌面或 TigerVNC，agent 自行觀察 GUI，但最後仍需人類驗收。

## 範圍與接口

- Cinder 的自訂地理不是水星全球地圖重建。沒有真實坑名／位置、全球照片或地表貼圖。
- `void-terrain::ImpactOptions` 是可序列化的地質配置，`ImpactTerrain` 是唯一 f64 高度與線性反照率取樣器；盆地／射紋撞擊配置與生成機制分離，Cinder preset 在 `impact/cinder.rs`。
- 共用 Terrain、SurfaceSampler、TileField、void-frames、environment、Fleet 及 owner 切換。沒有另一套景觀世界或物理 runtime。
- 大盆地、局部填充平原、不同尺寸／退化程度的坑、中央峰／階地、有限長推覆陡崖和噴出物是高度／材質場。近距離 shader 僅增加低於 mesh 的顆粒外觀，不冒充可碰撞大型地形。
- 高度仍遵守 `[0,max_height]` 的既有 terrain contract，datum 是 reference sphere 以上的正偏移。不得以 clamp 遮蔽非法高度。非法配置／非有限輸入明確 panic。
- `SurfaceRecipe::Regolith` 選擇 airless particulate 光照和 sampler 材質；不套用陸地高度雪線／棕色岩石覆蓋。Aurelia 使用原 SolidSurface 路徑。
- Cinder 無海、雲與光學／物理大氣。radius、mass、spin、orbit JSON 不改；原本已接近水星的 2440 km、58.6 日。7° 的 spin inclination 使用現有 ecliptic 框架，不誤作相對軌道面的 obliquity 修改。

## 跨尺度與接縫

大盆地、平原和稀疏亮射紋組織全球外觀；多級坑族連續覆蓋數百公里至數十公尺，粗糙度下探公尺。地形與 collider 以同一 `build_tile_mesh`／cell 尺度 band-limit，full-detail point query 與粗 mesh 不混為同一高度。高解析度不得無限擴充全球網格；先量取樣／LOD 成本。

坑族以固定 cube face ownership 生成，邊緣重疊且所有影響範圍都納入查詢，跨 cube face 不切換地形函數。測試要覆蓋跨面連續性、hash 網格界線、不同 cell、盆地／坑的形貌與实际 collider。

## 格式

新 TerrainConfig／SurfaceRecipe 變體是明確配置；world schema 保持 3，既有欄位結構未變。整合 model 升至 21，因預設世界的實際地表／碰撞行為改變，model 20 journal/checkpoint 不自動遷移。FleetCheckpoint 8、Craft 2 保持。

## 驗收與限制

在真正主遊戲以 `--body cinder --view orbit` 直接啟動，另看 far/near、固定地貌、F2–F5、F6/F7、journal/checkpoint。不改相機控制架構，不以 lab 代替接線。agent GUI 先檢查 shader、LOD、光照、材質和記憶體 ownership，再交使用者驗收。

本輪不含極區水冰／永久陰影熱環境、地理重建、撞擊演化時間模擬、其他天體美術或整個太陽系重配。

## 參考用途

使用者 11 張圖片用於形貌／光照／材質差異辨識，增強色及彩虹地形圖不作自然地表配色。科學方向核對：
- [NASA Mercury facts](https://science.nasa.gov/mercury/facts/)
- [NASA Mercury's subtle colors](https://science.nasa.gov/resource/mercurys-subtle-colors/)：材質差異、亮噴出物和盆地平原。
- [NASA extensive smooth plains](https://science.nasa.gov/photojournal/extensive-smooth-plains-on-mercury/)：大片古火山平原。
- [NASA first global topographic model](https://www.nasa.gov/missions/first-global-topographic-model-of-mercury/)：形貌與地形色不同於肉眼顏色。

## 主 renderer 的頻帶分工

初輪 GUI 已確認僅靠粗網格頂點顏色／法線會抹掉全球坑貌。因此保持原 LOD 密度，tile 額外提供 4-byte `TerrainCell` 頂點屬性；Regolith 像素 shader 以同一整數 feature hash、尺度／年齡／profile 求可見坑坡度，減去 cell 尺度的中央差分估計後補上像素尺度的坡度。這是粗 mesh 法線的頻帶重建近似，不新增撞擊物理或修改 collider。盆地與陡崖依然由真實 mesh 表現。材質在像素上連續求值，避免亮射紋變成沿 tile 的鋸齒／模糊色塊。

CPU／WGSL 的 impact profile 是同一規則的兩種實作；變更 profile 時須同步並做實際 GPU 核對。小於像素的特徵平滑退出；遠端 f32 不先參與物理位置計算。所有材質參數來自實際 ImpactOptions，沒有 shader 內另藏一份 Cinder seed 或顏色。

`--cinder-site basin|rim|ejecta` 只是普通 InitialWorld 的已知位置、暫停起始主遊戲；可與 `--craft` 選船，但不能覆蓋 world/load/replay/planet/terrain 或其他 fixture。`tools/mercury-acceptance.sh` 提供這三種地面與 near/orbit/far 入口。
