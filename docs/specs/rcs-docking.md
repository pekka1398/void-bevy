# RCS 與對接本輪實作

共同基線：master 41da390（MODEL_VERSION 13）。使用者已授權本輪 branch/worktree 與三個 agent。先讀 AGENTS.md、NOTE.md、docs/status.md，歷史完成狀態以程式為準。
只改自己的 worktree。不 commit/push/merge；交付可審查的 diff、設計理由、驗證結果和視窗驗收操作。不得加入 fallback 或放寬既有門檻。不跑全 workspace test/lint；只跑受影響 crates，編譯限制 -j 2，避免多份 Bevy 同時大量連結。GUI 使用 TigerVNC，禁用 xdotool；不要用 pgrep/pkill 字串比對，僅 numeric PID。GUI 與昂貴驗證開始前告知 root 以協調資源。
保持主遊戲預設行為；新增能力在獨立 core + lab 驗證。必要格式變更明確拒絕舊版，不冒稱相容。不要改用第二套零件、資源或座標模型。
先向 root 報告實作設計、涉及的共享接口和風險，再繼續實作；遇到跨任務接口修改先協調。不要因任務大就停在計畫，完成具體可驗收的一輪。

建立可配置、按穩定 module ID 尋址的 RCS 噴嘴/對接埠，消耗現有 Monopropellant 資源。噴嘴提供局部方向、施力點、推力/Isp，混控要求平移與旋轉；有限且可解釋的分配算法，不憑空產生控制力，不以理想 steering 代替 RCS。用 PartGraph 共用供油語義，處理無燃料/飽和/非對稱/多資源。SAS 與 RCS 職責清楚。
對接是實際捕獲流程：距離、埠方向、相對速度、旋轉條件，合法可用埠、防自接/重接；捕獲合併 graph/owner 時保持零件身份、世界 pose、質量/動量。解除對接與可配置小分離衝量，存檔/錄放完整保存埠和控制狀態。既有 debug join 不能冒充對接。
獨立 lab 提供近距離兩船、平移/轉向/RCS切換、對接/解除、存讀/錄放與實際噴嘴狀態 HUD。headless 驗證分配、供油、捕獲邊界、旋轉與相對速度、多船 owner/pose/動量及續跑。不要要求使用者每次從發射台重玩。
擁有 RCS/docking 模組/catalog/純核心/捕獲 graph 與 lab 控制。氣動 agent 擁有共用 wrench 與 Fleet 積分；先用既有 EngineForce/Propulsion 類似語義（f64 force、point、COM torque），共享 Fleet 修改協調，不自行寫第二個物理迴圈。檢查 git 是否有未合入舊 RCS 成果，若有評估重用但不得盲目 cherry-pick。

