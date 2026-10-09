# task1：KSP 真實星系視覺包參考環境

目的：用 KSP 的 RSS 類整合包對照 VOID 的星球視覺（大氣、海洋、雲、地形），找出美術上缺漏或效率更好的做法。只看視覺；RO、引擎參數、零件包不在範圍內。

狀態（2026-10-09）：下載、快取、六個比較用副本已建好，CKAN 檢查無缺依賴；**尚未實際啟動遊戲驗證**。

## 環境

- CKAN 1.36.4（`/usr/bin/ckan`），預設實例 `Kerbal Space Program`（Steam，KSP 1.12.5，`/mnt/data/SteamLibrary/steamapps/common/Kerbal Space Program`）。
- CKAN 快取：`/mnt/data/CKAN-cache`（`ckan cache set`），約 18.6 GB；快取上限 Unlimited，不要執行 `ckan clean`。
- 主實例原有的 Kopernicus、KSPCommunityFixes、Harmony、ToolbarControl 等為手動安裝（CKAN 顯示 unmanaged）。

## 下載慢的原因與繞法

- 本機到 GitHub release CDN（`release-assets.githubusercontent.com`）每條連線只有 50–120 KB/s，四個節點都一樣；同時 Cloudflare 7.3 MB/s、SpaceDock 1.7 MB/s。是 ISP 到該 CDN 的路由問題，CKAN 單線下載因此只有約 33 KB/s。
- 繞法：`ssh vultr-jp` 從 GitHub 下載（約 45 MB/s）→ 在 JP 驗 CKAN 記錄的 SHA256 → rsync 回本機（約 7 MB/s）→ 本機再驗 → `yes n | ckan import <zips>`（只進快取，不安裝）。
- SpaceDock 已下架的檔（RVE64KContinued、CashedowtEarthTUFXProfile）從 CKAN 的 archive.org 鏡像取得：`https://archive.org/download/<id>-<ver>/<sha1前8碼>-<id>-<ver>.zip`，SHA 相符。
- 注意：bash `read` 以 tab 分隔時會合併空欄位，TSV 空欄要填佔位符。

## 已快取的內容

- 大氣／雲：RSSVE-LR／HR、PhotoRealisticVisualEnhancement（含 64k、LowRes）、RealismEnvironmentalOverhaul、RVE64KContinued。
- 材質／地形：RSSTextures 4096／8192／16K、RSS-Origin TopoRevamp（configs + 4k／8k／16k 兩部分）、Parallax 2.0.8 + Stock 材質。
- 天空／環／光暈：RSS-Origin GalaxyTex 8k–64k、JSUNrings、DansSunflareRSS、Vaughn's／Astroniki sunflare。
- 後製：TUFX、SigmasRSSTUFXProfiles、CashedowtEarthTUFXProfile。
- 其他星系：KSRSS-Secondary。
- 天體／小行星：RSS-Origin 13 個 CelestialsPack、RSSOrigin-Less-Primary／Secondary、RSS-Expansion、RealSolarSystemExpanded（後兩者與 RSS-Origin 衝突）。
- 依賴：EVE Redux、Scatterer 0.0878／0.0632、TextureReplacer、AdvancedPQSTools v1.3／v1.6.1、VertexMitchellNetravaliHeightMap、VertexColorMapEmissive 等。
- CKAN 沒有的：KSRSS 本體、RSS-Reborn、Blackrack 的 EVE Volumetrics／新版 Scatterer（Patreon）。需要時手動取得，同樣可走 JP。

## 副本

位置 `/mnt/data/KSP-instances/<名稱>/`，直接執行該目錄的 `KSP.x86_64`（Steam 只會啟動主實例）。主實例維持原狀：RSS + RSSTextures4096 + RSSVE-LR + Parallax + Scatterer + EVE。

| 副本 | 內容 | 備註 |
|---|---|---|
| KSP-RSSVE | RSS + 8K + RSSVE-HR + Dan's sunflare + TUFX（Sigma、Cashedowt）+ Parallax | |
| KSP-PRVE | RSS + 8K + PRVE + 64k 地球 + TUFX（Sigma）+ Parallax | |
| KSP-REO | RSS + 8K + REO + TUFX + Parallax | REO 自帶 TUFX profile |
| KSP-RVE64K | RSS + 8K + RVE64K + Parallax | RVE64K 鎖 Scatterer 剛好 v0.0632（KSP 1.10 用），與 Scatterer-config 0.0878 並存；最可能有問題 |
| KSP-Origin | RSS + 4K + RSS-Origin + TopoRevamp 8k + 13 天體包 + GalaxyTex-16k + JSUNrings + RSSVE-HR + Parallax | RSSOrigin 鎖 AdvancedPQSTools 剛好 v1.3 |
| KSP-KSRSS2 | stock 星系 + KSRSS-Secondary + Parallax | |

工具（`/mnt/data/KSP-instances/`）：

- `packs/<名稱>.ckan`：各包的 metapackage，`ckan install --headless --instance <I> --no-recommends -c packs/<名稱>.ckan` 重建。
- `make-instance.sh <名稱>`：clone 主實例 → 拆開可變動的硬連結 → 移除 4K／RSS／Scatterer 組 → 安裝該 metapackage。
- `unshare-mutable.sh <dir>`：clone 會硬連結部分檔案；材質、模型、dll 等唯讀資產維持硬連結，其餘（cfg、存檔、PluginData 等）複製成獨立檔，避免副本改寫到主實例。

建副本時遇到的坑：

- 手動安裝的 Harmony／ToolbarControl 與 CKAN 模組 ID 對不上，CKAN 不覆蓋非自己安裝的檔案 → 副本中刪除手動版，由 CKAN 安裝同版本（已比對內容相同）；Harmony2、ToolbarController 已寫入所有 metapackage。
- `Scatterer-sunflare` 同時是模組名與虛擬名，headless 不會自動挑選 → 需在命令列明確指定。
- 只移除 Scatterer 不會連帶移除 `Scatterer-config`／`Scatterer-sunflare`；sunflare 類包互相衝突。
- DistantObject-RealSolarSystem 用的是 DistantObject 1.9 舊 zip，會與新版 DistantObject 檔案衝突，未放入任何副本。

## 各包改了什麼（看 zip 內容與 cfg 節點）

各包基本不帶自己的 shader；shader／演算法在引擎 mod：Scatterer（預計算大氣散射、海洋、日蝕陰影、sunflare）、EVE（2D 雲層、粒子雲、雲影、城市燈）、Kopernicus（PQS 地形生成）、Parallax（近地表細分與材質混合）、TUFX（後製）。

- RSSVE：雲 cubemap（地球 268 MB，另有金星、火星、土星、海王星）、城市燈、各星球 Scatterer 參數（configPoints）與海洋參數。
- PRVE：同類但材質大很多（主材質 1.8 GB）；15 個 Scatterer、6 個 EVE 設定；KS3P 後製（bloom、eye adaptation）。
- REO：雲 440 MB、極光、間歇泉；每顆星球附自算的 Scatterer 散射表（`.half`），即改了大氣剖面而非只調參數；附 TUFX profile。
- RVE64K：雲 cubemap 2 GB；Earth、Venus、Mars、Titan、Triton 等各有自算散射表。
- RSSTextures：每顆星 `Color`（軌道視角表面色兼 PQS 頂點色）、`Height`（PQS 高度圖）、`_NRM`（軌道視角法線）、`Biomes`。
- TopoRevamp：改 PQS 地形生成本身：新高度圖（地球等為 16-bit `.bin`）、`VertexMitchellNetravaliHeightMap16` 雙三次插值取代雙線性、疊 `VertexSimplexHeight` 噪聲、部分衛星 `_oblate` 扁球、自發光頂點色 shader；另改大氣壓力／溫度曲線與 biome。
- 對 VOID 值得細看：TopoRevamp 的高度圖插值與噪聲疊加、REO／RVE64K 的逐星散射剖面；散射演算法本身看 Scatterer 原始碼（Bruneton 預計算散射）。

## 授權界線

看效果、讀程式碼理解做法沒有問題；寫進 VOID 的程式碼、shader、資料須自寫或來自乾淨的公開資料。參考某 mod 寫的實作，在 commit 或文件記來源。

- Scatterer、TUFX：GPL-3.0；Kopernicus：LGPL-3.0；EVE、MitchellNetravali 插件、AdvancedPQSTools：MIT；Parallax：CC-BY-NC-ND-4.0。
- RSS、RSSTextures、PRVE、REO、RVE64K、RSS-Origin：CC-BY-NC-SA；RSSVE：CC-BY-SA-4.0。材質、`.half` 散射表、逐星調好的數值不直接搬進 VOID。
- 地表圖資回到原始來源（NASA／USGS：Blue Marble、MOLA、LRO 等）。

## 待辦

- 逐一啟動六個副本確認能載入，特別是 KSP-RVE64K（舊 Scatterer）與 KSP-Origin。
- 截圖比較各包效果，整理「效果／對應技術／VOID 現況」差距清單，視需要放入 `NOTE.md` 待決定事項。
- 視需要另建 16K 材質、TopoRevamp 16k、RSS-Expansion／RSSSE 的副本（已在快取）。
