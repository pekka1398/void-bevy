# 效能下一階段：導航延遲、資源界線與 LOD 接線

2026-10-10；task4 延續。這是後續實作規格，**不是已完成清單**。
共用流程／架構遵守 AGENTS.md。依據 `nbody-experiments.md`、LOD 分支的
`lod-review.md`（最終成果 `f2fcaea`）；各分支尚未合入 master，不以單分支數字推論整合 FPS。

## 採用順序

1. 保留 f64、既有 Yoshida8 步長、Kahan、Hermite 與 deterministic 累加順序。
   預設 target-row SIMD＋RK step 內相同時間取樣重用。已存在的 LOD CPU／storage
   優化另按其分支驗收，不在本分支混入不同 model 的主遊戲。
2. 下一個實作交付是主遊戲導航背景工作、取消、過期結果與記憶體預算。
   最快 solver 仍要秒級，單改 kernel 無法達成流暢介面。
3. 在同一個實際遊戲場景量 LOD 與導航競爭後，再選導航 worker 數。
   2／4 workers 為候選，Auto 暫不改多執行緒。禁止每個 candidate 再開一組
   積分 workers，禁止把所有硬體 threads 同時分給 LOD 和導航。
4. 候選驗證平行化、共享只讀星曆是再下一步；GPU packing 已在 LOD 分支接入主遊戲作 opt-in，完整 pipeline 未有穩定收益。
   IAS15 是模型版本研究，不混入上述保留行為的優化。

## 導航背景工作接口與 ownership

涉及 `orbit::solve_navigation`、`fleet-flight::plans::generate_navigation`、
app/session commands、journal。現有 Fleet 呼叫除了 solver，還有同步
`tail_state`／`candidate.extend(..., 2_000_000)`；只把 solver 放背景仍可能卡幀。

Core 提供可中止的求解 context，包含 cancellation、確定性 work counters、
預算與進度。現有同步 API 保留供 golden／headless 使用。
Fleet 負責建立一次不可變請求快照與驗證完成結果；app 負責 job handle、
進度／取消 UI 和主執行緒上的結果接收。不得讓 worker 借用可變 live Fleet。

快照包括 model／system catalog 身分、f64 source origin、ephemeris 的 q/v/Kahan
與確切步索引、船 ID／PartGraph revision、計畫 revision、engine／tolerances、
anchor time/state、request。不能只由插值位置重新起算星曆（最低 bits 不同），
也不能在主幀 clone 已累積數百 MB 的歷史。以同一套 Ephemeris 實作建立獨立
prediction 工作資料；coupled systems 必須保留跨系 force／frame 契約，沒有
相容 snapshot 時明確拒絕，不能暗換單星系模型。

每次新請求產生 generation；先取消舊請求，只保留一個執行中和最新待執行請求。
取消不在 UI thread join。完成時核對世界 generation、船／計畫 revision、anchor
與引擎；世界載入、船移除、owner／來源系統改變、計畫編輯後的舊結果丟棄並顯示原因。
飛行時間已前進不能直接套用舊 anchor；先以暫停計畫操作為明確初版契約，或另做
可驗證的 rebase，不默默接受。只在有效結果提交時改計畫；不耗真實燃料、不點火。

Journal 不能記錄取決於 thread 完成時刻的隱性世界修改。需明確的「提交已驗證計畫」
command／結果內容與 hash，保存 anchor 和版本；replay 不依賴 worker 排程。
若現有 command schema 不足，更新相應版本並明確拒絕不相容資料。

## 記憶體與取消

58 體目前約 105.24 秒一個 sample、每 body 的 q/v/a 共72 bytes，僅 sample payload
每天約 3.43 MB（還有 chunk 配置）；37 天實際 retained 約 128 MB。
多年區間不能無界預配置，也不能把每份候選複製一整份星曆。

初版建議全導航 scratch 上限 256 MiB、同時一個 job；此為待實測的產品預算，
不是目前已強制的限制。估算需用實際 chunk capacity，加上 snapshot、候選軌跡、
worker buffers；配置前 checked arithmetic，超限回傳 BudgetExceeded 並顯示
所需／允許大小。不得偷偷縮短窗口、放大步長或改低精度。

在每批 ephemeris steps、每批 vessel accepted/rejected steps、各候選邊界检查取消；
初始 batch 上限 64 星曆步，再用最慢支援場景驗證取消 p95 <100 ms。
step/bytes budgets 決定可重現的拒絕；wall deadline 只作使用者取消／超時狀態，
不讓計時競賽決定哪個解勝出。進度資訊分星曆／候選／計畫驗證。

後續才考慮 immutable shared chunks＋稀疏 checkpoint 重算；所有歷史回收須知道
最早 live query，不能在仍會回頭驗證候選時直接 forget。改插值或降採樣另屬模型研究。

## 平行候選與硬體方案

先串行建立有預算的共享只讀星曆；獨立 finite-thrust candidate 使用各自 propagator，
固定 candidate ID／排序／tie-break，合併依原序。不能 first-finished-wins。
選候選平行或星曆內平行其中一種，總 worker credit 共用並預留 renderer／主幀容量。

LOD：保留 CPU f64 sampler、碰撞契約與已驗證的 render-only／U16 優化。
增加 workers 要量冷啟動、快速低空、跨天體切換、導航同時運作的 p50/p95/p99。
LOD 最終分支已實作直接寫 renderer-owned slabs、render-thread ordering、批次 dispatch，
並測過動態相機與快速天體切換的 ownership；GPU kernel 更快但整體主幀沒有穩定改善，
因此保留 opt-in。後續重點是接縫 asset churn 及與導航的資源競爭，不能把已完成接線
重列為待實作，也不能以這些 headless 場景代替人類驗收。

58 體 GPU f64 probe 已輸 CPU，暫不投入 production physics GPU；不把結論泛化到大 N。
IAS15 保留研究：對多個獨立收斂參考、長期 parent-relative phase、dense output、
burn／impact／AN-DN 事件與 replay 做 time/error Pareto；30 天 h/8 vs h/16 已差 15 cm，
不能以接近該參考就宣稱真誤差只有幾公分。研究通過後再提出版本變更。

## 完成條件與驗收

- 核心：原 scalar／golden 門檻不變；同步與背景完整結果一致；所有 trial 不改 live state。
- 非同步：取消、連續請求、載入、切船、刪船、改計畫、owner／frame 改變、離開 app；
  舊結果不提交，thread 不洩漏，超預算在配置前拒絕。
- Replay：不同 worker 數／排程提交相同 plan，checkpoint/journal 往返一致。
- 主遊戲：58 體預設月球導航、最長窗口拒絕、導航同時低空飛行／轉視角；量測
  enqueue／snapshot／poll／commit 主幀成本（各自目標 <2 ms）、p95/p99 幀時間、
  cancel latency、RSS、scratch peak、LOD settle；這些是驗收目標，非已達成數字。
- 人類實際操作確認介面持續回應、取消有效、軌跡與機動正確後才算遊玩驗收。
  每輪只跑受影響 crate／接縫測試；整合後另安排組合場景，不自動跑全 workspace。


## Principia 調查後補充（2026-10-10）

詳細方法、固定原始碼來源與 VOID 差異見 [Principia 方法調查](../principia-methods-review.md)。
先分離天體插值／船積分／繪圖誤差；保留天體固定步與船自適應解耦。星際還須處理
Fleet 外層短 chunk 限制。生命週期分 active leases、精確 checkpoint、可重建 coast
與不可重建外力段；多項式壓縮、歷史降採樣、存檔壓縮各自驗證。
256 MiB 仍是待實測預算，不能以少算 acceleration 的舊估值定案。
