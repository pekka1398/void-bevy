# 目前狀態

> **2026-10-10 交付撤回：整機失去回應。** 使用者回報啟動後滑鼠／鍵盤完全無回應，需強制關機。`tools/navigation-acceptance.sh` 已停用；以下啟動步驟及先前 agent 驗證不代表此版本可安全使用。根因尚未確認，不重新啟動遊戲或以使用者桌面重現。

## Orbit 導航修正分支（2026-10-10）

`work/orbit-navigation` 基於擴充天體 `3dcfa94`，目前仍在
`/home/pekka/Desktop/void-bevy-navigation`，未合併 master、未 push。
核心提供分階段出發／修正／捕獲；主遊戲已接四種參照系、AN／DN、
點選定位及暫停計畫預覽。來源系統與 split 原點各自保留。

原 `60d97e4` 交付的人類操作暴露 Depart 同步求解卡死，不能算驗收通過。
本輪補齊可取消背景求解、120 秒搜尋時限、分幀星曆預覽、結果過期核對、
持續顯示結果與依目標尺度設定窗口。核心改正瞬間 Lambert Δv 與有限
Frenet 推力的初值語義，直接從原 anchor 檢查完整計畫；未放寬精度／燃料
門檻。生成不點火、不耗真實燃料。手動改計畫會清除過期導航數值。
`AcceptNavigation` 錄下實際計畫供重播；model 34，不轉換 model 33 資料。

針對性核心／Fleet／app 測試、lint、建置及 GUI 出發／捕獲已有本輪證據，
不宣稱完整旅程已執行或所有出發窗口都有解。驗收入口直接載入暫停、
原裝上面級就緒的 400 km 軌道場景，目標 Selene，按 Depart 即可開始。
人類驗收尚未通過；agent 截圖不代替使用者操作。詳見
[orbit-navigation](orbit-navigation.md) 及 [spec](specs/orbit-navigation-and-plotting.md)。
未跑全 workspace suite。


## 主遊戲原生 UI 已合入 master（2026-10-09）

`work/game-ui`／`void-bevy-ui` 以封存 `ref/void/src` 的 HUD 為參考，已接時間／warp、分級燃料條與Δv、油門、高度／速度、原導航球、軌道／機動、DEV及說明。使用現有 Action／ViewCommand；新增四種純視覺開關，model31／world5。根審查、針對性 headless、lint、build及已記錄的 agent GUI核對通過；使用者在正常桌面視窗確認初步外觀可接受，完整控制／載具 GUI驗收仍分範圍待續。使用者已明確授權合併；`work/game-ui` 的 `bd20325` 採入主線，未 push。詳見 [main-game-ui](main-game-ui.md) 與 [spec](specs/game-ui.md)。


## Ares／Vesper 已合入 master（2026-10-09）

使用者明確授權合併 Mars／Venus 並移除兩個 worktree。Mars `4a482f4`、Venus `eb17fe6` 已整合；保留兩邊程序地形、光學、普通地面 fixture 與主 camera 入口，三種 surface fixture 互斥。共同 GroundUniforms 與 WGSL 欄位順序一致；Vesper 厚雲漫射修正與 Ares 材質／薄塵參數同時保留。

整合版 model30／world5／FleetCheckpoint12／Craft3。兩個分支曾各自使用 model29，但模擬地形規則不同，因此整合版明確拒絕舊 model29 journal／checkpoint，不自動修補。分支既有 headless／agent GUI／root 審查證據見 [Mars](main-mars-scenery.md) 與 [Venus](main-venus-scenery.md)，僅適用於各自記錄版本；本次合併授權不擴寫為新增人類 GUI 驗收。未 push。

兩個 worktree 的 ignored lab-log、acceptance binaries 與 Venus 未提交 NOTE.md／patch 已保存到 `/home/pekka/Desktop/void-bevy-worktree-backups/{mars,venus}/`；主線原有未提交 NOTE.md 保留，不混入提交。

整合工作區（Mars merge 後、Venus merge 提交前）針對性 headless 驗證：terrain 的 Ares／Volcanic 測試、scenery lib、fleet-flight solar_scenery、app lib 全部通過；包含雙行星取樣／碰撞、checkpoint、地面 fixture 與 renderer ownership。四個受影響 crate 的 lib/tests Clippy `-D warnings`、fmt 與 staged diff check 通過。沒有跑全 workspace 或新的 GUI 驗收。主程式 `cargo build -p void-app -j 2` 通過，Mars／Venus 驗收入口的 binary 均更新為這次主線 build。

## 四功能與水修正已合入 master（2026-10-08）

使用者驗收水的減速、斜入水及漂浮後，明確授權合併；master由183b9ba
fast-forward至ad21813，包含此前整合版的EVA／rover、飛機零件與控制、
三星系以及model27平滑水阻力修正。其他功能的headless／root GUI核對沿用
整合證據，不把水的人類驗收擴寫為其他功能已完成獨立人類驗收。

原agent實作透過cherry-pick及接縫修正整合，不重複merge原分支；worktree保留。
水星其後已依使用者驗收授權合入，見下節。未push。model27／FleetCheckpoint12／world4／Craft3。
驗收入口已可從主線scripts執行；詳見[water-review.md](water-review.md)。

更新日期：2026-10-08。現行開發規則見 [AGENTS.md](../AGENTS.md)，需求與方向見 [NOTE.md](../NOTE.md)。本頁區分核心能力、主遊戲接線、驗證及合併；歷史測試结果只適用於記錄版本。

合併後核對（master/ad21813）：fleet-flight的aircraft、eva、eva_stellar、stellar_world、
stellar_wrench_frames、vehicles及water七組整合測試，37 passed／0 failed。
未跑全workspace；主線程式碼與已驗收候選逐檔相同，主線驗收入口的binary摘要也已核對。

## Cinder 已驗收並合入（2026-10-08）

使用者已確認水星外觀沒有問題並授權合併。`work/mercury-scenery` 的 `4294b0f` 採入主線；root 審查地形／材質／取樣接口，保留既有四功能入口，拒絕 Cinder fixture 與其他 fixture 混用。整合為 model28／FleetCheckpoint12／world4／Craft3，舊 model27 與分支 model21 明確拒絕，沒有自動遷移。分支人類驗收與 agent GUI 證據見 [main-mercury-scenery.md](main-mercury-scenery.md)。未 push。

## 一句話

TS→Rust 搬遷與主要架構重構已完成。主遊戲具備 assembly／Fleet、多船、完整氣動力矩、有限 RCS、對接／解除、分級、SAS、機動、warp、存讀、錄放與量測。RCS／氣動整合已推送至 origin（`0cd5012`）。本輪另將四種繪圖框架、熱／傳熱／燒蝕／防熱盾與十天體第一輪 scenery 接入主遊戲，使用者已於 2026-10-07 確認驗收完成；十天體外觀不是最終美術版。

## 本輪 master 直接整合（人類驗收完成）

使用者授權主 agent 直接在 master、無子 agent。繪圖框架 `168fcf2`、熱系統 `a625be9`；scenery 接線與驗證見 [main-solar-scenery.md](main-solar-scenery.md)。

| 項目 | 主遊戲行為 | 限制／文件 |
| --- | --- | --- |
| 繪圖框架 | 1–4／G 四模式、J／Shift+J 選天體，逐樣本換框架；相機／存讀／journal 共用 | 天體路徑限已存星曆窗口，見 [main-plot-frames.md](main-plot-frames.md) |
| 熱系統 | PartGraph 熱模組、連接傳熱、有限 ablator、防熱盾、失效能力停用；`--reentry`、HUD、存讀／journal | 初版參數、270 K 輻射背景；無碎裂／撞擊毀損，見 [main-thermal.md](main-thermal.md) |
| Solar scenery | 預設十天體；固體共用 LOD／碰撞取樣；氣態雲帶、環、Sol；F1 視角、曝光、O 指定天體軌道 fixture | 三顆光學大氣，只有 Earth 物理大氣；非最終外觀，見 [main-solar-scenery.md](main-solar-scenery.md) |

格式為 model 20／world schema 3／FleetCheckpoint 8／Craft 2，舊模型明確拒絕，沒有自動遷移。本輪採針對性檢查，沒有全 workspace；root TigerVNC／真實 journal 與 checkpoint 核對已通過；使用者於 2026-10-07 確認三項人類驗收完成，對應 master `f06c283`。scenery 最後核對為 26 app／77 Fleet／9 scenery 核心測試、相關 lint／fmt 通過，範圍見整合文件；已於 2026-10-07 推送至 origin。

## 已驗收 RCS／氣動整合

`1fb8ac9` 完成主遊戲控制、HUD、預設 RCS 火箭及近距對接場景，使用者授權後於 `134bb37` 合入 master，之後已 push（origin `0cd5012`）。原 model 17 基線的 root 審查、針對性測試／lint、TigerVNC、journal／save 核對見 [main-flight-integration.md](main-flight-integration.md)。

以下「整理時」表是歷史快照，不是目前進度。

## 文件整理時的歷史快照（整合恢復前）

| 項目 | 核心／場景 | 主遊戲與驗收 | Git 狀態 |
| --- | --- | --- | --- |
| 零件狀態／多天體基礎 | typed resources、module state、世界配置與多大氣已完成 | model 13 基線的主遊戲及相關場景已驗收；多天體仍是獨立操作場景 | 已合入；基線 `41da390` |
| RCS／對接 | 有限噴嘴分配、typed 供油、捕獲／解除、存讀與錄放；分支測試及 agent TigerVNC 已核對 | 主遊戲按鍵、埠選取、HUD、預設 RCS 火箭仍是暫停草稿；新主遊戲人類驗收未完成 | 分支 `4bd5322`；核心合入 `155a4f8` |
| 完整氣動力矩 | Wrench、翼面／偏心傘、姿態／平移耦合；分支測試及 agent TigerVNC 已核對 | 主遊戲 Full 模式接線為暫停草稿；新主遊戲人類驗收未完成 | 分支 `366f270`；核心合入 `0c60aad` |
| Solar scenery | 十天體第一輪可辨識外觀，固體地形 LOD、光學、球殼雲帶／環／恆星；agent 已核對 | 非最終美術版，未啟用主遊戲；細緻外觀後續處理 | `work/solar-scenery` 工作區，未 commit／合入 |

整理時 HEAD 為 `0c60aad`：整合 `MODEL_VERSION = 17`、Craft 2、world schema 2、FleetCheckpoint 7。新版本明確拒絕舊模型檔案；尚無自動遷移。

當時工作區另有暫停中的 app 接線、RCS 指令艙／craft constructor 及測試草稿，未完成編譯與組合驗證，不能當作可驗收版本。本次整理只修改文件，不恢復這些工作；上述本地核心合併尚未 push。

下列命令及「已驗收主遊戲基線」表保留既有入口及 `41da390` 行為；最新預設 RCS 火箭／Full 氣動見 [本輪整合](main-flight-integration.md)。

```sh
cargo run -p void-app                        # 主遊戲：Aurelia、預設 flight-rocket
cargo run -p void-app -- --craft my.json     # 用 assembly-lab 匯出的船
cargo run -p void-assembly-lab               # 組船
```

## 多天體與零件狀態 lab

多天體任務已建立同世界 Aurelia／Selene 地形與兩地 Fleet 船的獨立 lab；世界描述、InitialWorld、存讀檔與錄放已共用多天體配置。可執行 `cargo run -p void-app --example multi_body`；操作、數值檢查與 renderer 限制見 [multi-body-scenes.md](multi-body-scenes.md)。已依使用者授權審查後合併；該 lab 保留；本輪已在主遊戲啟用十天體 scenery，見上方新整合。

## 已驗收主遊戲基線（41da390）

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

## 驗證證據與範圍

- 人類驗收：既有主遊戲基線和 A／B 內容已完成；本輪使用者操作對接場景後認可並授權合併，其他操作另有 root smoke，詳見本輪整合文件。
- 分支驗證：RCS／氣動各自的 core、lab、存讀及 journal 已有針對性測試／lint 和 agent TigerVNC 證據，見 [rcs-docking.md](rcs-docking.md)、[aero-wrenches.md](aero-wrenches.md)。分支檢查不等於合併組合檢查。
- model 17 主遊戲整合：受影響範圍 50 個不同 core 測試、24 個 app library 測試、7 個 assembly graph 測試通過；相關 Clippy、fmt 與 root GUI journal／save verify 通過。合併後核對結果見本輪整合文件；未跑全 workspace。
- 測試：在 `a730519` 記錄的 `cargo test --workspace --all-targets` 為 283 passed、0 failed、4 ignored。ignored 包括一項需要 GPU 的 opt-in 測試，以及已結案為非缺陷的 Pebble 靜止傾角（量到的是場地坡度，見 [vessels.md](vessels.md)）。本頁的文件修改沒有重跑測試。
- GPU：只在使用者本機與 RTX 5060 Laptop／Vulkan 離屏 benchmark 上跑過，其他 GPU／平台沒有測過。

## 已完成的架構重構

- **座標樹統一已完成並合入 master**：所有座標走同一棵 `void-frames` 樹，從銀河、恆星系、天體、地面場景到船；相機掛在焦點下，零件、碰撞線與 tile 直接轉到相機，不再繞經 1 AU 的質心系。使用者已完成主遊戲、multiscale example／lab 的視窗驗收。審查修正了 plain lab 天體焦點的 NaN、探測器焦點的 split 精度，並把模型版本升到 5。修正後相關 40 項測試、受影響範圍 clippy 與 fmt 通過；未重跑全量。見 [frame-tree.md](frame-tree.md)、[frames.md](frames.md)。
- **環境介面已完成並合入 master**：重力共用 `void_orbit::gravity`；`void-environment` 在座標樹上查重力、大氣、地形與海深。Fleet、舊火箭與再入 lab 共用環境取樣，積分器不再線性外推天體中心。layered 大氣與散射天空從海平面起算，水的物理仍待開發。審查補上環境／星曆世界描述核對，相機固定追蹤 craft root（主遊戲為上面級指令艙），避免分離時因質心切換而跳動；該次合併的 `MODEL_VERSION` 為 7。使用者已完成本輪視窗驗收。整合後 42 項針對性測試通過，相機修改後另有 19 項相機／存檔／錄放檢查通過，相關 Clippy、fmt 與主遊戲／multiscale example 編譯檢查通過；未重跑全量。見 [environment.md](environment.md)。
- **零件圖整合已完成並合入 master**：`void_assembly::PartGraph` 是 Fleet 唯一零件紀錄，燃料、分級、點火、pose 在零件上；每艘船為連通分量加物理擁有者，每個零件有座標樹節點，噴嘴面積進 catalog。審查補上還原連通性、PartGraph 基本狀態驗證與受控修改接口，跳過未改變父節點的 reparent。整合保留世界核對與上面級相機，`MODEL_VERSION` 8。使用者完成視窗驗收並同意合併；F6 存檔已加入緩衝寫入，修正大量小型檔案寫入造成的卡住。見 [part-graph.md](part-graph.md)。
- **零件模組整合已完成並合入 master**：引擎背壓與零件阻力集中到 `void-modules`，Fleet 直接讀環境，移除外掛力接線；修正非法氣壓掩蓋與軌道段累積時鐘誤差，整合版模型 9。保留前幾輪審查修正與 F6 緩衝存檔。使用者已完成本輪視窗驗收並同意合併；101 項針對性測試、相關 lint、編譯檢查與 fmt 通過，未重跑全量測試。見 [part-modules.md](part-modules.md)。

## 待續工作

- **RCS／氣動主遊戲接線**：本輪已合入；對接埠外觀、操作手感及有限 RCS 驅動 SAS 可另開後續工作，不混入這輪。
- **Scenery**：第一輪基礎已採入主遊戲；原始 model 14 lab 已封存至 `archive/solar-scenery-model14`（`e1f13f3`）並推送，原 worktree 已移除；逐顆細緻美術仍待安排。
- 後續新功能按現行 branch／worktree 主遊戲流程開發。其他需求不因列在 NOTE 就自動啟動。

## 對照 NOTE.md 的願望清單

| # | 項目 | 狀態 |
| --- | --- | --- |
| 1 | 零件組裝 | 完成：編輯器 lab＋主遊戲讀取。限制：仍以堆疊接點為主，沒有表面接合、對稱或結構破壞；核心已支持 typed 多資源 |
| 2 | 軌道機動、N 體 | 完成：N 體星曆、有限燃燒計畫、逐船機動與導引 |
| 3 | 軌道／飛行視角切換 | 完成：map 淡入、多天體 map、標籤焦點、四種 plot frame（本輪已驗收） |
| 4 | profiling、GPU、SIMD、多執行緒 | profiling 完成（CPU／GPU）；效能優化本身尚未系統性進行 |
| 5 | scenery | 大氣、海、體積雲、星空、分層地形已有。雲的移動與雲影、極光、天氣、植被、生物群系配色未做 |
| 6 | 火箭／飛機零件 | 指令艙、油箱、引擎、分離器、著陸腿已有。降落傘 core／獨立 lab 已完成，主遊戲預設 craft 尚無傘；Fleet 核心新增可配置固定翼模組；完整飛機零件與組裝流程仍未完成 |
| 7 | 空氣動力、燒蝕、熱 | aero-lab 完整（力矩、翼面、熱、燒蝕、再入）。Fleet 完整力矩／翼面及主遊戲氣動接線已合入；熱／傳熱／有限燒蝕與防熱盾已接入 Fleet／主遊戲（本輪已驗收），無結構碎裂 |
| 8 | 交會、對接 | 交會基線完成；捕獲／解除核心和主遊戲操作已合入 |
| 9 | 多船 | 完成 |
| 10 | SAS、旋轉、RCS | SAS 穩定／鎖定姿態完成。順行等進階模式未做；理想機動導引仍直接指定姿態；有限 RCS 核心和主遊戲操作已合入 |
| 11 | 存檔 | 完成：直接世界存檔＋錄放。沒有跨模型版本的存檔遷移 |
| 12 | 參考框架切換 | 完成：frames 樹、orbit-lab 四種繪圖框架、multiscale 的跨星系換框架；已統一到同一棵樹並完成視窗驗收；四種主遊戲 plot frame 本輪接入並已驗收 |
| 13 | 其他天體的程序地形 | 多天體 lab 已支持同世界多個可著陸天體；主遊戲已接第一輪十天體外觀／五個固體地形（已驗收），非最終美術 |
| 14 | 水上漂浮 | 未做：環境介面已提供海平面與深度（`Surroundings::sea`），浮力、水阻力、濺落等水的物理之後做 |
| 15 | UI | 主遊戲原生 HUD 與互動面板已接入；完整設定／組裝／流程 UI 未完成 |
| 16 | 太空人 EVA | 未做 |
| 17 | 車輛 | 未做 |
| 18 | multiscale | 核心與 lab 完成，並在接縫檢查中與 Fleet 共用；未接入主遊戲 |
| 19 | 相對論 | 未做 |
| 20 | 更多引擎類型 | 未做：catalog 只有四個液體引擎定義（原 lab 大／小，加主遊戲上級／助推級），沒有固體、離子等類型 |

其他未做：撞擊毀損（舊主遊戲也沒有啟用）、音效、科技樹、主線。

## 已知缺口與限制

- 主遊戲已有曝光與天體視角操作；完整 scenery 開關／調參面板仍僅 example 有。
- 噴嘴面積已是 catalog 的正式欄位（`nozzleExitAreaM2`）；新增引擎需在定義中提供，沒有額外的 engine ID 對照表。
- 搬遷時列出、仍未補的觀察工具：`lod` example 的 preset 選擇、vessels-lab 的歷史軌跡、multiscale encounter 的任意天體／船選取。詳見 [port-audit.md](port-audit.md)（歷史頁）。
- 存檔與錄影綁定模型版本、catalog 與 Rapier 版本，版本不同時直接拒絕，沒有遷移。

## 歷史驗證：零件狀態／資源與多天體（model 13）

`claude/part-state-resources` 把 Fleet 的 live 零件可變資料統一為 typed resource map、按穩定 module ID 尋址的 state/stage map，新增雙資源供應與 force-only 降落傘。Craft 2／FleetCheckpoint 4／整合 model 13 明確區分基線；舊 craft 可用顯式離線工具轉換。主遊戲預設火箭未新增傘。獨立 `void-part-state-lab` 與驗收操作、限制見 [part-state-resources.md](part-state-resources.md)。整合版 world schema 2 / model 13，分支暫用的 10 / 12 明確拒絕。

多天體後續追加：多顆分離大氣 HDR 合成、自訂散射／雲配置與 solar scenery 交接規格已實作；world schema 2 / 整合 model 13。該次只合併開發入口；其後 Solar 第一輪在分支完成、未合入，見 [specs/solar-scenery.md](specs/solar-scenery.md)。

合併核對（2026-10-05）：受影響核心／場景 **245 passed、0 failed**；主遊戲原有 offscreen GPU 重建／存檔測試另跑 **1 passed**。唯一仍跳過的行為測試是已結案的 Pebble 傾角；沒有重跑全 workspace。所屬 crates all-targets Clippy `-D warnings`、fmt、主遊戲與兩 lab 編譯通過。新增第二天體降落傘 checkpoint／續跑／錄放，以及分支舊模型拒絕回歸。

合併 commit 後再次以 TigerVNC 核對：降落傘 deploy／全開／F6-F7 續跑，雙大氣切船／月球軌道／存讀；兩份實際操作錄影的 merged-model headless verify 與多天體 checkpoint verify 通過，未見 panic／shader validation 錯誤。

## Git 與歷史資料整理（2026-10-07）

主工作區只保留 `void-bevy`／master；已合併的開發分支與 worktree 已清理。
組合飛行測試已在 master，舊 flight-checks 工作區已移除。Scenery 原始成果保留於
上述封存分支，不混入目前主線。既有驗證錄影／存檔搬至
`/home/pekka/Archives/VOID/2026-10-05/review-recordings/`，同層 README.txt
記錄用途與版本限制，SHA-256 摘要核對搬移前後一致。

## 四功能 model26 整合版的歷史核對（2026-10-08）

`work/four-feature-integration`／`void-bevy-integration`，實際程式來源 `c7c638b`。
四功能分支的實作已完成，root審查後接入同一主遊戲。以下為合併前model26
核對紀錄；目前master已包含此整合版與model27水修正，見頁首合併狀態。
啟動入口、操作、重新編譯及限制見 [四功能驗收](next-features-acceptance.md)。

| 範圍 | 核心與主遊戲 | root 審查的主要接縫 |
| --- | --- | --- |
| EVA／rover | 有乘員座位、出入座、步行／跳躍、有限背包、輪子／懸吊／驅動／煞車 | 隔離燃料與乘員質量、COM/P/L、遠端 Ground/Orbit 出入座、dry accepted cadence、sleep/wake、輪胎 reciprocal impulse、accepted steering、Orbit rotor momentum／rails |
| 飛機／assembly | 共用 PartGraph、cuboid 幾何／慣量／外觀、surface mount／mirror、翼面／jet、共享起落架 | native 接觸 bookkeeping、低速 AIR 資訊、profile navball、世界向量的真實按鍵方向與滑行轉向 |
| 水 | 真實海柱部分浸水、排水浮力、偏心力矩／水阻、濺落 | sea presence 與岸上拒算、Scene/Orbit force frame、accepted water substeps、ForceOnly air 語義、dry cadence |
| 三星系 | 共用世界 Sol/Beryl/Cygnus、精確 split state、普通火箭、地圖／切船、存讀／錄放 | 持久 query frame／escaped Wrench、遠方小量、owner 交接與 reanchor、coupled checkpoint、地圖 root label/hitbox、日照真實地形 fixture |

相容性：model26／FleetCheckpoint12／world4／Craft3；明示保留未用新增幾何的
既有 Craft2。新狀態必要欄位與舊模型明確拒絕，不自動修補。native Rapier 為
`rapier3d-0.35.1/f32/v1`；公共物理與座標使用 f64／局部浮動原點，沒有宣稱 native f64。

Root headless：受影響九個核心 crates 的 lib/tests **345 passed、0 failed、1 ignored**；
ignored 是既有已結案 Pebble 傾角，沒有放寬門檻。app 最終 **28 passed**，其中兩項
直接檢查真實氣動力矩和 native 起落架的玩家方向。所屬核心與 app all-targets
Clippy `-D warnings`、fmt 通過；沒有跑全 workspace。

Root TigerVNC 初步檢查（RTX 5060 Laptop／Vulkan）：

- model26／`0be322f` 地面 EVA：F 出座，人物與實際 collider，W/D 移動、跳躍與
  接地，F6/F7 存讀、F 回座。太空有乘員火箭 F 出座，有限背包從 5 kg 減至約
  4.82 kg，相對運動，存讀與 F 回座。最後 `c7c638b` verifier 核對兩份真實錄影
  T+10.55 s／2.10 s 通過；回座的額外 GUI 觀察與錄影涵蓋範圍分開記錄。
- model26／`0be322f` 水：日侧真实海洋濺落，艙體持續部分浸水，T+35.45 s
  顯示約 0.2 m/s，F6/F7 存讀。最後執行檔 verifier 通過；AGL 是海底地形距離。
- model26／`e0b659a` 星系：16 ly 三星標籤，切 Sol/Beryl/Cygnus，Beryl 普通火箭
  升至 AGL 73 m、Cygnus RCS 耗用至 19.911 kg。錄影只涵蓋初段 T+1.60 s；
  後續升空／RCS 是額外 GUI 觀察，不冒稱都在该錄影內。最終執行檔 verify 通過。
- model26／`c7c638b` rover：低速1.7m/s、D右轉heading000→013、四輪支撐，
  Space煞停至0.0m/s、F6/F7，錄影T+6.70s verify通過。另一段約13m/s全幅急轉
  抬輪／翻覆，chassis碰撞正常、無自動扶正；T+97.50s錄影也保留並verify，
  不把高速翻覆樣本說成四輪穩定煞停。
- model26／`c7c638b` 飛機：50.4 m/s 滑行、W 抬頭約8°、0/3支撐並升至
  AGL4.7m；切油門下降、拉平、輪子接地、B煞停至0.0m/s及3/3支撐，F6/F7。
  同執行檔錄影 T+70.15 s verify 通過。沒有結構破壞／任意組装適航認證。

EVA／車輪／水／星系的 production 內容在上述 GUI 版本後未再修改；water test module
後移以通過 all-targets lint，非 test token 相同。最後兩次
app 修正是飛機方向映射與純觀察 GPU 截圖，最終二進位另核對真實錄影。
Root GUI 截圖不代替人類最終驗收。TigerVNC 曾有間歇遮擋矩形，重繪後消失；
GPU 截圖診斷加入後的配對樣本正常，未取得異常同幀的 GPU 證據，未宣稱根因已修復。

驗證檔保存於 `/home/pekka/Archives/VOID/2026-10-08/four-features-review/`；
原開發暫存位於 `/tmp/void-next-feature-evidence/`。驗收二進位及 SHA256/source manifest
位於整合工作區 `target/acceptance/`，各功能 worktree 的同目錄有明確指向組合版的入口，
不偽裝成各自分支已驗證的二進位。詳細需求核對見 [root review](next-features-review.md)。
