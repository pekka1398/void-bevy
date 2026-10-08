# 主遊戲 Cinder：水星觀感

2026-10-08，`work/mercury-scenery`，基線 `183b9ba`。使用者已驗收並授權合併；root 已審查並採入 master，未 push。範圍／接口見 [spec](specs/mercury-scenery.md)。

## 可執行入口

在本 worktree：

```sh
./tools/mercury-acceptance.sh orbit
./tools/mercury-acceptance.sh near
./tools/mercury-acceptance.sh far
./tools/mercury-acceptance.sh basin
./tools/mercury-acceptance.sh rim
./tools/mercury-acceptance.sh ejecta
```

前三項直接把主遊戲焦點放到 Cinder，不必點 map label。後三項是普通 InitialWorld 的固定地面起始位置，暫停啟動、預設同一火箭；P 放行。左鍵拖曳／滾輪沿用主相機，Home 回船；F1 看不同距離，O 生成目前觀測天體的軌道 fixture。F2 線框、F3 tile、F4 真 collider、F5 地形顯示；F6/F7 存讀，F8 結束錄製。

對應主遊戲 CLI：`--body cinder --view near|orbit|far`、`--cinder-site basin|rim|ejecta`。`--record <journal>`／`--save <checkpoint>` 可以直接附加到腳本。地面 fixture 不可混入 world/load/replay/planet/terrain 或 reentry/rendezvous。

驗收 binary：本 worktree 的 `target/acceptance/void-app-mercury`。重新建立：

```sh
cargo build -p void-app -j 2
mkdir -p target/acceptance
cp target/debug/void-app target/acceptance/void-app-mercury
```

本輪曾發現共享 target 混用其他分支 rmeta，之後改為獨立 target，清除全部本地 crate 產物後重建；最終 binary 來自本 worktree。不要直接使用主工作區可能已被其他分支重寫的 debug binary。

## 外觀與物理

這是自訂水星式地理，不重建真實坑／盆地位置。5 個大盆地、局部平原填充、9 級不同半徑和退化程度的坑族、複雜坑平坦底／中央峰／階地、有限長陡崖和6處稀疏年輕亮射紋共同組成 Cinder。自然色為微暖灰和低反照率省區；增強色／彩虹參考沒有變成地表顏色。

盆地／坑／陡崖與公尺粗糙度是 `void-terrain` 的實際 f64 高度，渲染和碰撞共用同一配置／cell-limited sampler。高度遵守既有正值 contract，8 km datum 是 reference sphere 之上的地形偏移，不是另一個天體半徑。Cinder 原半徑2439700m、質量3.3011e23kg、自轉5067014.4s及 orbit JSON不變。

主 renderer 沿用原 LOD 密度。像素 shader 用同一坑族身份和形狀補足粗 mesh 法線的可見頻帶，連續取樣反照率與射紋；近地面由真實網格接手。這是法線重建近似，並非另一套物理表面。岩屑光照使用 Lommel–Seeliger／Lambert 混合與很弱的小相位角增亮，沒有宣稱為標定的 Hapke 模型。微小顆粒只做 shader 細節。粗網格坡度補償是近似，沒有新增逐像素地形投影陰影；陰影／反射尚非完整光線追蹤或經量測標定的水星光度模型。

Cinder 無海／大氣／雲；Aurelia 原地形與光照分支保留。沒有新增氣態巨星框架、星環功能、EVA、冰或極區熱環境。高相位斜照能清楚讀到坑壁；較迎光時主要看材質和射紋。地面平原可以很平滑，環山與較密集坑群可用 rim/ejecta 入口及滾輪拉高觀察。

## 主線整合

2026-10-08：使用者確認外觀沒有問題並授權合併。採入 `4294b0f`，保留主線 EVA／rover、飛機、星系及水功能。整合 model **28**／world **4**／FleetCheckpoint **12**／Craft **3**；下方 model21 與驗證紀錄是水星分支證據，不代表整合版可載入分支存檔。model27 與分支 model21 檔案均明確拒絕。Cinder fixture 與其他主遊戲 fixture 不可混用。

整合工作區（`796761d` + `4294b0f` 與 root 接縫修正）針對性核對：impact 4、solar_scenery 5、app lib 30 項通過；app／Fleet `--lib --tests` Clippy `-D warnings`、fmt 與主遊戲 build 通過。未跑全 workspace。主線驗收 binary 為 `target/acceptance/void-app-mercury`，SHA256 `e5bf986fdb2698bc37d3002bfdfadebaf50d15dfdea08351b059cd2206ed14fe`。

## 分支格式與驗證

model **21**，world schema **3**／FleetCheckpoint **8**／Craft **2**。新 terrain/surface enum 有明確序列化配置；既有 world 欄位結構未變。model 20 的 journal/checkpoint 明確拒絕，沒有自動遷移。

本輪針對性檢查，未跑 workspace 全量：

- terrain 9項、scenery 13項與 Fleet 原77項通過；最後坑壁／退化參數調整後另跑 impact 4項（含10萬方向全細節高度／色域）及solar collider/checkpoint接縫。
- `cargo test -p void-app --lib -j 2`：27 passed，包括三個真實地面 InitialWorld、十天體主 renderer 切換、同世界 restore 的資產數穩定。
- 最終 solar 接縫5項通過，包含 Cinder 新 recipe、model20拒絕、同 cell 渲染／collision頂點1mm門檻；沒有放寬舊門檻。
- 受影響四 crates `--lib --tests` Clippy `-D warnings`、fmt、主遊戲編譯通過。
- agent TigerVNC :12／RTX5060 Laptop／Vulkan 自行操作多個光照相位、全球／中景／地面，觀察材質、坑、射紋、真collider與存讀；這不代替人類最終驗收。

中間美術版混合旋轉／縮放1440×900 GUI capture的 opaque pass p50 2.12ms、p95 9.36ms，沒有 render error；同期其他視窗／編譯存在，不當專用性能基準。最終證據在本 worktree ignored `lab-log/mercury-evidence/`：

- `final-near-v2.jsonl` 與 `final-save.json`：近景、F6/F7／相機，最後 binary 再驗證通過（T+0，觀察不推進物理）。
- `final-basin.jsonl` 與 `final-basin-save.json`：Cinder 真地面 owner，P短跑1.283333s、F4、F6/F7，再以最後binary核對 journal及checkpoint皆通過。
- `final-near.png`、`final-orbit.png`、`final-far.png`、`final-ground-collider.png` 與 `final-aurelia.png`：agent實際看過的主renderer截圖。最終GUI logs沒有panic／shader validation error。
- 中間版 shader/GPU profile及diagnostic圖保留於同目錄／`/tmp/cinder-*`，只作迭代資料，不能代替最終binary或人類驗收。

## 人類驗收

1. orbit：放大並繞行，檢查灰色自然材質、大盆地與平原對比、稀疏射紋，不是均勻相同坑。
2. near／rim／ejecta：從中景降到地面，看真實起伏、坑壁和材質頻帶切換；F2–F5 對照。
3. basin：P短跑、F6→F7，再續跑／起飛；船與地表維持一致。用錄製重播核對。
4. 同binary看 Aurelia／Selene，確認其他原有天體和主遊戲控制沒有被換成水星外觀。

主線整合 GUI：同 SHA binary 的 Cinder／Aurelia、縮放／F1／F2／F6-F7／Home 核對正常；1.05秒 journal 與 checkpoint verify 通過，無 shader error／panic。root 已檢視整合 Cinder 與 Aurelia 截圖。另水與多星系19項接縫測試通過。
