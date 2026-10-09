# 導航效能實作：可取消預測、共享星曆、視角曲線

基底 `f55056a`，工作區 `void-bevy-navigation-performance`／分支
`work/navigation-performance`。共用規則依 AGENTS.md；本輪為使用者授權的研究後實作。

## 決策與範圍

1. 天體／船解耦、f64、Yoshida8、既有步長與 Hermite 保持；預設 rows SIMD 不改多 worker。
2. 用 immutable Arc/COW history 建立精確 prediction snapshot，包含 celestial 最新積分
   狀態與補償，不能從插值值重建初值。單星系與 coupled source 均需正確，沒有暗換模型。
3. 背景工作涵蓋 candidate 前置、tail、導航、追加計畫與驗證；每次工作的 ephemeris
   steps、vessel accepted/rejected trials、allocation bytes 都受明示預算／取消約束。
4. 初版只允許暫停時生成，畫面、鏡頭與介面仍運作；不自行暫停。取消／新請求／世界或
   船計畫改變使舊結果無效；只允許一個 running job 及最新 queued request。
5. 成功計畫所需的 future ephemeris 也必須一起交付給主遊戲讀者，否則繪圖會卡住或越界。
   暫停且 source/plan/anchor 一致時接收背景精確 source；正常 UI 不再同步補算。
   durable commit 記錄 prepared plan 與必要 coverage；replay 可重建天體 coverage，
   不重新搜尋導航。接受後保留的星曆是計畫需要的資料；取消結果不污染 live source。
6. 畫面曲線由 f64 相對幾何誤差導向取樣，保留 knot/端點及有限點數狀態，不改物理。
   初版 camera focus 的米/像素是明示近似，不能宣稱任意透視深度的嚴格 pixel bound。

選擇原因：目前主問題是秒級同步等待、長窗口無界樣本，以及畫面取樣與積分綁定。
GPU／高階壓縮／換積分器尚無完整遊戲收益證據，不為同一輪同時改數值模型。

## 所有權與分工

- ephemeris_jobs：orbit（flight_plan.rs 除外）、multiscale 的 snapshot／context／source adoption／測試。
- fleet_background：fleet-flight、app、orbit/flight_plan.rs 的 prepared plan／journal／UI／測試。
- plot_precision：view、app 中 map path call sites（先與 app owner 協調）。
- 主 agent：規格、資源調度、接口／diff 審查、整合驗證、驗收入口與結果文件。

所有 agents 在同一獨立 worktree 修改不重疊範圍；不自行 merge/push，不並行昂貴 link。
新 target 借 registry cache 後清除所有 local workspace packages，避免其他分支 artefact；
Rust -j2；重型編譯與 GUI 由主 agent 串行執行，數字 PID group 資源 guard。

## 預算與狀態契約

256 MiB 是本輪導航 reservation budget，含共享來源的邏輯保留與新配置；不是全 app RSS
上限。預算計數方式、峰值與可能保守累積需在交付文件明示；不得以此宣稱所有 RAM 有界。
超長窗口配置前 preflight，清楚報 required/limit。取消檢查覆蓋 rejected trials 與
星曆延伸，錯誤不轉成成功短解。當前 UI 可顯示 cancel/budget/stale 與基本工作計數。

## 驗證

- 原有 orbit goldens、逐位元 backend、Fleet plans／coupled／checkpoint/durable 接縫。
- snapshot query/continuation 等價、live 不修改、coupled split origins／Rc ownership 正確。
- 取消、超預算、重複請求、過期結果、paused-only、對接／owner／engine／plan 改變。
- 正常 UI 完成的主幀不重新求解或巨量複製；測 snapshot／poll／commit latency。
- 新舊同步完整導航 plan 對照；成功結果 frame plotting 具有完整 ephemeris coverage。
- 繪圖 witness：長總時間中短曲段；原 temporal grid 漏掉而新方法達目標；點数／深度
  上限、端點與 source frame／offset／zoom 失效核對。
- 新 durable command／格式若需要版本改變，明確拒絕旧版本；不改既有 golden 門檻。
- 主遊戲 build、agent GUI／headless 實際入口與人類驗收分列。未通過項目不寫完成。

本輪不宣稱已完成歷史壓縮、全域 lease／LRU、稀疏 checkpoint archive、星際大步調度，
或人類近地折線問題的精確重現。這些由研究報告的後續 gate 管理；本輪先交付完整可玩的
背景導航及其必要資料生命週期，不以 prototype 代替主遊戲接線。
