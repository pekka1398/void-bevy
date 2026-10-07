# Scenery 本輪實作

共同基線：master 41da390（MODEL_VERSION 13）。使用者已授權本輪 branch/worktree 與三個 agent。先讀 AGENTS.md、NOTE.md、docs/status.md，歷史完成狀態以程式為準。
只改自己的 worktree。不 commit/push/merge；交付可審查的 diff、設計理由、驗證結果和視窗驗收操作。不得加入 fallback 或放寬既有門檻。不跑全 workspace test/lint；只跑受影響 crates，編譯限制 -j 2，避免多份 Bevy 同時大量連結。GUI 使用 TigerVNC，禁用 xdotool；不要用 pgrep/pkill 字串比對，僅 numeric PID。GUI 與昂貴驗證開始前告知 root 以協調資源。
保持主遊戲預設行為；新增能力在獨立 core + lab 驗證。必要格式變更明確拒絕舊版，不冒稱相容。不要改用第二套零件、資源或座標模型。
先向 root 報告實作設計、涉及的共享接口和風險，再繼續實作；遇到跨任務接口修改先協調。不要因任務大就停在計畫，完成具體可驗收的一輪。

以 solar-scenery.md 為正式範圍，實作 solar scenery lab 任意 body ID、近景/軌道/遠景預設，太陽、九主要天體與月球的首輪可辨識外觀。Earth 保留既有能力；岩石地形/撞擊坑、Vesper 光學與雲、巨行星雲帶、Halo 環、Sol 發光各用合理 renderer recipe，不製造假 terrain/collider。每顆能獨立展示、配置、保存；不以單純 tint 宣稱專屬地形完成。先完善可擴展 recipe，再逐顆完成。
物理大氣與光學分離；本輪以 visual/terrain 為主，不改 Fleet 施力。LUT 極端/真空/NaN、solid 繪圖與碰撞一致、配置往返、切焦點/重建資產上限需驗證。視窗檢查要實際 TigerVNC 截圖，不只 headless。若未完成所有細節，明確列出完成天體與限制。擁有 terrain/scenery/world visual configs/app solar preview；不修改 RCS、module resource 和 Fleet 積分。

