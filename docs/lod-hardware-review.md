# LOD 第二輪：f64 SIMD、GPU packing 與 mesh residency

本輪在 `work/lod-profiling` 沿用既有工作區，基線 `72fd958`。不修改地形配置、LOD 門檻、物理推進、座標樹、存檔格式或原 golden 門檻；保持 model31／world5／FleetCheckpoint12／Craft3。後續小改動由主 agent 自行處理。

## 已採用的主遊戲改動

- `noise()` 明確只算 value，省去未使用的 analytic gradient。
- 使用 gradient 時，x86_64 有 AVX2 的 CPU 以四個 f64 lanes 同時計算三個軸。一次硬體能力檢查後選擇函式；其他平台使用相同運算順序的 scalar 實作。没有 fast-math／FMA 合併／f32 noise。
- 每次 LOD selection 預計算 observer radius 與 horizon angle；node-dependent acos 和判斷順序保持。tile grid 的 tangent／face-axis product 每列／欄只算一次。
- 渲染 tile 在所有 index 可表示時使用 U16；大於 U16 範圍時使用 U32 表示同一拓撲。強制 U16 的不相容要求明確 panic。
- Bevy tile Mesh 使用 `RENDER_WORLD`，上傳後釋放重複的主世界 vertex／index payload。權威 f64 origin／raw tile／碰撞 sampler 保留於 core；Mesh handle、cached AABB 與 GPU ownership 保留。

本輪 CPU 優化與 storage 優化預設啟用；先前的 `--lod-frustum-bounds`、`--lod-workers` 仍為獨立實驗。對照可加 `--lod-u32-indices --lod-main-world-meshes` 保留本輪前的 storage；原 `--lod-u16-indices`／`--lod-render-only` 仍可明示。衝突旗標拒絕。

## CPU 量測與等價證據

AMD Ryzen 7 H 260（8 cores／16 threads），本地 release CPU-only 18 個 33×33 tile，level2／8／14、六個 face。交錯 old／new binaries 六輪，每 backend 排除第一個暖機 batch；舊 terrain 與新 terrain 使用相同當前 LOD 核心。

| terrain backend | 18 tiles batch median ms |
| --- | ---: |
| 72fd958 原 terrain | 29.508 |
| value-only + scalar gradient | 22.309 |
| value-only + AVX2 gradient | 21.728 |

此批總降約26.4%，主要來自省略不用的 gradient；AVX2 再降約2.6%。獨立 gradient microbenchmark scalar18.643 ms／AVX2 15.231 ms，約18.3%。所有 checksum 相同。背景曾有其他工作區 VOID 程序與編譯，原始每 batch／PID／背景負載記錄保留；不把此比例換算成 FPS。

數值核對：20,000 random 與108個 lattice／signed-zero／cell-edge noise 點逐 bit；1024 direction×5 cell 的 layered height／color逐 bit；18 tiles 的 positions／normals／colors／heights／origin／error／height envelope逐 bit；既有 terrain／LOD goldens 未改。dev及release terrain tests皆通過。grid tangent cache另在六face、L0／4／18／21、邊界／中央tile、5／33／65 resolution逐 bit比對原cube mapping；horizon在caps邊界、近表面與1e20尺度比對原判斷。

主遊戲1920×1080、120 delivered GPU frames，固定paused saved scene：v1 traversal p50約0.613／0.507 ms（前後兩個control runs），v2約0.480 ms。此場景的改善較小且受頻率影響，不宣稱select或全遊戲有固定加速比例。

## GPU 實際路徑與 compute 實驗

讀取 pinned Bevy0.19.1 本地源碼並核对執行時：已有 shared mesh allocator slabs、GPU mesh instance preprocessing／indirect draws。本機最大支援模式為 `culling`，GPU preprocessing pass有真實diagnostics；主相機無NoIndirectDrawing。原tile的NoFrustumCulling仍會讓地形跳過部分裁切，不能把支援模式當成每tile已有效裁切。

獨立 `lod_compute_probe` 用與Bevy同locked wgpu29.0.4。GPU只將現有f32 attributes的bit payload從SoA打包成現行48-byte interleaved vertex，並產生相同U32 triangle indices；不是GPU生成高度場。整數load/store驗證包含signed zero與NaN payload，但這些只是獨立transport測試，不進入遊戲物理。

CPU baseline有三種：實際Bevy convenience allocate+pack、preallocated pack+upload，以及allocator實際的 `write_buffer_with` direct staging（每mesh一個vertex＋一個index write）。同device warmup5、repeat30；adapter初始化、SoA輸入gather與allocator growth排除。GPU resident mode不讀回geometry；kernel timestamp的query readback在已量完的total外進行。這仍是probe，不含主遊戲allocator／renderer整合成本。

| resolution／tiles | CPU direct staging total p50 ms | GPU resident total p50 ms | GPU kernel p50 ms | GPU帶geometry readback total p50 ms |
| --- | ---: | ---: | ---: | ---: |
| 33／1 | 0.079 | 0.062 | 0.0066 | 0.081 |
| 33／8 | 0.583 | 0.110 | 0.0091 | 0.205 |
| 33／64 | 3.860 | 0.564 | 0.0375 | 3.624 |
| 65／64 | 11.821 | 1.604 | 0.1256 | 13.625 |

33／64上傳3,066,624 bytes，輸出vertex＋U32 indices 4,918,272 bytes。主遊戲本輪已改U16，所以此probe的U32 input/output比較不能直接當成目前主遊戲精確收益。GPU版把多個tiles合為一個upload；實際收益依ready tiles批量而變。CPU preallocated pack+upload的33／64約2.066 ms，低於direct mapped staging3.860 ms，顯示staging寫入方式亦值得優化。不能只把CPU direct結果當唯一最佳CPU路徑。

結論：resident compute packing已有可觀潛力，下一步需接入Bevy allocation／render asset ordering，並直接寫renderer使用的buffer；把結果讀回再交普通Mesh會失去優勢。此輪不把probe偽裝成已接入主遊戲的compute renderer。f64 sampling／stitching與碰撞仍共用CPU權威實作；現有f64 tile origin＋f32 offsets已符合精度要求。

## 主遊戲 storage 與 ownership

固定1512 rendered tiles：

| storage | 主世界tile payload bytes | GPU slab capacity bytes | allocator allocations |
| --- | ---: | ---: | ---: |
| U32、保留主世界 | 116,194,176 | 219,173,688 | 1652 |
| U16、保留主世界 | 97,614,720 | 203,497,272 | 1652 |
| U16、render-only | 0 | 203,497,272 | 1652 |

indices payload省18,579,456 bytes；render-only釋放重複主世界payload約116 MB。這是arrays的有效payload，不是總process RSS／capacity；原raw LOD約139 MB仍在。GPU slab capacity包含空位與其他mesh，不是live geometry bytes。

Bevy extraction留下Mesh shell與final_aabb；`compute_aabb`使用cache。GPU配置由最後strong handle的Unused事件回收。tile despawn與owned mesh移除仍照原流程，不能保留隱藏strong handles。

實際GPU lifecycle probe經selene／cinder／ares／vesper／aurelia循環三輪，共15次切換。相同天體每輪allocation值一致（334／333／337／328／320），末輪320且render errors空；storage mode slab capacity最後180,162,360 bytes、無逐輪增长。這證明記錄場景未見allocation累積，不概括所有載入／失敗／硬體情境。

v1-before、v2-default、U16、render-only、合併storage、v1-repeat六組固定畫面逐像素相同，完整checkpoint JSON相同；cycle control與storage最終1080p圖片亦逐像素相同；採用新預設後的final-default／原storage control／v1-before仍逐像素相同、完整checkpoint一致。這是headless真實GPU，不是人類GUI驗收。

## 驗證與操作

受影響接縫：terrain→LOD mesh／landing collision→Fleet→app rendering；新增wgpu dev-dependency沿用同locked版本，僅供獨立probe。

- 所屬terrain／LOD／diagnostics共31個dev tests通過；terrain release數值測試與所屬core Clippy通過。
- 最終預設設定的app lib 42 passed／0 failed，含U16 topology、extracted payload／cached bounds、scene unload／switch與存讀／journal。
- landing及Fleet lib/tests（含solar scenery）共156 passed／0 failed；未跑全workspace。
- app lib／tests／compute example Clippy、fmt、diff check。

曾偵測共用target的local crate artefact混入其他分支，該次編譯被停止，沒有拿失敗結果作驗證。已改本工作區獨立target，借用registry dependencies cache後清除並重建所有本地workspace crate；後續binary source manifest綁此分支。

直接執行 `target/acceptance/void-app-lod-v2-final`。本輪前storage對照：

```sh
target/acceptance/void-app-lod-v2-final --lod-u32-indices --lod-main-world-meshes
cargo run -p void-app --example lod_compute_probe -j 2 -- 33 64 30 5
```

GPU lifecycle操作：

```sh
target/acceptance/void-app-lod-v2-final --render-benchmark lab-log/lod-profile-v2/cycle.json --load lab-log/lod-profile/surface-640.world.json --benchmark-cycle-bodies selene,cinder,ares,vesper,aurelia,selene,cinder,ares,vesper,aurelia --benchmark-image lab-log/lod-profile-v2/cycle.png --benchmark-frames 60 --width 1920 --height 1080
```

原始GPU／CPU報告、PNG與checksums在ignored `lab-log/lod-profile-v2/`；noise交錯release資料保存於`target/lod-noise-profile/`。本輪未merge/push，未宣稱完成人類最終GUI驗收。
