# RCS／對接核心與主遊戲操作

更新：2026-10-05。原規格以 `41da390` 為基線，核心已由 `4bd5322` 合入 master；主遊戲接線已於 `134bb37` 合入。組合版本為 model 17／FleetCheckpoint 7；完成狀態與驗證範圍見 [status.md](../status.md)。

本檔保留該輪功能契約與原分工，供審查及後續接線參考。共用開發、測試、Git 和 GUI 規則只引用 [AGENTS.md](../../AGENTS.md)。原輪次的 lab 交付不代表後續每項功能都要另建 lab；新任務需列明主遊戲行為與完成條件。整理前的完整任務指令見 [歷史任務規格](../history/task-specs-before-2026-10-05.md)。

## 功能契約與原輪次範圍

建立可配置、按穩定 module ID 尋址的 RCS 噴嘴/對接埠，消耗現有 Monopropellant 資源。噴嘴提供局部方向、施力點、推力/Isp，混控要求平移與旋轉；有限且可解釋的分配算法，不憑空產生控制力，不以理想 steering 代替 RCS。用 PartGraph 共用供油語義，處理無燃料/飽和/非對稱/多資源。SAS 與 RCS 職責清楚。
對接是實際捕獲流程：距離、埠方向、相對速度、旋轉條件，合法可用埠、防自接/重接；捕獲合併 graph/owner 時保持零件身份、世界 pose、質量/動量。解除對接與可配置小分離衝量，存檔/錄放完整保存埠和控制狀態。既有 debug join 不能冒充對接。
原輪次的獨立 lab 提供近距離兩船、平移/轉向/RCS切換、對接/解除、存讀/錄放與實際噴嘴狀態 HUD。headless 驗證分配、供油、捕獲邊界、旋轉與相對速度、多船 owner/pose/動量及續跑。不要要求使用者每次從發射台重玩。
原任務分工（已結束）：RCS agent 擁有 RCS/docking 模組/catalog/純核心/捕獲 graph 與 lab 控制。氣動 agent 擁有共用 wrench 與 Fleet 積分；先用既有 EngineForce/Propulsion 類似語義（f64 force、point、COM torque），共享 Fleet 修改協調，不自行寫第二個物理迴圈。檢查 git 是否有未合入舊 RCS 成果，若有評估重用但不得盲目 cherry-pick。

