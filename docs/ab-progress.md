# A／B 完成清單

使用者目標：完成 A（assembly＋vessels 整合主遊戲與飛行世界存檔）和 B（profiling、碰撞疊圖、record/replay、差分／不變量與必要觀察接口）；視窗驗收由使用者之後補做，不以等待 GUI 驗收中斷實作。此要求允許在 headless 檢查後繼續原規則中的整合階段。仍不由 agent 開啟 GUI。對接／RCS、EVA、車輛、視覺精修和效能優化不是這兩項基礎工作的替代品。

| 要求 | 目前證據 | 尚缺 |
| --- | --- | --- |
| A：assembly 船在 Fleet 多船世界飛行 | fleet-flight core／lab、自訂 craft、分級、切船、SAS、air、owner／warp 測試 | 主遊戲已改用共享 Fleet runtime；舊 PartJointRocket 場景保留為 legacy_flight 回歸 example |
| A：保留主遊戲能力 | docs/fleet-flight.md 的缺口盤點 | main／lab 共用存讀、錄放、任意零件繪圖及逐船控制；main 接回 navball／多天體 map／scenery；live PlanEngine 已接。逐船計畫保存／機動執行已接入；warp-to-maneuver 已接入保存／重播與攔截；舊 PartJointRocket 主遊戲未啟用 crash_detection（預設 false，legacy_flight 無啟用入口）；撞擊毀損屬後續新增功能，不列為整合遺失 |
| A：保存完整飛行世界並續玩 | 直接 Fleet checkpoint（graph＋native owner caches）、atomic file write、跨程序驗證、載入後完整狀態續玩對照；journal 錄放可從 checkpoint 開始 | 逐船計畫／active burn 已加入直接 checkpoint 與 journal；持續核對其餘主遊戲能力 |
| B：操作可重現且會檢查回歸 | 舊主遊戲 input／session；Fleet Action journal、完整 world mark、incremental playback、--verify | Intent／Commit／Mark stream 已逐條 sync，含獨立程序崩潰與恢復入口；ResetWorld／LoadWorld 仍留在同一 journal。Fleet 相機／觀察操作與暫停逐幀錄放已接入 checkpoint／mark；core 及 renderer-free 視窗控制檢查，GUI 驗收之後補做 |
| B：profiling 量測與可分析輸出 | 舊主遊戲 LabLog；void-diagnostics p50／p95、CPU 系統 trace、Fleet lab --profile 與 --verify --profile、native perf wrapper | GPU／draw-call 與 rendered benchmark；main 已接共享 CPU profile；本機 native perf 因 paranoid=4 未能實測 |
| B：碰撞體與地形疊圖 | 主遊戲及 landing F4 真實 collider 線；Fleet lab 地形與船體均讀回 native collider；形狀／local transform 故意變更的觀察測試 | 主遊戲／lab 現在共用此疊圖；GUI 驗收由使用者之後補做 |
| B：接縫差分與不變量 | landing/seams：frame／origin／rails／handoff；assembly 分離掃描；Fleet flight 三 owner 阻力與步長收斂 | Fleet／multiscale 交接與合併／分離掃描、保存失敗案例與重跑入口，依目前測試實際涵蓋確認 |

本表以 current source／command output 為準；不以「有 lab」或「有測試」直接宣稱整項完成。每個缺口完成時更新對應證據，最後再逐項核對 A／B 全部要求。

撞擊能力盤點修正：`landing/src/rocket.rs` 提供 opt-in `crash_detection`，但舊主遊戲 `app/examples/legacy_flight.rs` 從未將其設為 true；demo 的上／下級 tolerance 不會自行啟用它。Fleet 未提供此 opt-in 擴充，但 A 的「保住既有主遊戲行為」不以新增毀損系統為前置。
