# Principia 方法調查：積分、連續星曆、軌跡與生命週期

2026-10-10。VOID 對照版本：`work/nbody-profiling` 的 `9194dda`；LOD 結論另以
`work/lod-profiling` 的 `f2fcaea`／`docs/lod-review.md` 為準。
本次是官方原始碼與本地接線審查，沒有移植 Principia 程式碼、改數值模型、
執行 Principia benchmark，或宣稱已完成以下新方案。

## 來源與本地副本

既有 `/home/pekka/Desktop/void-bevy/vendor/Principia` 可供完整方法研究。
HEAD 為 `0feb271b24a2a0c9ab200e34766711265d220039`（2026-10-08）；
`git status --short` 乾淨、`git ls-files -d` 為零、connectivity fsck 通過。
它是 shallow clone，沒有啟用 sparse checkout；缺少舊 Git 歷史不代表缺少目前受追蹤原始碼。
未核對外部建置工具／依賴是否足以編譯，沒有修改此 vendor 副本。
先前另取得的 `/tmp/void-principia-review-20261010` 是相同 commit 的 sparse 參考副本。

官方 wiki 可解釋概念，但數值設定以 pinned code／系統 blueprint 為準。
例如舊 changelog 的 45 分鐘、wiki 的 10 分鐘不能當所有情境統一預設。

以下來源連到同一固定 commit，便於重查：

- [S1 預設積分器與容差](https://github.com/mockingbirdnest/Principia/blob/0feb271b24a2a0c9ab200e34766711265d220039/ksp_plugin/integrators.cpp)
- [S2 RSS 太陽系 numerics blueprint](https://github.com/mockingbirdnest/Principia/blob/0feb271b24a2a0c9ab200e34766711265d220039/astronomy/sol_numerics_blueprint.cfg)
- [S3 Ephemeris 接口、massless bodies、並行限制](https://github.com/mockingbirdnest/Principia/blob/0feb271b24a2a0c9ab200e34766711265d220039/physics/ephemeris.hpp)
- [S4 Ephemeris 推演／預算／checkpoints／重建](https://github.com/mockingbirdnest/Principia/blob/0feb271b24a2a0c9ab200e34766711265d220039/physics/ephemeris_body.hpp)
- [S5 連續星曆、多項式擬合、查詢及序列化](https://github.com/mockingbirdnest/Principia/blob/0feb271b24a2a0c9ab200e34766711265d220039/physics/continuous_trajectory_body.hpp)
- [S6 船舶歷史與近期補算](https://github.com/mockingbirdnest/Principia/blob/0feb271b24a2a0c9ab200e34766711265d220039/ksp_plugin/pile_up.cpp)
- [S7 船舶預測、可重建歷史與生命週期](https://github.com/mockingbirdnest/Principia/blob/0feb271b24a2a0c9ab200e34766711265d220039/ksp_plugin/vessel.cpp)
- [S8 軌跡降採樣與序列化壓縮](https://github.com/mockingbirdnest/Principia/blob/0feb271b24a2a0c9ab200e34766711265d220039/physics/discrete_trajectory_segment_body.hpp)
- [S9 視角誤差控制的曲線繪製](https://github.com/mockingbirdnest/Principia/blob/0feb271b24a2a0c9ab200e34766711265d220039/ksp_plugin/planetarium_body.hpp)

## 1. 真正值得學習的是時間尺度與資料用途的分離

Principia 的 massive-body ephemeris 只由天體相互作用推進，船舶在這個場中當
massless test body。這不代表船沒有慣性質量或燃料；是船不反作用改寫天體星曆。
VOID 的 celestial `Ephemeris`／`CoupledWorld` 也沒有把 PartGraph 船質量加入天體力計算。
船可共享同一份天體場，不必每艘船重做 N 體。（S3、S4）

但「天體固定步長、所有飛船一律自適應」不是 pinned Principia 的完整行為：

| 用途 | Principia 現有路徑 | VOID 對照 |
| --- | --- | --- |
| 天體 | 固定步長積分＋連續多項式軌跡；通用預設 Blanes–Moan SRKN14A／35 min；RSS blueprint 改 Quinlan–Tremaine 12 階／10 min | 單星系 Yoshida8、最緊近心點時間尺度/256；58 體 h=105.23952868085598 s；不是船舶 dt |
| 無額外力的船歷史 | 固定 10 s、Quinlan 8 階；不足當前時刻部分以 adaptive psychohistory 補齊 | 既有 Orbit owner 的 DOPRI5，搭配 Fleet 外層時間分段 |
| 當前有額外力 | PileUp 清掉固定步實例，以 adaptive 推進；所傳額外加速度在該次呼叫內可為固定 force/mass | AirSource 在 trial 依 t/x/v/m 求值，氣動／姿態耦合另受短 leg 限制 |
| 無推力未來預測 | adaptive RKN、獨立背景 prognosticator、步數預算 | DOPRI5／導航預測同步，已有 accepted-step 預算但延伸星曆不受它約束 |
| 計畫燃燒段 | generalized adaptive RKNG，可處理依狀態而變的加速度 | DOPRI5 七維 x/v/m，加推力與質量流率，末端由燃盡／機動邊界截斷 |
| 接觸／地面 | KSP／PhysX 接線有自己的負責範圍，不是僅靠軌道預測器解接觸 | Rapier／Ground／Bubble 和 PartGraph 是既有權威路徑，不另造 Principia-style 平行 runtime |

設定依 S1/S2，分工依 S6/S7。不能把 Principia 的 35 min／10 min 直接套到 VOID：
天體組成、最短衛星週期、積分器、擬合器與誤差目標都不同。
Principia 的 prediction 預設 1 m、1 m/s；psychohistory 是 1 mm、1 mm/s，
這是各自局部積分誤差控制，不是任意時間的全域真誤差保證。（S1）

**決策：保留天體／船解耦；不因近地船需要小步就把所有天體或其他船一同縮步。**
不為了「像 Principia」立刻把 VOID 船歷史改成固定步多步法；它涉及重啟、接觸、
變力、owner 交接與 replay，需先證明比現有自適應路徑更有利。

## 2. 近地、氣動、近遇與星際的步長控制

VOID 已有 adaptive，需改進的是完整調度，而非另加一個名稱叫 adaptive 的功能：

- `orbit/src/propagator.rs::advance` 按位置／速度各分量的誤差與容差控制 DOPRI5 接受／拒絕，
  空氣在各 trial 求值；末端與燃盡事件由 caller 分段。不在 trial 提交真實燃料／模組。
- `vessels/src/fleet.rs` 的 Scene 固定步預設 1/60 s；氣動力矩、轉向、輪子／水面等
  耦合情境也把 Orbit leg 限制為小步。不能把這個接觸時鐘換成星曆的 105 s。
- `advance_on_rails` 若 world environment 有大氣，就用 `flight_chunk_seconds`
  （FleetOptions 預設 1 s），否則用 `rails_chunk_seconds`（預設 10 s），再受 band safety
  限制。這個 world-wide 判斷可能讓遠離所有大氣的星際船仍頻繁回到外層迴圈；
  是**已核對的程式路徑與效能假設**，未以完整星際航程 profiling 證明為最大瓶頸。
- `multiscale::Traveller` 獨立 probe 已有每步 split anchor、adaptive、最大 5 天與
  clearance/(4×relative speed) 接近限制；主 Fleet 並不是自動使用此 probe。
  其 5 天也不是整個主遊戲的實際步長，不可直接宣稱星際整合完成。
- `VesselPropagator` 目前先 `ephemeris.extend_to(t_end)`，才檢查 vessel max_steps；
  rejected trials 不計入該局部 steps 計數。統一資源預算需涵蓋兩者，否則「1000 步」
  仍不能限制整個請求的等待時間。

**方案：以當前力學與可預見事件限制 h，不只按高度或距離恆星硬切。**
遠方 coast 允許自然增步，但高速 flyby、推力／姿態切換、大氣入口、地表、近船
接觸、owner／frame 交接都須設安全區間、截步／定位事件。adaptive 只看 stage 上的
誤差可能跨過狹窄作用區；要有區間事件檢查，不只測起點「現在沒有大氣」。
近地高精度與長航程可用不同明示品質配置，但誤差容差、max step、events 是不同概念。
不能在負載升高時默默放寬容差，也不能用遠方 barycentric 座標尺度放大相對誤差容許值。

現有單星系 0.02 m／0.001 m/s 是 navigation benchmark 的設定，不能当整個 app
所有 owner 統一配置；FleetOptions 的基礎預設更嚴（1e-6 m／1e-9 m/s），實際情境
可由其 world/options 設定。後續 profiling 必須記錄 effective options，不能混用。

## 3. 積分步、查詢表示、繪圖點必須分開

Principia `ContinuousTrajectory` 收到等間距的 q/v，每 8 個間隔／9 個點做 Newhall
多項式近似，完成後清除已吸收的原始點，保留尾部支援下段。依估計誤差選 3–17 次；
目前實際儲存／求值為 monomial-basis polynomial、Estrin policy，不能把舊說明的
「Chebyshev」當作現行儲存布局。（S5）

它的 1 mm fitting tolerance 是近似誤差估計門檻，不是天體真實位置精確到 1 mm。
程式在數值不穩時會調整 tolerance；這不符合 VOID 不默默放寬門檻的原則，不能照搬。
VOID 應在達不到目標時明確拒絕該壓縮段，或按**預先定義、明示的 lossless 儲存模式**
保存原樣本，報告原因；不能悄悄改成更差的曲線。

查詢方面，Principia 先試最後使用的 polynomial interval，未命中才 binary search；
直接求位置／導數，兩者同查可共享運算。VOID 已有固定網格索引＋兩個 chunk 查找＋
五次 Hermite，58 體位置查詢約 0.47 µs/次（既有 microbenchmark），並非當前主要瓶頸。
先確保不重算／不配置臨時 arrays；若加入可變長壓縮段再測 reader-local cursor，
不可為了快取引入全局鎖或跨 source/version 的失效錯誤。（S5）

**多項式壓縮不會使天體積分自動加快。** 保留同 h 時主要減少 storage，新增 fitting
反而有 CPU 成本；只有 accuracy sweep 支持更大 h，才可能另省 force evaluations。
先保持積分器不變，把儲存表示作獨立實驗，測總 time/memory/error。

繪圖方面，Principia 的 Planetarium 有依 angular／apparent error 調整取樣間隔、
限制點數的路徑。VOID `view/src/plot.rs::PlotPath::update` 目前令 dt≈整段時間/512，
未依放大倍率或近地曲率選取密度；長航程的近地短段可能分不到足夠頂點。（S9）
這是使用者所見折線的**具體候選**，不是已重現的根因。

**決策：先把近地折線按三種誤差分別驗證：天體插值、飛船積分／dense output、畫面折線。**
固定相同世界／相機，獨立調 h、vessel tolerances、plot sampling；觀察 body-relative
位置／速度、clearance 與 pixel error。螢幕曲線改成誤差導向取樣時只改 presentation，
不能把繪圖簡化線回灌物理、導航事件或 impact 判斷。

## 4. 四種「省記憶體」不是同一件事

### A. 天體連續表示

上節的 Newhall polynomial 代替每步原始數組，是常駐表示的壓縮。VOID 目前每個 body
每筆 q/v/a=72 bytes，58 體約 3.43 MB/日；1024 samples/chunk 約 4.276 MB，
約 1.25 日。37 天導航約 128 MB。不能再使用先前漏掉 acceleration 的 48 bytes／
2.3 MB/日估算。压缩比取決於階數、跨度、尾部、metadata，未實測不能承諾。

### B. 船歷史的誤差有界降採樣

Principia 在新點加入時，對延長的 cubic Hermite 與前段曲線之差估計範數，累積
compression error，在容差內才移除中間點。不是每 N 點留一點，也不是只比端點。
一般 backstory 預設 10 m；orbit analyser 用 1 mm。這是儲存／分析品質區分，不是
把物理誤差一律放到 10 m。該界線是相對原曲線的壓縮誤差，仍不包含原積分誤差。（S1、S8）

VOID 的 `Trajectory` 是 accepted-step q/v VecDeque＋cubic Hermite，目前無降採樣。
先對可丟棄的顯示歷史實驗，保留事件、owner 切換、燃燒起止與精確 anchor；
physics／navigation authoritative 軌跡若要改表示，需定義位置和速度誤差、端點
一致性、事件誤差與版本，不把呈現容差當物理容差。

### C. 存檔壓縮

Principia 在序列化中用 ZFP：時間／error exact，q/v 可按設定容差壓縮，另外保存
端點與指定 exact points。這主要是 save bytes，**不等於查詢時的常駐 RAM 壓縮**。（S8）
VOID 的 authoritative checkpoint 優先 lossless；lossy 表示屬明示新 schema／品質模式，
不得宣稱 bit-identical replay。這輪不引入 ZFP 或 Principia C++ runtime。

### D. Checkpoint 與按需重建歷史

Principia 天體 ephemeris 保存積分器 checkpoint（最多約180日間隔，存檔亦可強制），
也保存 continuous trajectory 擬合器狀態。新格式存檔通常不逐段寫出全部可重建
polynomial。讀檔先恢復所需區間，背景 RequestReanimation 與阻塞 AwaitReanimation
分開；重建順序可往較早 checkpoint 走，但每段仍從 checkpoint **向前積分**。（S4、S5）
船歷史則區分可重建 coast 與不可重建的有外力／多船 pile-up 段；後者保存資料，
不是只靠初始位置就能重播所有氣動、碰撞或玩家操作。（S7）

這不是「Principia 已有一個固定 RAM 上限的通用 LRU」。本次看到的 continuous
polynomials 與 vessel backstory 仍可在 session 內成長；checkpoint/reanimation
首先解決持久化與按需恢復。VOID 必須另設自己的 memory budget 與 active-reader
區間保護，不能以採用 checkpoint 就宣稱 RAM 有界。

## 5. 背景預測、工作預算與確定性

Principia `FlowPrognostication` 先算到已知星曆範圍，再在明示的 ephemeris step
budget 內向前延伸；prognosticator 是 recurring background job。它可返回未達
原目標的短預測，取消則傳狀態；這不等於已驗證完整機動解。（S7、S4）

VOID 可借鑑的分工：

- EphemerisProducer：唯一的 celestial writer，按確定性數值規則生成 immutable segments。
- ReadView：明示 source/model/frame/version 及 covered interval，多船／候選共享只讀資料。
- PredictionJob：own vessel run、cancel token、generation、deterministic trial/step/bytes
  budgets；不可持有可變 live Fleet，不在 UI thread 等 job join。
- ConsumerLease：記錄物理、計畫、事件與顯示仍需要的最早/最晚區間；解除後才回收。
- CheckpointArchive：可重建資料的精確恢復錨點；包含 q/v、Kahan 補償、step index、
  模型與 frame/catalog identity；多步法還需完整過往內部狀態，不能只存畫面插值值。

這些是 VOID 的擬議接口，不是聲稱 Principia 使用相同抽象名稱。
首版仍可每 job 獨立 prediction ephemeris，但不得在 UI thread clone 巨量歷史；
長期應共享 immutable celestial segments，避免候選各存128 MB。

主遊戲多星系目前是 `Rc<RefCell<CoupledWorld>>` 的共享 view，不能直接跨 thread 搬用；
需要明確 snapshot／immutable read-view 接口，不能只把型別換成 mutex 就在主幀
等待長積分鎖。其 8192 樣本自動淘汰也必須接入 lease 契約，避免預測延伸把其他
船或計畫仍需的過去區間淘汰。

完成狀態須區分 complete／partial／cancelled／budget-exceeded／stale／numerical-error。
partial preview 可顯示實際截止時間；導航結果只有全部驗證通過才可提交，不能把
最早完成的候選直接當最佳解。journal 記錄確定性的提交內容，非 thread 到達時機。

## 6. 建議實作順序與 gate

| 順序 | 交付 | 通過條件 |
| --- | --- | --- |
| P0 | 分離三種誤差的近地 witness；記錄實際外層 dt、accepted/rejected h、星曆延伸與 plot 點密度 | 在相同源資料上辨識折線來自哪一層，報告而不先降全部步長 |
| P1 | 背景導航＋可取消的分批 ephemeris/vessel 推進＋配置前 bytes budget | 圖面持續回應、超限無巨大配置；舊 generation 不提交；同步／背景結果同一 oracle |
| P2 | 主遊戲區間 lease、精確 checkpoint、可重建 coast 與不可重建事件段 | 多船／多計畫依賴不被回收；取消／讀檔／切換釋放；重建與原 raw source 一致；存讀/journal 核對 |
| P3 | 星際外層事件導向 chunk；近遇／大氣入口縮步 | 同一 Fleet owner/PartGraph；遠航省外層迴圈，近遇／熱／氣動／地面事件不漏；不改成另一個 Traveller runtime |
| P4 | 連續星曆壓縮與船歷史降採樣各自實驗 | 給出 memory/query/build/error Pareto，沒有未驗證的壓縮比承諾；物理格式變更另版本 |
| P5 | 比較替代固定步天體方法／IAS15、RKN 船方法 | 同 force model／同事件精度／同輸出密度的時間比較，不只量孤立 kernel 或能量漂移 |

P0 的 presentation 修正與 P1 的 job 管理可分開交付。P2 的 lease／預算不必等到
高階壓縮研究；P4 首次可在既有星曆樣本上離線比較，不急著替換 production source。
4-worker N 體維持 opt-in；先讓上述 ownership 可測，再和 LOD／renderer 同時測，
才能決定共享 CPU 預算，避免 nested candidate workers × ephemeris workers。

## 7. 驗證矩陣

- 近地／再入：有氣動、姿態變化、熱／耗油、海岸與接地；以 body-relative state
  及地形 clearance 比較，固定 trial 不提交原則；分開檢查 view pixel error。
- 高偏心／flyby：近心點前後 step 分布、最接近距離／時間、入出境狀態、表面事件。
- 星際 coast→接近恆星→機動：same source epoch、split frame／origin acceleration、
  owner 交接前後狀態，不對跨系潮汐作未宣告的忽略；長區間記憶體和 outer-loop 次數。
- 多船與多計畫：共享讀取、前後亂序 query、舊區間 pin、任務取消／重建，無失效 reader。
- 精度：原 golden 不變，縮 h／容差的收斂與独立參考並列；局部 tolerance、插值／
  壓縮 error 與整段 global error 分開。既有30日Yoshida參考自身差15cm，不能當真值。
- 效能：generation CPU、query latency、fit CPU、retained capacity、peak RSS、save bytes、
  restore/reanimation latency、main p95/p99、cancel latency，各自量測。
- 相容：數值方法／查詢表示／持久化 schema 改變前明確記錄版本；舊資料明確拒絕，
  不自動修補。實際遊戲的人類驗收與 headless／agent 檢查分開。

本次沒有執行遊戲／數值測試，因為僅調查與文件修正；來源完整性檢查和程式接口核對
如上。下一次功能實作依 AGENTS.md 另訂分支、修改範圍、接縫與所屬測試。
