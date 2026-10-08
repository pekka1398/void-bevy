# Ares：主遊戲火星式地表

2026-10-08；`work/mars-scenery`，基線 `74aec6b`。共用規則：[AGENTS.md](../../AGENTS.md)。

## 範圍與研究

直接替換主遊戲 Ares 佔位天體。自訂程序地理，不重建真實地圖、特定登陸位置，不用全球照片 texture。維持既有半徑、質量、自轉和 orbit；使用原 frames、LOD、tile mesh、collision、Fleet owner、主相機、存讀和錄放。

- [NASA Mars facts](https://science.nasa.gov/mars/facts/)：氧化鐵塵、撞擊坑、火山、峽谷、極地冰。
- [JPL InSight Mars at a glance](https://www.jpl.nasa.gov/news/press_kits/insight/landing/facts/mars-at-a-glance/)：自然外觀黃褐至紅色；盾狀火山約600km寬、最高約26km；大型峽谷3–6英里起伏，約4000km長。這些是地貌量級依據，不是複製真實地理座標。
- [NASA Mars red dust research](https://www.nasa.gov/centers-and-facilities/goddard/nasa-new-study-on-why-mars-is-red-supports-potentially-habitable-past/)：紅塵與氧化鐵、暗色基底、古代水作用。此處不模擬礦物光譜或地質演化。

- [JPL Pathfinder sky colour](https://www.jpl.nasa.gov/images/pia00917-color-variations-in-the-sky-at-sunset/)：白天塵霧的紅褐天空與太陽附近的藍色前向散射。現有RGB有效散射模型只近似紅塵天空，不宣稱重建完整相函數／真實夕照光譜。

已實際查看主工作區 `ref/celestial/mars` 全9張參考截圖。全球赭色塵覆／暗玄武岩、白極冠與近地分層岩石作觀感依據；其中混合假色、增強藍色和想像山景，不能直接當自然色。無像素資料進 runtime。

## 所有權與接口

`void-terrain::AresTerrain` 組合原 `ImpactTerrain`，不更改 Cinder 行為。AresOptions 明確序列化 impact 配置與盾狀火山尺寸；地形在 body-fixed f64 單位方向查詢。低地／高地分界、火山隆起、有限峽谷、近地粗糙度與極地沉積均在 sampler 的真實高度內；tile以格距濾波。正值高度contract使用8km基礎datum，非更改天體半徑。

`SurfaceRecipe::MartianRegolith` 明確區分有薄大氣照明的塵土。現有GroundUniforms regolith分支值2、相同impact uniforms；尾端新增ares_rise／ares_canyon資料欄位，與其他天體字段獨立。shader只重建mesh未解析撞擊坑的法線頻帶與微粒材質；宏觀地貌來自sampler。Venus的cloud／air接口由另一分支owner管理。

## 交付條件

- 主遊戲 `--body ares --view near|orbit|far` 直接觀察。
- `--ares-site plains|canyon|volcano` 是普通 InitialWorld fixture，暫停啟動；不得混合world/load/replay/planet/terrain或其他fixture。
- 針對性terrain範圍／連續性／序列化、Fleet renderer-collider／存讀、app fixture測試與lint。
- 實際GUI全球、中景、地面操作與截圖；使用者最後驗收獨立列出。
- model29，world4／FleetCheckpoint12／Craft3不變；model28拒絕，不自動遷移。root整合時統一版本。

## 限制

光學大氣使用既有Custom接口的RGB有效散射近似，增加Ares塵霧相對於佔位版的光學厚度，保留薄大氣尺度，物理大氣仍未啟用。沒有把地球大氣density scale當作火星大氣；未加入氣候、塵暴動態、冰熱力學或體積碎石實體。極冠是固定季節材質及真實低幅地形。光照是粗糙表面近似，非標定礦物BRDF；沒有像素級地形投影陰影。

## CPU／shader省區同步

`rise_direction`、`rise_width`、`canyon_direction`是AresOptions必要欄位；火山方向已在ShieldVolcano。fixture讀同一canyon／volcano資料。GroundUniforms尾端`ares_rise`直接上傳terrain的rise中心與寬度，並非shader另選地理位置。下列無量綱侵蝕／再鋪覆規則在`ares.rs`與`ground.wgsl`保持一致：

| 規則 | 兩端值 |
| --- | --- |
| 高地轉換 | smoothstep(-0.12, 0.24, z + 0.16 noise(4.2,19) + 0.06 noise(13,31))，取1減值 |
| 隆起遮罩 | max(1−(chord/rise_width)²,0)²，方向／寬度來自同一options |
| 撞擊坑剩餘強度 | 1−max(1−highland×0.92,rise×0.94)×0.94 |

shader只乘到既有Impact未被mesh解析的法線頻帶；原Impact的cell/pixel濾波保留。修改上述模型需核對兩端並更新model版本。宏觀高度由sampler與mesh權威提供。


## RSS參考與最終谷地頻帶

已讀本機`ref/RealSolarSystem` commit `75139b9` 的[官方Mars.cfg](https://github.com/KSP-RO/RealSolarSystem/blob/75139b9/GameData/RealSolarSystem/RSSKopernicus/Mars/Mars.cfg)：ScaledVersion以global normal支援遠景，PQS以steep triplanar、近距紋理與對比處理地面，兩者有高度交接區間。這是呈現分工參考，沒有移植MarsHeight／MarsColor／Mars_NRM或其真實地理；也沒有把註明尚未為Mars訂製的ambientColor當作物理數據。VOID保持既有LOD與光學架構。

最終canyon_mask的CPU與WGSL使用同一上傳中心／穩定basis，主壁smoothstep(0.55,0.85,q)、外側bench(0.85,1.35,q)，主／支谷權重1／0.6、端部bump及邊界noise相同。舊坑幾何與未解析坑法線共同乘(1−canyon_mask×0.94)；細尺度真地面粗糙度亦在谷底減弱。宏觀谷寬／深不因畫面而縮放。這一塊僅調制Impact剩餘頻帶，沒有重新添加已由mesh解析的宏觀法線。
