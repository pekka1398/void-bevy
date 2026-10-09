# LOD profiling 與初步效能檢查

基線 master `95c0581`，工作分支 `work/lod-profiling`。本輪增加量測與 CPU benchmark、消除 CPU 重複工作，並提供 opt-in worker／frustum bounds 實驗；不改 LOD 門檻、幾何、地形模型或畫質；無存檔版本變更。主線原有文件修改不採入。

## 量測入口

- `cargo run -p void-terrain --example lod_profile -j 2 > core.json`：四種 production terrain 的 33×33 tile，L4/L14/L18，各 64 tiles、1/2/4/8/16 workers、每項三次。worker batch 包含 thread 啟動，使用本專案 dev opt-level=1；不是整體 FPS。
- 主遊戲既有 `--profile` 增加 `lod_*` 細分 wall time 與 counters。worker time 不含 queue wait；`lod_create_mesh` 是 stitch／CPU mesh／Assets::add（bounds開啟時也包含CPU AABB計算），包含在 `lod_draw` 內，不可相加，也不代表 GPU upload。
- 離屏 `--render-benchmark` 另保存 `.cpu.warmup.cpu.json`，保留原本會在正式取樣前清掉的冷啟動 LOD 成本；正式 `.cpu.json` 只包含穩定取樣。GPU pass 使用現有 diagnostics，不啟用 detailed_trace，本輪不宣稱量到 draw-call 數。

CPU 硬體 AMD Ryzen 7 H 260，8 cores / 16 threads；GPU RTX 5060 Laptop 8 GB，driver 580.178.04。系統排程與頻率會影響結果。原始報告在工作區 ignored `lab-log/lod-profile/`。

## 實作審查

1. **背景生成已有 tile 層級並行**。`AsyncComputeTaskPool` 預設最多四個 workers；`TileField::select` 用 OS logical threads×2 當 in-flight 上限，此機通常允許32個工作而只有4個worker。排隊數不等於平行度。適合測試4/6/8個專用／重新配置背景workers，搭配全局queue上限、主執行緒與渲染幀時間，不能直接把所有core拿走。
2. **接縫變更重建整個 entity／mesh**。CPU mesh cache仍在，但渲染資產移除後重建，可能造成 upload與allocator尖峰。可先保留entity／更新mesh，再研究有界render mesh快取；16種coarse-edge mask只是基本情況，cache還必須包含鄰居版本與world ownership。
3. **多餘複製**。`stitch_edges` clone包括skirts的position/normal/height，`tile_mesh`再切片clone，最後實際不畫skirts。可在app做只生成drawn grid的轉換並直接移交vector；不能破壞core golden與skirt接口。各tile另複製相同indices及逐vertex constant cell，硬體shared buffer需Bevy自訂render路徑，先量upload成本再決定。
4. **每幀重建選集與接縫表**。穩定camera也重作HashSet/HashMap／鄰居查找。可依render selection revision及mesh版本只重算拓撲；相機相對f64 transform仍需更新，不能停掉。整體選擇結果的memoization需納入observer、camera、完成build、eviction及retain-frame語義。
5. **horizon求值有重複**。`below_horizon`每個node重算同一observer半徑及horizon的acos，可每次select預計算。先保持同一浮點操作／culling邊界與golden；直接用cos改寫需另驗證。
6. **balance多次全選集掃描**。鄰居平衡有全掃描及collapse時再次掃描，動態refinement尖峰值得測；可用受影響邊queue減少工作，但維持deterministic次序與無裂縫契約。

不建議第一步直接GPU生成全部地形。碰撞仍須CPU authoritative sampler；GPU版需維持同一取樣、cell band limiting、誤差估計與接縫，readback／雙份實作不一定划算。CPU SIMD需要batch sampler才能跨頂點有效向量化，目前scalar噪聲、分支及libm為主要障礙，浮點重排／fast-math不可直接開啟。

## CPU 初步結果

獨立重跑 `core-clean.json`，固定patch三次batch median，64個L18 tiles：

| 地形 | 1 worker ms | 4 workers ms | 8 workers ms | 16 workers ms |
| --- | ---: | ---: | ---: | ---: |
| Earth layered | 149.60 | 40.42 | 22.04 | 20.50 |
| Cinder | 203.18 | 54.31 | 31.44 | 25.33 |
| Ares | 221.33 | 56.99 | 30.25 | 26.10 |
| Vesper | 181.13 | 47.85 | 24.84 | 21.73 |

這證明此批CPU生成可並行，但不證明16 workers是主遊戲最佳配置。不能把worker累計wall time當主幀阻塞時間。

另用明示synthetic距離表、Earth sampler的靜止選塊測試：surface 1433 drawn／2054 built、payload約130.4 MB，select p50 0.641 ms（traversal 0.297、balance 0.326）；orbit 265 drawn／434 built，select p50 0.121 ms。這是core microbenchmark，不是主遊戲實際LOD配置或顯存量。

## 主遊戲量測與驗證

同一model31 saved surface world，1920×1080、120個GPU delivered samples；本地dev build，無detailed_trace。CPU samples包含122個main updates，與GPU delivered samples分列。

| CPU項目 p50 ms | instrumentation baseline | 優化後 |
| --- | ---: | ---: |
| LOD draw | 1.160 | 0.338 |
| LOD schedule | 0.155 | 0.00056 |
| traversal | 0.861 | 0.855 |
| balance | 0.685 | 0.681 |

CPU優化：初始化時快取OS slots、選集不變時重用接縫拓撲（相機anchor仍逐幀f64更新）、stitch arrays truncate後直接交Mesh。coarse stitch与mesh属性有等價驗證；沒有聲稱整體FPS提升相同比例。

`--lod-frustum-bounds` 是opt-in：將mesh AABB每軸擴大海面最大位移＋64eps×planet radius的f32 guard，取代NoFrustumCulling。地表opaque clipper invocations從3,111,550降到924,286（約70%）；這是GPU管線primitive量，不是draw call數。單次GPU opaque p50從1.057 ms至0.539 ms，但不同run頻率／負載影響明顯；不作跨場景速度保證。軌道同樣從735,870降到207,486，但首次GPU p50反而0.425→0.621 ms，後續ABBA交錯重測無裁切0.415/0.368 ms、裁切0.350/0.393 ms，改善小且有run波動，不能只報有利結果。

`--lod-workers 8` 用實際Bevy async pool8 threads，其他compute pool會得到較少threads。單次surface settle updates從125降至93；啟動時間含GPU初始化與LOD、暖機，不能當純worker加速比例。預設worker配置保留，尚無動態飛行長時間幀時間驗證。

candidate surface的無裁切／裁切／8worker／8worker+裁切，以及orbit無裁切／裁切，1920×1080截圖逐像素相同；candidate／bounds／worker8的完整checkpoint JSON一致，candidate與bounds saves headless verify通過。`--benchmark-image <path.png>` 在正式取樣後才回讀，避免污染run的GPU時間。固定場景圖片相同不代替完整人類GUI驗收與旋轉、海岸、天體切換、星際邊界驗證（地表preset為夜側固定視角），因此bounds維持opt-in。

CPU與GPU時間互相包含或重疊不可任意相加；offscreen非pipelined場景與真實視窗的FPS不等價。早期報告曾與編譯重疊，後續比較採獨立serial benchmark，原始檔保留以便追溯。

perf PMU probe被本機kernel拒絕（perf_event_paranoid=4，task-clock也無權限）；保留錯誤log，未改系統設定或宣稱取得cycles／cache misses。已取得本機wall timings、GPU timestamps/pipeline statistics與pixel comparison。

檢查範圍：LOD／terrain／diagnostics tests、app lib tests、受影響crate Clippy、fmt、diff check；沒有全workspace。最終app lib 41 passed，LOD golden 3 passed，terrain 16 passed，diagnostics 7 passed；受影響核心all-target及app lib/tests Clippy `-D warnings`、fmt、diff check通過。未merge/push。


## 接下來最值得實驗的硬體路徑

優先批次f64 terrain sampling／SIMD：跨獨立vertices lanes保持每lane運算順序，避免fast-math/FMA改動原契約；現有libm噪聲／branch需要先作batch API與golden比較。tile級多worker已有充分平行度，不宜每tile再nested parallel。frame tree＋f64 tile origin／f32 offsets現在已存在，不需要為了GPU重新建立另一套座標世界。

GPU compute可優先研究共享index buffer、mesh arena／批次提交、GPU culling／indirect draw；既有f64 sampler與stitching不直接降成f32。完整GPU高度場需仍供應CPU碰撞、LOD error/minmax並處理readback，所以先測dispatch／資料轉移成本，不能因compute shader存在就認定值得。這輪未實作GPU高度場或SIMD sampler。


## 驗收入口

工作區 `/home/pekka/Desktop/void-bevy-lod-profile`，自行編譯 `cargo build -p void-app -j 2`。本輪可直接用 `target/acceptance/void-app-lod-final` 啟動主遊戲；加 `--lod-workers 8` 或 `--lod-frustum-bounds` 開啟對照實驗。預設仍使用原worker配置與NoFrustumCulling，只採用CPU重複工作消除。背景池調整會影響其他async任務，不直接當成最終配置。

可重跑：

```sh
target/acceptance/void-app-lod-final --render-benchmark lab-log/lod-profile/repeat.json --load lab-log/lod-profile/surface-640.world.json --benchmark-frames 120 --width 1920 --height 1080 --benchmark-image lab-log/lod-profile/repeat.png --lod-frustum-bounds
```

model31 direct saves與headless verify語義保持；不改物理owner、collider sampler或frame tree。詳細原始報告、PNG、binary SHA與source manifest存於工作區 `lab-log/lod-profile/`，均ignored。`instrumentation-baseline.patch` 記錄原baseline量測接線，並保留原baseline與candidate執行檔。


## 第二輪延續

後續已加入f64 AVX2／value-only、horizon／tangent cache、U16與render-only預設，以及獨立compute packing量測。第一輪上方數據仍對應`72fd958`，新版storage與驗收binary見[第二輪報告](lod-hardware-review.md)。
