# 完整氣動施力與力矩

更新：2026-10-05。原規格以 `41da390` 為基線，核心已由 `366f270` 合入 master；主遊戲接線已於 `134bb37` 合入。組合版本為 model 17／FleetCheckpoint 7；完成狀態與驗證範圍見 [status.md](../status.md)。

本檔保留該輪功能契約與原分工，供審查及後續接線參考。共用開發、測試、Git 和 GUI 規則只引用 [AGENTS.md](../../AGENTS.md)。原輪次的 lab 交付不代表後續每項功能都要另建 lab；新任務需列明主遊戲行為與完成條件。整理前的完整任務指令見 [歷史任務規格](../history/task-specs-before-2026-10-05.md)。

## 功能契約與原輪次範圍

把現有 aero 核心的力/力矩接入 PartGraph/Fleet，取代 production force-only 限制。在 modules 層建立清楚的 wrench（力、關於明確參考點的力矩）契約；engine/air/chute/RCS 能共同遵循，座標、COM、作用點一律 f64 且明確，不重複加 r×F。
氣流速度包含剛體角速度與作用點偏移；air/chute 的力與力矩須進入 bubble/contact 及 orbit 的姿態/平移積分，姿態变化不能整段使用過期的 frozen attitude 而假稱完整耦合。環境 trial evaluation 純函式；耗油、降落傘等狀態只在接受步提交。質量/慣量動態變更、rails 不適用條件、睡眠地面不被被動空氣喚醒要保住。
用既有 aero 係數/元素接入可配置翼面或穩定翼示例，展示風標效應/氣動阻尼與偏心降落傘力矩；不做熱/燒蝕/破壞。原輪次先形成完整力矩通路和可驗收 flight lab，當時不替換主遊戲預設。headless 驗證零空氣極限、對稱零矩、偏心矩、旋轉局部風、耗散適用條件、不同 dt 收斂與 owner 接縫、存讀/錄放。
原任務分工（已結束）：氣動 agent 擁有 modules::air、wrench契約、Fleet 共用施力/姿態積分與 aero lab。與 RCS agent 協調 assembly model/catalog 與 Fleet interfaces，對外接口先報 root；保持 scenery world optics 不受影響。

