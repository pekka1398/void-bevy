# LOD 效能探索結論與驗收

基線master `95c0581`；成果在`work/lod-profiling`，工作區`/home/pekka/Desktop/void-bevy-lod-profile`。本次範圍為LOD的生成、選擇、接縫、ECS呈現、GPU資產／上傳／draw與ownership。没有改orbit／物理規則、地形模型、LOD門檻或存檔版本；沒有merge／push。人類GUI最終驗收另列，不由headless圖片代替。

## 採用與保留的路徑

| 項目 | 結論 | 證據與界限 |
| --- | --- | --- |
| f64 terrain計算 | 採用value-only noise、AVX2 gradient、row/column tangent與horizon常數快取 | scalar／AVX2位元與原golden核對；18-tile release微測29.508→21.728ms，不能說FPS提高26% |
| selection／seams | 採用不變接縫拓撲重用、已證明平衡輸出快取 | static balance p50約0.36–0.38→0.003ms；440-frame full-scan oracle包含coarsening／分批完成／eviction；moving時p95沒有明顯改善 |
| CPU mesh資料 | 採用stitch結果截斷後直接移交、U16 when fits、render-only Mesh | 原grid拓撲／attributes、pixel／checkpoint相同；固定1512 tiles重複主世界payload約116MB→0，U16少約18.6MB indices；authoritative raw tiles仍保留 |
| anchors／ECS | 採用相同f32位元不寫回Transform | 仍逐幀做f64相對量；signed-zero與Changed測試；static每update1512 writes→0；没有穩定FPS提升證據 |
| workers | 預設保留；8可作接受延遲選項，16不推薦由此case採用 | 原Bevy async pool4；pipe mean提交→接受約16ms，8約12.5ms但main約11ms近似，16 main p95約26ms；paused camera case，不含完整physics競爭 |
| render thread | 主遊戲已有，量測必須保留對照 | serial headless main約18ms、pipe約11ms；这是原遊戲的並行能力，不是本輪新增的遊戲加速 |
| GPU resident packing | 留opt-in | 直接寫renderer slabs、正常無geometry readback；pipe實際main CPU/GPU兩路均約11ms，沒有穩定整體優勢 |
| GPU batch dispatch | 留opt-in | GPU kernel p50約0.26–0.27→0.035ms、約7.5倍；CPU encode累計近似、main沒有改善。證實並行利用率，而非目前主瓶頸 |
| frustum bounds | 留opt-in | conservative sea displacement＋f32 guard，固定GPU primitive量少約70%，固定pixel一致；未把有限視角資料推成全海岸／所有鏡頭的人類验收 |

GPU尚未做f32重新取樣terrain。當前Core／coordinate tree／tile origin都是f64，渲染在相機附近取相對量再轉f32；這套已是GPU可用的tile-local表示，packing傳的是已求好的f32位元。把FDlibm、band limit與coarse seams改成另一套GPU sampler會增加接口／一致性成本，現有整體測時未支持優先如此重寫。不能把packing bit-exact說成GPU重新取樣也已正確。

## profiling 的覆蓋與判斷

已分開traversal、balance、eviction、schedule、worker queue wait／純build／finish lag、draw／mesh conversion、seam rebuild與payload counters。GPU prepare／encode CPU總量、input/output bytes、groups／dispatches／verification pending另列；GPU pass timestamps／pipeline statistics與warmup／stable分開，source-frame標記匹配後才聚合。component有包含關係，不把它們或跨thread累計wall time加成FPS。

冷啟動、fixed scene、camera movement／refinement、五種天體的15次穩定切換、每三update的15次快速切換、CPU／GPU／U16／U32／bounds、serial／pipe、4／8／16workers及batch/per-tile均有實際資料。快速切換在CPU／GPU分別提交1128／1144 raw jobs、只接受702／704；最後沒有pending，说明取消／丟棄舊scene工作實際發生。GPU快速版4161tiles全部位元核對、終點pixel與完整checkpoint等於CPU，allocation320／322差額為兩個永久prototypes。此probe沒有製造GPU尚未prepare的SourceAsset cancellation（canceled counter0）；ticket clone cancel／complete競態由所屬測試覆蓋，不虛稱觸發了所有GPU失敗路徑。

主瓶頸優先順序因此明確：保留便宜且有等價證據的CPU／資料重複消除；按場景評估worker接受延遲；更大的renderer／terrain改寫必須同時量extract、metadata、allocation、staging、encode與整條pipe，不能只看很快的kernel。每次seam改變仍重新建mesh／entity，有界seam-version cache或in-place更新是下一級研究候選；它必須有完整owner／neighbor版本／bounds與回收契約，目前不以未測方案替換可靠路徑。

## 正確性與限制

- 原stored geometry／mesh／selection golden保留、門檻未放寬；f64 scalar／SIMD數值、stitched attrs／U16拓撲、balance全掃描oracle與ECS變更語義核對。
- 最後LOD核心6 tests；受影響landing／Fleet lib/tests156 passed（core cache接縫版本）；app最後45 tests、scoped Clippy及fmt/diff checks。後續只改benchmark／render packing／anchors，沒有新增core模擬規則。未跑全workspace。
- 各輪GPU全位元數據與pixel／完整checkpoint在各報告注明。U32/bounds/batch/pipe的15-cycle9222tiles、motion11049tiles、快速4161tiles皆通過；allocation重複循環不增長。高水位slab capacity不是live bytes或總RSS，也不概括所有硬體。
- PMU受`perf_event_paranoid=4`限制，未改系統設定。headless、paused、NVIDIA Vulkan／此CPU的結果不能宣稱所有硬體或完整遊戲FPS。正常視窗有presentation／vsync，人工遊玩验收尚未完成。
- 完全無GPU queries時pinned Bevy numeric marker mapping會失敗，記錄失敗並明確拒絕不相容render profiling，沒有偽造CPU基線；CPU `--profile`仍可用。statistics-off對照保留timestamps，沒有證明所有診斷成本為零。
- 19:25 VS Code scope被systemd-oomd殺掉；後續重型工作串行、-j2、linker2threads或驗證CPU affinity，按owned numeric process group記錄RSS／PSI並提前中止過高壓力工作。沒有改使用者其他進程或master未提交檔案。

以上有限測量已足以決定此輪採用／保留路徑；沒有剩餘必需的LOD實驗才能支撐这些決定。仍有可研究的下一級方案，不把它们說成已完成的優化或無需驗證的改寫。

## 可重跑與交付

直接啟動主遊戲：`target/acceptance/void-app-lod-review-final`。一般預設只採用上表CPU／storage／cache／anchor改動。對照旗標：`--lod-full-balance`、`--lod-always-update-anchors`、`--lod-u32-indices --lod-main-world-meshes`。實驗：`--lod-workers 8`、`--lod-frustum-bounds`、`--lod-gpu-pack`、`--lod-gpu-pack-batch`；`--lod-gpu-pack-verify`有額外讀回，不能作正常速度對照。

```sh
target/acceptance/void-app-lod-review-final --render-benchmark lab-log/lod-final-repeat.json --load lab-log/lod-profile/surface-640.world.json --benchmark-lod-motion 120 --benchmark-motion-body aurelia --benchmark-frames 180 --benchmark-settle 20 --benchmark-pipelined --width 640 --height 360 --benchmark-image lab-log/lod-final-repeat.png
```

原始reports／images／source manifests／binary hashes在ignored `lab-log/lod-profile*`、`lod-resident`、`lod-motion`、`lod-pipeline`、`lod-anchors`、`lod-batch`、`lod-rapid`；不要從另一分支共用target盲信本地crate快取。詳細分輪：[初步](lod-performance.md)、[SIMD/storage](lod-hardware-review.md)、[resident](lod-resident-packing.md)、[motion/balance](lod-motion-review.md)、[pipe](lod-pipeline-review.md)、[anchors](lod-anchor-review.md)、[batch](lod-batch-review.md)。

人類验收可在主遊戲旋轉／移動camera、靠近地表／海岸、切換天體，核對LOD接縫／海面／載具附近呈現；需要比較時使用上方baseline旗標。此交付沒有代替使用者驗收，也沒有合入master或push。
