# 完整氣動施力與力矩本輪實作

共同基線：master 41da390（MODEL_VERSION 13）。使用者已授權本輪 branch/worktree 與三個 agent。先讀 AGENTS.md、NOTE.md、docs/status.md，歷史完成狀態以程式為準。
只改自己的 worktree。不 commit/push/merge；交付可審查的 diff、設計理由、驗證結果和視窗驗收操作。不得加入 fallback 或放寬既有門檻。不跑全 workspace test/lint；只跑受影響 crates，編譯限制 -j 2，避免多份 Bevy 同時大量連結。GUI 使用 TigerVNC，禁用 xdotool；不要用 pgrep/pkill 字串比對，僅 numeric PID。GUI 與昂貴驗證開始前告知 root 以協調資源。
保持主遊戲預設行為；新增能力在獨立 core + lab 驗證。必要格式變更明確拒絕舊版，不冒稱相容。不要改用第二套零件、資源或座標模型。
先向 root 報告實作設計、涉及的共享接口和風險，再繼續實作；遇到跨任務接口修改先協調。不要因任務大就停在計畫，完成具體可驗收的一輪。

把現有 aero 核心的力/力矩接入 PartGraph/Fleet，取代 production force-only 限制。在 modules 層建立清楚的 wrench（力、關於明確參考點的力矩）契約；engine/air/chute/RCS 能共同遵循，座標、COM、作用點一律 f64 且明確，不重複加 r×F。
氣流速度包含剛體角速度與作用點偏移；air/chute 的力與力矩須進入 bubble/contact 及 orbit 的姿態/平移積分，姿態变化不能整段使用過期的 frozen attitude 而假稱完整耦合。環境 trial evaluation 純函式；耗油、降落傘等狀態只在接受步提交。質量/慣量動態變更、rails 不適用條件、睡眠地面不被被動空氣喚醒要保住。
用既有 aero 係數/元素接入可配置翼面或穩定翼示例，展示風標效應/氣動阻尼與偏心降落傘力矩；不做熱/燒蝕/破壞。本輪先形成完整力矩通路和可驗收 flight lab，不替換主遊戲預設。headless 驗證零空氣極限、對稱零矩、偏心矩、旋轉局部風、耗散適用條件、不同 dt 收斂與 owner 接縫、存讀/錄放。
擁有 modules::air、wrench契約、Fleet 共用施力/姿態積分與 aero lab。與 RCS agent 協調 assembly model/catalog 與 Fleet interfaces，對外接口先報 root；保持 scenery world optics 不受影響。

