# 目前狀態

（2026-10-05，零件狀態／資源與多天體場景在審查修正後合入 master；整合模型 13）本頁回答「現在做到哪裡、還缺什麼」。各項的細節與驗證紀錄見連結的文件；NOTE.md 是原始筆記與願望清單，其中的完成狀態是歷史紀錄。

## 一句話

TS→Rust 搬遷已完成。主遊戲已改用 assembly 船與 Fleet 多船物理，具備有大氣阻力的飛行、分級、多船、SAS、機動計畫、九級 warp，以及完整存讀檔、操作錄放、碰撞疊圖與 CPU／GPU 量測。使用者已在本機完成視窗驗收。RCS／對接正在進行，尚未合入 master。

```sh
cargo run -p void-app                        # 主遊戲：Aurelia、預設 flight-rocket
cargo run -p void-app -- --craft my.json     # 用 assembly-lab 匯出的船
cargo run -p void-assembly-lab               # 組船
```

## 多天體與零件狀態 lab

多天體任務已建立同世界 Aurelia／Selene 地形與兩地 Fleet 船的獨立 lab；世界描述、InitialWorld、存讀檔與錄放已共用多天體配置。可執行 `cargo run -p void-app --example multi_body`；操作、數值檢查與 renderer 限制見 [multi-body-scenes.md](multi-body-scenes.md)。已依使用者授權審查後合併；多天體仍是獨立 lab，未啟用為主遊戲預設。

## 主遊戲現在有的

| 功能 | 狀態 | 文件 |
| --- | --- | --- |
| assembly 船 | 主遊戲讀 assembly craft；預設 `flight-rocket.json`（7620 kg、理想 Δv 9.6 km/s、14 個零件含四隻著陸腿）；`--craft` 可換船 | [fleet-flight.md](fleet-flight.md)、[assembly.md](assembly.md) |
| Fleet 多船 | orbit／bubble／ground 三種 owner 交接、交會氣泡、分離與合併、逐船油門／SAS、Tab 切船、N／O 生成第二艘 | [vessels.md](vessels.md) |
| 大氣 | aero 的大氣與零件阻力、噴嘴氣壓；只有力（force-only） | [fleet-flight.md](fleet-flight.md) |
| 行星與畫面 | 發射行星的 LOD 地形、大氣、海、體積雲、星空、navball、多天體 map | [scenery.md](scenery.md)、[game.md](game.md) |
| 機動 | 逐船機動計畫、Pe／Ap 定位、理想軌道導引執行（B）、機動前快轉（Z）、九級 warp 與攔截 | [fleet-flight.md](fleet-flight.md) |
| 存讀檔 | F6／F7、`--load`、`--verify-save`：直接保存完整 live 世界（含 native Rapier cache），載入後可續玩 | [fleet-flight.md](fleet-flight.md) |
| 錄放 | `--record`／`--replay`／`--verify`／`--recover-recording`：逐條 sync 的 JSONL journal，崩潰可恢復已提交部分；相機操作也會錄 | [fleet-flight.md](fleet-flight.md) |
| 疊圖 | F2 線框、F3 tile 邊界、F4 實際 Rapier collider、F5 地形顯示 | [fleet-flight.md](fleet-flight.md) |
| 量測 | CPU：`--profile`、F9；GPU：`--render-profile`、離屏 `--render-benchmark` | [fleet-flight.md](fleet-flight.md) |
| 接縫檢查 | `void-seam-check` 六類共 1200 案例通過，失敗案例保存可重跑 | [seam-check.md](seam-check.md) |
| 舊主遊戲 | 搬遷時的 PartJointRocket 主遊戲保留為 `--example legacy_flight` 回歸場景 | [game.md](game.md) |

A／B 兩批工作（主遊戲整合＋存檔；profiling、疊圖、錄放、差分檢查）的完成核對見 [ab-progress.md](ab-progress.md)。

## 驗收與測試

- 視窗驗收：使用者已在自己的本機完成，包括主遊戲手動飛行與 A／B 內容。先前的文件寫成「之後補做」，是因為沒有記錄。
- 測試：在 `a730519` 記錄的 `cargo test --workspace --all-targets` 為 283 passed、0 failed、4 ignored。ignored 包括一項需要 GPU 的 opt-in 測試，以及已結案為非缺陷的 Pebble 靜止傾角（量到的是場地坡度，見 [vessels.md](vessels.md)）。本頁的文件修改沒有重跑測試。
- GPU：只在使用者本機與 RTX 5060 Laptop／Vulkan 離屏 benchmark 上跑過，其他 GPU／平台沒有測過。

## 已完成的架構重構

- **座標樹統一已完成並合入 master**：所有座標走同一棵 `void-frames` 樹，從銀河、恆星系、天體、地面場景到船；相機掛在焦點下，零件、碰撞線與 tile 直接轉到相機，不再繞經 1 AU 的質心系。使用者已完成主遊戲、multiscale example／lab 的視窗驗收。審查修正了 plain lab 天體焦點的 NaN、探測器焦點的 split 精度，並把模型版本升到 5。修正後相關 40 項測試、受影響範圍 clippy 與 fmt 通過；未重跑全量。見 [frame-tree.md](frame-tree.md)、[frames.md](frames.md)。
- **環境介面已完成並合入 master**：重力共用 `void_orbit::gravity`；`void-environment` 在座標樹上查重力、大氣、地形與海深。Fleet、舊火箭與再入 lab 共用環境取樣，積分器不再線性外推天體中心。layered 大氣與散射天空從海平面起算，水的物理仍待開發。審查補上環境／星曆世界描述核對，相機固定追蹤 craft root（主遊戲為上面級指令艙），避免分離時因質心切換而跳動；該次合併的 `MODEL_VERSION` 為 7。使用者已完成本輪視窗驗收。整合後 42 項針對性測試通過，相機修改後另有 19 項相機／存檔／錄放檢查通過，相關 Clippy、fmt 與主遊戲／multiscale example 編譯檢查通過；未重跑全量。見 [environment.md](environment.md)。
- **零件圖整合已完成並合入 master**：`void_assembly::PartGraph` 是 Fleet 唯一零件紀錄，燃料、分級、點火、pose 在零件上；每艘船為連通分量加物理擁有者，每個零件有座標樹節點，噴嘴面積進 catalog。審查補上還原連通性、PartGraph 基本狀態驗證與受控修改接口，跳過未改變父節點的 reparent。整合保留世界核對與上面級相機，`MODEL_VERSION` 8。使用者完成視窗驗收並同意合併；F6 存檔已加入緩衝寫入，修正大量小型檔案寫入造成的卡住。見 [part-graph.md](part-graph.md)。
- **零件模組整合已完成並合入 master**：引擎背壓與零件阻力集中到 `void-modules`，Fleet 直接讀環境，移除外掛力接線；修正非法氣壓掩蓋與軌道段累積時鐘誤差，整合版模型 9。保留前幾輪審查修正與 F6 緩衝存檔。使用者已完成本輪視窗驗收並同意合併；101 項針對性測試、相關 lint、編譯檢查與 fmt 通過，未重跑全量測試。見 [part-modules.md](part-modules.md)。

## 進行中

- **RCS／對接**：噴嘴分配、捕獲判定、接點吸附、解除對接、單推進劑與 RCS 零件，以及對接 lab 場景，尚未合入 master。合入前需要視窗驗收。

## 對照 NOTE.md 的願望清單

| # | 項目 | 狀態 |
| --- | --- | --- |
| 1 | 零件組裝 | 完成：編輯器 lab＋主遊戲讀取。限制：只有堆疊接點與單一推進劑，沒有表面接合、對稱或結構破壞 |
| 2 | 軌道機動、N 體 | 完成：N 體星曆、有限燃燒計畫、逐船機動與導引 |
| 3 | 軌道／飛行視角切換 | 完成：map 淡入、多天體 map、標籤焦點、inertial／surface path frame |
| 4 | profiling、GPU、SIMD、多執行緒 | profiling 完成（CPU／GPU）；效能優化本身尚未系統性進行 |
| 5 | scenery | 大氣、海、體積雲、星空、分層地形已有。雲的移動與雲影、極光、天氣、植被、生物群系配色未做 |
| 6 | 火箭／飛機零件 | 指令艙、油箱、引擎、分離器、著陸腿已有。降落傘 core／獨立 lab 已完成，主遊戲預設 craft 尚無傘；飛機只在 aero-lab 有寫死的機體，沒有飛機零件 |
| 7 | 空氣動力、燒蝕、熱 | aero-lab 完整（力矩、翼面、熱、燒蝕、再入）。主遊戲只接了阻力，力矩／熱／燒蝕／翼面未接 |
| 8 | 交會、對接 | 交會完成；對接進行中（見上） |
| 9 | 多船 | 完成 |
| 10 | SAS、旋轉、RCS | SAS 穩定／鎖定姿態完成。順行等進階模式、有限轉向時間未做；RCS 進行中 |
| 11 | 存檔 | 完成：直接世界存檔＋錄放。沒有跨模型版本的存檔遷移 |
| 12 | 參考框架切換 | 完成：frames 樹、orbit-lab 四種繪圖框架、multiscale 的跨星系換框架；已統一到同一棵樹並完成視窗驗收 |
| 13 | 其他天體的程序地形 | 多天體 lab 已支持同世界多個可著陸天體；各太陽系天體專屬地形／美術未做，主遊戲預設仍單近景 |
| 14 | 水上漂浮 | 未做：環境介面已提供海平面與深度（`Surroundings::sea`），浮力、水阻力、濺落等水的物理之後做 |
| 15 | UI | 只有 HUD 與按鍵操作，正式 UI 未做 |
| 16 | 太空人 EVA | 未做 |
| 17 | 車輛 | 未做 |
| 18 | multiscale | 核心與 lab 完成，並在接縫檢查中與 Fleet 共用；未接入主遊戲 |
| 19 | 相對論 | 未做 |
| 20 | 更多引擎類型 | 未做：catalog 只有四個液體引擎定義（原 lab 大／小，加主遊戲上級／助推級），沒有固體、離子等類型 |

其他未做：撞擊毀損（舊主遊戲也沒有啟用）、音效、科技樹、主線。

## 已知缺口與限制

- 主遊戲沒有 scenery 開關與曝光調整（scenery example 有）。
- 噴嘴面積已是 catalog 的正式欄位（`nozzleExitAreaM2`）；新增引擎需在定義中提供，沒有額外的 engine ID 對照表。
- 搬遷時列出、仍未補的觀察工具：`lod` example 的 preset 選擇、vessels-lab 的歷史軌跡、multiscale encounter 的任意天體／船選取。詳見 [port-audit.md](port-audit.md)（歷史頁）。
- 存檔與錄影綁定模型版本、catalog 與 Rapier 版本，版本不同時直接拒絕，沒有遷移。

## 本輪合併：零件狀態／資源與多天體

`claude/part-state-resources` 把 Fleet 的 live 零件可變資料統一為 typed resource map、按穩定 module ID 尋址的 state/stage map，新增雙資源供應與 force-only 降落傘。Craft 2／FleetCheckpoint 4／整合 model 13 明確區分基線；舊 craft 可用顯式離線工具轉換。主遊戲預設火箭未新增傘。獨立 `void-part-state-lab` 與驗收操作、限制見 [part-state-resources.md](part-state-resources.md)。整合版 world schema 2 / model 13，分支暫用的 10 / 12 明確拒絕。

多天體後續追加：多顆分離大氣 HDR 合成、自訂散射／雲配置與 solar scenery 交接規格已實作；world schema 2 / 整合 model 13。已合併 master，實際九天體外觀未做，見 [specs/solar-scenery.md](specs/solar-scenery.md)。

合併核對（2026-10-05）：受影響核心／場景 **245 passed、0 failed**；主遊戲原有 offscreen GPU 重建／存檔測試另跑 **1 passed**。唯一仍跳過的行為測試是已結案的 Pebble 傾角；沒有重跑全 workspace。所屬 crates all-targets Clippy `-D warnings`、fmt、主遊戲與兩 lab 編譯通過。新增第二天體降落傘 checkpoint／續跑／錄放，以及分支舊模型拒絕回歸。
