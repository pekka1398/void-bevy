# N 體效能探索

工作分支 `work/nbody-profiling`，基底 `work/orbit-navigation` 的 `60d97e4`。
包含實際新增的 58 天體及導航；未合併、未 push。這輪以 headless CPU 核心／整合量測為主，
沒有 GUI 或人類遊玩驗收，也不把核心時間改善換算為 FPS。

## 問題與採用的改動

`Ephemeris` 使用固定步長 Yoshida 8 階，15 個 leapfrog 子步，每子步重算全部互引力。
原 kernel 按 `(i,j), i<j` 算一次距離，同時更新兩個天體。不是 58 個獨立的 Kepler 軌道。
Kahan 位置 drift、kick、Hermite 樣本和插值仍保持原計算方式。

新增 `AccelerationBackend`：預設 Auto，在可用的 x86_64 AVX2 上選四對 f64 SIMD；
其他平台選 scalar local。ScalarReference 保留原 kernel，供 oracle／回歸與量測。
明確指定 Avx2 而硬體不支援會報錯。沒有 approximate reciprocal、FMA、fast-math、
softening、忽略小質量天體或改步長。

- SIMD：每次 force evaluation 把位置轉成連續的 x／y／z 工作陣列，四個不同 j 同時計算
  距離、sqrt、除法及乘法；每個 contribution 仍按原順序逐個加回，沒有水平 reduction。
  陣列重用，不逐子步配置；i 的累加保留在局部變數，最後寫回。
- ScalarLocal：同樣把 i 的累加放在局部變數，維持 j 的更新及所有浮點表達式。
- `enable_profiling`／`profile`：可選的 extend、force、樣本儲存時間及實際步數／force 次數。
  預設不讀 clock。細分時間有 instrumentation overhead，與無插樁測時分開。

## 量測

硬體：Ryzen 7 H 260，8 核 16 執行緒；release，`cargo -j 2`。每模式先 warmup，
一天完整星曆各 9 次，scalar → local → SIMD → SIMD → local → scalar 交錯；
計時包含積分、有限值檢查、樣本儲存及 chunk 配置，排除建構／後續插值與船舶測試。
以下是兩輪各自的中位數範圍，非僅 kernel throughput。

| 系統 | ScalarReference | ScalarLocal | AVX2 SoA |
| --- | ---: | ---: | ---: |
| 15 體，一天 | 1.015–1.057 ms | 0.956–0.973 ms | 0.929 ms |
| 58 體，一天 | 86.221–87.354 ms | 78.034–80.291 ms | 76.004–76.976 ms |

58 體 SIMD 改善約 12%，scalar local 約 8–10%。初版直接收集交錯 xyz 的 SIMD
約 82.7 ms，只有約 3% 改善；只加 intrinsics 不足夠，資料布局／scatter 成本很重要。

15 體步長 593.461906 s，一天 146 步、229,950 對 force evaluation；
58 體步長 105.239529 s，一天 821 步、20,356,695 對。
配對數 `1653/105 ≈ 15.74`，一天步數比 `821/146 ≈ 5.62`；組合約 88.5 倍工作量。
最短 periapsis 時間尺度新增衛星讓步長縮短，不能只按 N² 估計。
這也表示正常 1× 時間前進通常不需要每幀積分；58 體每步平均約 0.093 ms，
長時間 warp／導航延伸才會一次累積許多步。此平均不代表最差 chunk 配置延遲。

58 體插樁的一天：force 約 75.13/77.40 ms（約 97%），樣本儲存約 0.470 ms。
不是繼續優化 BTreeMap／樣本 copy 就能移除主要成本。

15／58 體的 10,000 次全體 positions 查詢約 1.35／4.59 ms；
10,000 次船舶完整 gravity（含插值及 J2）約 2.25／6.75 ms。
在已覆蓋星曆上，Aurelia 400 km 的一小時無推力傳播約 0.15／0.46 ms，
兩者均 57 accepted、0 rejected，Reached。這是一個固定 fixture，不能代表所有
近掠、推力、撞擊或導航候選的自適應步數。

58 體 30 天完整星曆，3 次中位數 2.346 s，保留 106,905,600 bytes；
15 體約 29.17 ms，保留 5,529,600 bytes。30 天測試沒有 Forget，正好對應需要保留
未來可查詢星曆的情況；不能用此範例宣稱長期軌道完全穩定。

實際 expanded-system 導航 fixture（Aurelia 400 km、Selene、30 天窗口／7 天航程，
有限推力 1 MN、1000 kg、dry mass 100 kg、exhaust velocity 10 km/s）已實跑。
首輪 scalar 4.397 s／Auto 3.786 s，星曆部分 3.614／3.012 s；
星曆保留 128,286,720 bytes，延伸到約 37 天。兩種 backend 的完整導航解文字
逐項相同（Rust round-trip 浮點輸出），不是只比「有成功」。量測開啟細分 profiler，
沒有 renderer；不能拿它當一般機體、所有導航目標的延遲保證。

## 多執行緒的邊界

32 個彼此獨立、每個 58 體一天的任務，三次完整批次、結果 energy bits 相同：

| workers | 批次中位數 | 相對單 worker |
| ---: | ---: | ---: |
| 1 | 2.432 s | 1× |
| 2 | 1.220 s | 1.99× |
| 4 | 0.617 s | 3.94× |
| 8 | 0.317 s | 7.66× |
| 16 | 0.245 s | 9.94× |

這是獨立問題的吞吐量實驗，尚未把導航或多星系主遊戲接到 worker。
單一星曆的一個 force evaluation 只有約 6 µs；每步需 15 次依賴性的全體同步。
是否能把這種小任務拆給持久 worker 得益，仍需專門 kernel 實驗；沒有測過，不能據此
宣稱並行 force 不值得。直接在每個子步 spawn threads 明顯是錯的工作粒度。

導航的 departure-window coast 是一條連續時間線，不能拆成互不相干的步驟；
但獨立候選的有限推力驗證可用已延伸、唯讀且可跨執行緒的星曆平行處理。
目前 `EphemerisSource` 無 Send/Sync 契約，coupled source 有 Rc/RefCell，並不是
加 `par_iter` 就能安全完成。候選排序／拒絕／選中結果也須維持確定性。
建議先做可中斷的背景任務與 immutable chunk snapshot，之後才平行候選。

## 導航的較大問題

目前主遊戲 UI 同步呼叫 GenerateNavigation → solve_navigation。
搜尋中 `extend_to(t + max_flight_seconds)`，星曆為全體共用、單調延伸，沒有此 solver
自己的記憶體／積分時間預算。候選 accepted-step budget 不限制星曆延伸的成本。
介面預設等待 30 天／航程 7 天，最遠約 37 天；最大等待 1825 天／航程 3650 天。

按既有 105.24 s 步長與 58×9×8 bytes/sample，37 天約 127 MB，5475 天約 18.8 GB
（十進位、加上 chunk rounding；不含軌跡／其他遊戲資產）。時間按 30 天實測線性估計
約 2.9 s／428 s，**這是外推，不是實測整段導航時間**。不得實跑十多年把機器耗盡。

優先處理背景運算、取消、可觀察的進度和明確資源預算；拒絕必須告知原因，不能偷偷
縮短請求、換二體模型或漏掉天體。長區間可研究精確積分照舊但降低儲存密度的 Hermite
輸出／有界 paging；這需要新的插值誤差和查詢生命週期驗證，不能直接忘掉候選仍要讀的
chunks。它比現在 force kernel 再快幾個百分點更影響使用感受。

## 演算法與 GPU 候選

- Barnes–Hut／FMM：近似合併遠方引力，58 體規模小，而且改變引力／累加語義；
  不適合直接替換目前必須維持結果的 direct kernel。若未來有海量小行星，再比較誤差與
  crossover，不以漸近複雜度當現在的收益。
- Wisdom–Holman／WHFast：把主要 Kepler 漂移精確解掉，可能允許更大的步長；但此系統
  有衛星層級、彗星和近遇，不能假定只有恆星主導的弱擾動。
  [REBOUND 官方說明](https://rebound.hanno-rein.de/ipython_examples/WHFast/) 明確指出近遇時
  WHFast 不適合，且改步長會破壞其辛性。值得做獨立 accuracy/time-budget 對照，不能
  直接取代 golden 與既有軌道行為。
- IAS15／混合積分：可處理近遇及高偏心，會改變積分樣本及誤差策略。
  [IAS15 官方說明](https://rebound.hanno-rein.de/integrators/ias15/)；
  [原論文](https://arxiv.org/abs/1409.4779)。屬模型層級實驗，須能量、角動量、phase、
  月球／Phobos 長期相對軌道和 vessel 事件的共同驗證，不能只比單次 force 時間。
- GPU tiled all-pairs 是成熟方向，見
  [NVIDIA 的實作說明](https://developer.nvidia.com/gpugems/gpugems3/part-v-physics-simulation/chapter-31-fast-n-body-simulation-cuda)。
  本輪沒有 GPU kernel 時測；58 體與每個 6 µs CPU phase 不足以先宣稱 GPU 能贏。
  要測的是完整駐留積分、15 phase 的同步、輸出／回讀與 renderer 競爭，不是只有 shader。
  標準 WGSL 的 f32 不可直接替換目前 f64 物理。座標分塊解決位置表示問題，並不自動
  解決平方距離、inverse-cube 動態範圍、GM 和力的累加精度。

f32 初始引力實驗（58 體；只比較初值，沒有當遊戲 backend）：

| 輸入方法 | 原 inverse-cube 公式最大相對誤差 | 改成方向 × GM/r² 後 |
| --- | ---: | ---: |
| 絕對位置先轉 f32 | 100%（遠方分母溢位） | 1.8935%（Charon） |
| 相對 Sol 後轉 f32 | 100% | 0.6586%（Styx） |
| 先 f64 求每對差值再轉 f32 | 100% | 約 3.13e-7（Vesta） |

最後一種方法可作精度研究基準，但需 CPU 算全部 pair delta，並未證明 GPU 可更快。
不把較小的單次 force 誤差當長期 orbit／impact／burn 行為等價。沒有測 split-tile GPU
運算或 f64 GPU，亦沒有更改 physics 的 f64 契約。

另有 `CoupledWorld` 的多星系互引力路徑；它保留 split origins、跨系 external force
與質量加權的移動原點加速度，公式／累加次序不完全相同。本輪未改此 kernel，
上述 SIMD 收益只適用 `Ephemeris` 單星系（包含 navigation 分支的預設 58 體）。
不能套用 worker 的「獨立系統」數字到存在跨系潮汐的 coupled simulation。

## 重現與驗證

```sh
cargo run --release -p void-orbit --example nbody_profile -j 2 -- 9 scalar
cargo run --release -p void-orbit --example nbody_profile -j 2 -- 9 local
cargo run --release -p void-orbit --example nbody_profile -j 2 -- 9 simd
cargo run --release -p void-orbit --example nbody_profile -j 2 -- 2 simd 1 profile
cargo run --release -p void-orbit --example nbody_profile -j 2 -- 3 simd 30
cargo run --release -p void-orbit --example nbody_precision -j 2
cargo run --release -p void-orbit --example nbody_workers -j 2
cargo run --release -p void-orbit --example navigation_profile -j 2 -- scalar
cargo run --release -p void-orbit --example navigation_profile -j 2 -- auto
```

`lab-log/nbody/` 保存原始 JSONL、測試及 scoped Clippy logs。
Benchmark days 明確限制 ≤30，避免不小心跑到上述十多年請求。
逐位元測試覆蓋 1/2/3/4/5/7/8/15/57/58 體、SIMD 尾部、chunk 邊界、Forget、
插值位置／速度、energy 和 angular momentum；實際 58 體完整一天。
原 golden／導航單元測試一併保留門檻。

最終 scoped 驗證：orbit release 39 passed／1 個既有 ignored；另外明確執行該
`real_sol_parking_departures` witness，1 passed。Fleet 的 plans／expanded_bodies 7 passed。
Orbit all-targets Clippy `-D warnings`、fmt、diff check 通過。沒有跑全 workspace／app GUI。
Fleet 編譯與實際導航使用已知自啟動 PID group 的 RSS／MemAvailable／PSI guard，
`-j 2`，沒有與 Bevy linker 或 GUI workload 重疊。

最後重建後又重跑六組 9 次交錯測時：58 體 scalar 84.97／90.40 ms、local
78.16／78.36 ms、SIMD 75.95／76.15 ms。15 體首次 scalar 出現 2.178 ms 的
較大系統噪音，末輪 1.016 ms；因此不以約 1 ms 的小樣本宣稱普遍遊戲收益。
六組診斷取樣位置 bits 一致。實際導航再重跑 Auto／scalar，各次完整解均一致。
導航第二組 Auto 3.671 s／scalar 4.295 s；仍是 headless fixture。
