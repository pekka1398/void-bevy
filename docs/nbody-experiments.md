# N 體進階實驗：工作粒度、GPU 與積分方法

2026-10-10，`work/nbody-profiling`；接續 [第一輪](nbody-performance.md)。
仍以 navigation `60d97e4` 的 58 體為基底。沒有合併、push 或人類 GUI 驗收。
所有量測 headless；沒有推論遊戲 FPS。測試與硬體原始資料位於 `lab-log/nbody/`。

## 採用：四條 target rows 的 f64 SIMD

第一輪 SIMD 保留 pair 對稱性，每四個 j 算一次距離，再逐項 scatter 到兩個天體。
本輪讓四個 target i 各占一條 AVX2 lane，每個 source j 依序 broadcast，累加保持
vector registers。距離重算兩遍，但省掉內圈 scatter／scalar reduction。

當 j<i，使用原 `(j,i)` 的差值與 subtraction；當 j>i，使用 `(i,j)` 差值與 addition。
self lane 的值被屏蔽，尾部為 exact-order scalar rows。每一個天體仍按原 source
順序更新，保持 +／− 的 signed-zero 語義，不將兩次 kick 合併或使用 FMA。

兩輪 9 次交錯量測、每模式先 warmup，包含樣本儲存與 chunk 配置：

| backend | 15 體，一天 | 58 體，一天 |
| --- | ---: | ---: |
| 第一輪 pair SIMD | 0.953–0.971 ms | 77.43–77.72 ms |
| target-row SIMD | 0.890–0.917 ms | 57.81–58.24 ms |

在此工作負載，新 kernel 比上一版快約 25%，比原 ScalarReference 約快 32–34%。
預設 Auto 在 AVX2 上改選 `Avx2Rows`；其他平台仍選 ScalarLocal，原 pair SIMD、
ScalarReference 和所有試驗模式保留 explicit backend。未改物理／模型版本。

## 採用：RK step 內重用同一時間的星曆查詢

Dormand–Prince 最後兩個 stage 都在 `t+h`。船的位置可以不同，但只讀星曆的
天體位置／速度相同。只在一次 `Dopri5::step` 的 closure 內記錄前一次時間的 bits，
相同時間直接重用 Field 中的天體位置／速度；重算船舶 gravity、J2、thrust、air。

每個新 step（含重試）、每次 `advance`、控制變更、直接 gravity 查詢和撞擊求值
都重新取樣。不把只看 t 的 cache 放到跨場景／跨 source 的長生命週期中。
`set_stage_cache(false)` 保留 baseline。沒有快取或提交 trial 耗油／模組狀態。

Aurelia 400 km、已覆蓋星曆的一天 coast，21 次＋warmup、ABBA：

| 同時間重用 | 兩輪中位數 | 行為 |
| --- | ---: | --- |
| 關閉 | 11.033／11.086 ms | 1308 accepted，0 rejected |
| 開啟 | 10.641／10.549 ms | 完整 accepted trajectory 相同 |

改善約 4–5%，不是只有一個新的快取計數。另有 coast、constant force、inertial／
Frenet／Surface thrust 的跨 advance 控制切換 oracle，包含末狀態與全部軌跡。

實際月球導航，新 Rows＋stage reuse 約 2.949 s，星曆部分 2.208 s，
retained 128,286,720 bytes。完整解與第一輪 scalar／SIMD 四次導航 oracle 相同。
原 scalar fixture 約 4.3–4.4 s，第一輪 SIMD 約 3.7–3.8 s；它仍是同步操作。

## 不採用：同一星曆的持久 workers

實作了常駐 worker pool，分配固定 target ranges；每個 worker 重算其 target 對
所有 source 的引力，原順序累加。輸入陣列重用、結果 buffer 來回移交，沒有每子步
spawn/join。每 phase 用 channels 同步，會複製一份 q。保持 Ephemeris 的 Send/Sync。

| workers | 15 體，一天 | 58 體，一天 |
| ---: | ---: | ---: |
| 2 | 18.77 ms | 168.70 ms |
| 4 | 19.03 ms | 159.03 ms |
| 8 | 27.00 ms | 173.15 ms |

比 SIMD 慢，保持 `Workers(N)` 實驗選項，不進 Auto。這個具體設計的同步／雙重 pair
工作量有成本，不是「Rust threads 沒用」的結論。更大的 N／跨獨立候選仍可能有收益；
第一輪獨立任務 workers 的接近 10× 吞吐量依然成立。

1/2/4/8 workers、SIMD lanes／tails、history／Forget 逐位元比較通過。
worker 的非法 coincidence panic 會傳回 owner；有回歸確認錯誤不會導致等待 deadlock。
Drop 結束 thread；非法 worker 數明確拒絕。沒有 silent fallback。

## 不採用：CUDA f64 完整駐留積分

這台機器有 CUDA 13／RTX 5060 Laptop，實際編譯 native probe，而不是只估計 shader。
`tools/nbody/resident.cu` 以 `--fmad=false -arch=sm_120` 編譯：

1. 每個 target 一個 thread，各自照原順序算引力；單 block 駐留 q，v／Kahan 在 registers。
2. 每個 pair 平行算一次，兩側 contribution 存在 shared matrix，再按原順序 reduction；
   512 threads、約 82 KB shared memory，避免重算 pair。

兩者均做完整 15 子步、保存每步的 q／v，回讀全部樣本核對。

| CUDA kernel | 一天 kernel | 包含全部 q／v 回讀的 wall |
| --- | ---: | ---: |
| target rows | 約 606.3 ms | 約 606.8 ms |
| shared pair matrix | 約 373.5 ms | 約 373.9 ms |
| 32 個獨立 pair-matrix blocks | 約 748.9 ms／批次 | 約 759.1 ms／批次 |

32 blocks 將工作分散到多個 SM，對照第一輪 CPU 16 workers 的相同 32 個任務約
244.7 ms，仍慢。GPU 雙精度、shared matrix 與同步成本沒有被「駐留」自動消除。
未與 renderer 同時跑；沒有 Vulkan/WGSL 的 GPU physics 接線，沒有依此下普遍
GPU／較大粒子系統的結論。

每個 raw q／v value 都逐位元一致，單軌跡 285,708 個、32 軌跡 9,142,656 個。
最初拿 CPU 的端點 `states_at` 當 raw oracle 有 10,153 個不一致；檢查後確認
Hermite 的括號／端點時間 rounding 會產生不同的最低 bits。新增 `current_states`
讀原積分器狀態，重新生成 oracle 後三個 CUDA 模式的 mismatch 均為 0。
這是 oracle 接縫問題，不是放寬 GPU 誤差門檻；原插值方法保持不變。

## 不採用：降階或放大固定步長

獨立 Rust lab，以同樣 direct f64 force／Kahan drift 比較 8 階、Yoshida 4 階、
Verlet 2 階。共同終點 `832 × h`，約一天，參考為 Yoshida8 的 h/8。時間用 lab 的
scalar force，不拿 lab kernel 絕對時間當主遊戲新 SIMD 的效能。

| 方法／步長 | 時間 | 最大相對 parent 的位置差 |
| --- | ---: | ---: |
| Yoshida8／h | 79.5 ms | 2.81 mm |
| Yoshida8／4h | 19.6 ms | 1.10 m（Phobos） |
| Yoshida4／h | 16.4 ms | 115.9 m（Amalthea） |
| Yoshida4／h/16 | 254 ms | 2.24 mm |
| Verlet2／h | 5.77 ms | 177.8 km |
| Verlet2／h/16 | 84.1 ms | 694.7 m |

能量誤差小不代表 orbit phase 精確：Yoshida4／h 的能量相對誤差約 1.6e-16，
衛星仍錯 116 m。lower-order 比較快，卻無法同時維持同樣精度；所有方法保留 lab。
細步長參考也存在 roundoff，不能把 sub-mm 數字當精確真值。

## 外部算法：WHFast、IAS15

隔離於 `target/rebound-venv` 的 REBOUND 5.2.2，只是 optional research dependency，
不加入 Cargo、遊戲 runtime 或 required tests。
官方方法說明：[WHFast](https://rebound.hanno-rein.de/ipython_examples/WHFast/)、
[IAS15](https://rebound.hanno-rein.de/integrators/ias15/)。
相同 gm（G=1）、barycentric 初值、同一終點；WHFast safe_mode=0 後 explicit synchronize。
WHFast 用完整步，不以最後一個短步改變方法；紀錄實際時間差。

約一天、每設定 warmup＋3 次：

| 方法 | 中位數 | 對 h/8 raw reference 的最大位置差 |
| --- | ---: | ---: |
| WHFast／h/4 | 72.9 ms | 11.2 km |
| WHFast／h | 15.4 ms | 179.5 km |
| WHFast／4h | 3.77 ms | 2864.7 km |
| WHFast／h＋11 階 corrector | 15.4 ms | 178.9 km |
| IAS15／epsilon 1e-9 | 40.8 ms | 0.703 mm |
| IAS15／epsilon 1e-12 | 78.8 ms | 0.692 mm |

普通 WHFast splitting 並不適合直接替換這份多層衛星系；corrector 沒解決主要問題。
IAS15 值得深入，但這是不同 library 的算法／實作整體比較，不是純階數比較。
即使誤差小，adaptive sample／interpolation、近遇與 impacts、版本／錄放都須另行
驗證；沒有改成遊戲預設。

## 重現

```sh
cargo run --release -p void-orbit --example nbody_profile -j 2 -- 9 rows
cargo run --release -p void-orbit --example nbody_profile -j 2 -- 3 workers4
cargo run --release -p void-orbit --example vessel_stage_profile -j 2
cargo run --release -p void-orbit --example nbody_integrators -j 2
cargo run --release -p void-orbit --example nbody_cuda_fixture -j 2 -- lab-log/nbody/cuda-fixture-raw.bin
nvcc -O3 --fmad=false -arch=sm_120 tools/nbody/resident.cu -o target/nbody-cuda
target/nbody-cuda lab-log/nbody/cuda-fixture-raw.bin pairs
target/nbody-cuda lab-log/nbody/cuda-fixture-raw.bin pairs 32
```

CUDA 是可選 native NVIDIA 實驗，不作一般 Rust CI 的要求。
REBOUND 以獨立 venv、指定 5.2.2 安裝，腳本見 `tools/nbody/`；沒有修改系統 Python。
昂貴運行逐次執行、`-j 2`、已知自啟動 PID group 資源 guard；沒有開新 agents。

## 30 天參考解收斂（前輪已完成，本次補齊記錄）

`reference-long8.json` 與 `reference-long16.json` 的共同終點為
2,593,101.9866962912 秒；最大絕對位置差 0.154274901 m。
IAS15 epsilon 1e-9 中位數 0.863 s、對 h/16 最大位置差 0.1125 m；
epsilon 1e-12 約 2.01 s。這些差值不能視為真誤差，因參考自身尚未更精確收斂。
WHFast h/16 約 4.689 s、parent-relative 差約 20,980 m（Amalthea）。
此結果支持繼續研究 IAS15，沒有支持直接替換既有模型／插值。

## 原子同步 workers：收尾與閒置修正

前輪最後未整理的 SpinWorkers 已有收益，不能沿用 channel workers 較慢的結論。
原版在 Ephemeris 閒置／僅查詢時仍忙等，本次增加有限 spin 後 park；新 phase 與
Drop 喚醒，先發布 parked 再檢查 generation/stop，使用 SeqCst 避免漏掉喚醒。
回歸覆蓋 idle→park→compute 多次循環、錯誤傳回與 drop，未將多 worker 設成 Auto。
沒有新開 worktree 或 agents；沿用中斷的效能工作區。

本次 serial 9 次＋warmup，rows→2→4→8→2→rows，每列為完整一天星曆中位數：

| 模式 | 15 體 ms | 58 體 ms |
| --- | ---: | ---: |
| rows 首輪 | 1.657 | 59.839 |
| spin2 | 1.685 | 39.099 |
| spin4 | 1.810 | 29.164 |
| spin8 | 2.305 | 32.222 |
| spin2 回測 | 1.791 | 37.882 |
| rows 回測 | 0.906 | 53.766 |

15 體與 rows 首末有波動；不能宣稱每個系統都適合多 worker。2 workers 使用
2 個工作 threads 加 owner 同步，4 workers 亦需 owner，不可只按 workers 數算
CPU 資源。所有量測不包含 renderer 競爭，不能推論主遊戲 FPS。

新增 `tools/nbody/compare_navigation.py`，串行正反順序測 scalar／Auto／2／4／8，
保存完整 solution 與 binary/source 摘要，assert 每個結果成功且與 scalar 完全相同。
研究執行檔先以 `cargo build --release -p void-orbit --example navigation_profile -j 2`
重建，再 `python3 tools/nbody/compare_navigation.py`；不要與編譯／其他 benchmark 重疊。

預設仍採 target-row SIMD＋stage reuse。下一階段具體 ownership、取消、預算、
replay、並行候選與 LOD／GPU 路線見 [實作與驗收規格](specs/performance-next.md)。
背景導航、記憶體上限尚未實作，這次完成的是 kernel／實驗收尾與下一階段規格。


### 最終完整導航對照與驗證

同一請求，各模式正反序各一次，`navigation-comparison/summary.json`：

| 模式 | 首輪秒 | 反序秒 |
| --- | ---: | ---: |
| ScalarReference | 4.3325 | 4.3849 |
| Auto（rows） | 2.9079 | 2.8989 |
| SpinWorkers(2) | 2.1772 | 2.2131 |
| SpinWorkers(4) | 1.8975 | 1.8666 |
| SpinWorkers(8) | 2.0200 | 1.9778 |

十次完整 solution 相同且成功，每次 retained 128,286,720 bytes。
ScalarReference 此處也使用本輪 stage cache；與歷史完全原版的差異需看前文。
4 workers 相較本輪 Auto 約省 35% wall time，但尚未量 renderer／LOD 競爭、
CPU 能耗或其他機器，故不把 4 設成通用預設。8 workers 未進一步改善。

檢查版本：`7cf29bc` 加本次收尾 diff（本文件所在提交）；獨立 worktree target。
Orbit release 44 passed／1 個既有 ignored，該 real-scale witness 另跑 1 passed；
Fleet plans／expanded_bodies 共 7 passed；orbit all-target Clippy `-D warnings`、
fmt、diff check 通過。新 park/resume、worker error、vector tails／history／Forget、
多種 thrust 跨 advance 的 oracle 均通過。日誌為 `lab-log/nbody/resume-*.log`。
沒有重新執行昂貴 CUDA／REBOUND 探針，相關數字經既有原始 logs 核對，非本次新量測；
沒有 app GUI／人類驗收、全 workspace、合併或 push。
