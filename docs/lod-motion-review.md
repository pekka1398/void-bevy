# 動態 LOD、排隊與平衡掃描

承接`128aa81`，本輪仍在`work/lod-profiling`独立工作區。新增profiling與可重播的camera路徑，不改terrain／LOD門檻／物理／存檔版本。主要待審優化是只在輸入本身已證明平衡時跳過鄰居掃描。

## 量測與場景

`lod_worker_queue_wait`：提交到worker開始。`lod_worker_build`：worker內純生成。`lod_worker_finish_lag`：生成完成到主世界取回。後者包含主update週期，不是worker仍在執行。只有明示profile時才記錄clock／samples；每次提交／完成的三段有相同樣本數，可加其mean得到mean提交→取回延遲，不能直接相加三個p50／p95。

`--benchmark-lod-motion 120 --benchmark-motion-body aurelia`在正式capture內用同一組120次Action改變camera：方向繞Y轉0.4 rad，距離在1.0005–1.0025 reference radius之間變化；使用f64 ViewCommand及原座標樹，物理保持paused。正式capture至少要有同數delivered frames。路徑結束後等待build/request與GPU pending清空，再取終點圖片/checkpoint。這是可控camera壓力場景，不能說已量到完整有推進／碰撞／orbit負載的實際飛行FPS。

串行、有資源guard、同640×360 saved world、180 delivered frames；無編譯與GPU run重疊，未開detailed trace或draw-count readback。原始證據`lab-log/lod-motion/`，摘要`summary.json`；圖與完整checkpoint在8種baseline/control設定全部一致，render errors空。

## workers與接受延遲

| 設定 | queue p50／p95 ms | build p50 ms | main interval p50／p95 ms | 提交→取回 mean ms |
| --- | ---: | ---: | ---: | ---: |
| CPU 4 | 6.35／14.32 | 2.07 | 18.04／22.78 | 19.01 |
| CPU 4 repeat | 6.33／14.44 | 2.08 | 17.74／22.26 | 18.80 |
| CPU 8 | 3.22／7.97 | 2.37 | 18.38／24.19 | 19.58 |
| CPU 8 repeat | 3.14／7.83 | 2.32 | 17.96／23.16 | 18.92 |
| CPU 16 | 2.23／5.17 | 2.91 | 19.56／24.61 | 20.51 |
| GPU packing 4 | 6.41／14.47 | 2.13 | 16.34／20.25 | 17.57 |
| GPU packing 4 repeat | 5.91／13.59 | 2.02 | 16.51／19.75 | 16.69 |

更多workers縮短queue，卻讓per-tile生成略慢；worker8完成更早後，大部分時間仍等主世界下一次accept。此場景8／16未展現穩定主幀優勢，所以不把worker預設改成吃滿logical threads。保留`--lod-workers`供機器／場景個別核對。被capture的main updates約182–183，動態期CPU生成約3338–3348 raw tiles；因背景完成順序，不能把每次render mesh／seam rebuild數當作完全相同。

GPU packing在第三輪冷啟動未有穩定優勢，但這條持續換塊路徑兩次main intervals較低。這是具體場景的正面訊號，不能從兩次run推出所有GPU／遊戲場景速度保證；保持opt-in。開verify的run有額外CPU expected arrays與GPU readback，不納入性能對照。動態verify版11,236個tile全部逐位元通過，pending零；非verify版完全沒有geometry readback。

raw tile cache此路徑峰值約294MB，包含當前camera與既有observer／近期地區的取樣；與MeshAssets重複payload、GPU slab capacity分列，不能混稱VRAM。動態期約3,800次接縫重建仍造成很多asset churn，是後續render快取／buffer更新路徑的明確候選；cache key須包含鄰居版本、sampling與world owner，不能只存16種edge mask或直接重用不同場景資料。

## 已平衡拓撲快取

cache只保存上一個**已平衡輸出**的ordered key vector。下一次raw walk與其逐項相同時，所有neighbor level gaps仍≤1；原balance在此輸入下不會refine／collapse／插入balancing requests，因而可跳過掃描。新增ready children無法改變這個事實。若raw input不平衡，無論它與上一個raw是否相同都走完整算法。prefetch、traversal、last-used frame、eviction與camera anchor全都繼續執行。

`--lod-full-balance`是完整掃描對照；`lod_balance_cache_hits`記錄每次select是否命中。API設定不是LodOptions／serialized world欄位，沒有版本或模型規則改動。core differential測試440 frames比對ordered render、request priorities bits／order、collapses、visited／culled、node/cache counts，包含移動、每次最多17個非固定順序completed tiles、coarsening與eviction。原geometry／mesh／selection golden也通過。

1920×1080、60 delivered frames、static ABBA：

| full/cache | balance p50 ms | p95 ms | cache命中 |
| --- | ---: | ---: | ---: |
| full A1 | 0.364717 | 0.572099 | 0／62 |
| cache B1 | 0.003206 | 0.012173 | 63／63 |
| cache B2 | 0.002846 | 0.010219 | 62／62 |
| full A2 | 0.383192 | 0.521352 | 0／62 |

四次pixel與完整checkpoint相同。這是balance component約99% reduction，不是整體FPS99%提升。動態control的balance mean0.19255ms，cache0.13405ms（62／183次命中），moving時p50／p95幾乎相同，符合只對重複拓撲省工作的設計；未宣稱所有動態幀變快。cache＋GPU verify的11,257tiles全位元一致、pending零、errors空，三種motion cache/full/GPU終點圖片/checkpoint均與原motion baseline相同。

## 驗證與剩餘工作

已跑所屬LOD core tests/golden（6 tests，另重跑新增coarsening覆蓋assert的440-frame differential）、app lib44 tests、landing／Fleet lib/tests156 passed；app與LOD scoped Clippy、fmt／diff check。受影響接縫是LOD selection→app→landing／Fleet；沒有跑全workspace、merge/push或人類GUI最終驗收。

後續需把本輪證據收斂到完整優化優先順序，核對rapid scene取消／ownership與診斷對render成本的影響，並保留可直接執行的驗收入口。GPU采樣地形仍須維持原f64 sampler、band limit、FDlibm／運算順序與碰撞一致性；現有GPU packing不宣稱取代terrain sampler。

本輪binary為`target/acceptance/void-app-lod-balance-probe`。直接開主遊戲可使用預設cache；`--lod-full-balance`作CPU對照。量測操作：

```sh
target/acceptance/void-app-lod-balance-probe --render-benchmark lab-log/lod-motion/repeat.json --load lab-log/lod-profile/surface-640.world.json --benchmark-lod-motion 120 --benchmark-motion-body aurelia --benchmark-frames 180 --benchmark-settle 20 --width 640 --height 360 --benchmark-image lab-log/lod-motion/repeat.png
```

`--lod-workers 8`、`--lod-gpu-pack`為獨立實驗旗標；`--lod-gpu-pack-verify`只用於正確性讀回，不拿它的耗時當正常遊戲成本。源碼manifest與原始資料保存在ignored `lab-log/lod-motion/`。
