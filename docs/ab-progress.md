# A／B 完成清單

使用者目標：完成 A（assembly＋vessels 整合主遊戲與飛行世界存檔）和 B（profiling、碰撞疊圖、record/replay、差分／不變量與必要觀察接口）；視窗驗收由使用者之後補做，不以等待 GUI 驗收中斷實作。此要求允許在 headless 檢查後繼續原規則中的整合階段。仍不由 agent 開啟 GUI。對接／RCS、EVA、車輛、視覺精修和效能優化不是這兩項基礎工作的替代品。

| 要求 | 目前證據 | 尚缺 |
| --- | --- | --- |
| A：assembly 船在 Fleet 多船世界飛行 | fleet-flight core／lab、自訂 craft、分級、切船、SAS、air、owner／warp 測試 | 主遊戲仍是 PartJointRocket，尚未完成替換 |
| A：保留主遊戲能力 | docs/fleet-flight.md 的缺口盤點 | live PlanEngine／trait-based FlightPlan 已接；仍缺機動執行、navball／map／scenery、撞擊毀損、逐船所有權及 UI 兩級假設的改接 |
| A：保存完整飛行世界並續玩 | Fleet command journal 保存／載入、atomic file write、同程序與獨立程序重播、pending／SAS／多船測試 | 直接語意 snapshot／快速載入與主遊戲存讀入口 |
| B：操作可重現且會檢查回歸 | 舊主遊戲 input／session；Fleet Action journal、完整 world mark、incremental playback、--verify | Fleet 相機／純視覺操作；崩潰前逐條持久化；主遊戲換 Fleet 後的同一條錄放路徑 |
| B：profiling 量測與可分析輸出 | 舊主遊戲 LabLog；void-diagnostics p50／p95、CPU 系統 trace、Fleet lab --profile 與 --verify --profile、native perf wrapper | GPU／draw-call 與 rendered benchmark；main 接線；本機 native perf 因 paranoid=4 未能實測 |
| B：碰撞體與地形疊圖 | 主遊戲及 landing F4 真實 collider 線；Fleet lab 的地形碰撞線 | Fleet 零件真實 collider 疊圖；整合後不能退化 |
| B：接縫差分與不變量 | landing/seams：frame／origin／rails／handoff；assembly 分離掃描；Fleet flight 三 owner 阻力與步長收斂 | Fleet／multiscale 交接與合併／分離掃描、保存失敗案例與重跑入口，依目前測試實際涵蓋確認 |

本表以 current source／command output 為準；不以「有 lab」或「有測試」直接宣稱整項完成。每個缺口完成時更新對應證據，最後再逐項核對 A／B 全部要求。
