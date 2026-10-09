# LOD profiling 的 render thread 與 queries 對照

承接`8a50efd`。第四輪離屏預設關閉Bevy pipelined rendering，因此只代表串行main＋render配置，不能直接當主遊戲FPS。主遊戲原本保留pipelined plugin。本輪增加`--benchmark-pipelined`供相同saved world與camera序列使用原render thread；不改一般遊戲配置。

`--benchmark-no-pipeline-statistics`只關閉pipeline statistics，保留timestamps與source-frame GPU數值標記。裝置實際features另在report.adapter記錄，沒有假造缺失的GPU指標。兩個控制旗標只接受搭配`--render-benchmark`。

## 串行／並行與worker對照

同640×360、120 camera updates、180 delivered captures、paused physics；資源guard與串行run，沒有與編譯重疊。此处的main interval是離屏update間隔，沒有視窗presentation／vsync或完整飛行physics負載。原始資料在`lab-log/lod-pipeline/`。

| 配置 | main interval p50／p95 ms | submit→accept mean ms | 動態raw tiles completed |
| --- | ---: | ---: | ---: |
| serial CPU4、queries全開 | 18.174／23.370 | 19.341 | 3337 |
| serial CPU4、statistics關閉 | 17.997／23.571 | 19.167 | 3334 |
| pipe CPU4 | 11.388／16.257 | 16.046 | 2942 |
| pipe CPU4 repeat | 10.895／16.974 | 16.026 | 2864 |
| pipe CPU4、statistics關閉 | 10.793／17.319 | 15.895 | 2941 |
| pipe GPU pack4 | 11.427／15.492 | 15.659 | 2967 |
| pipe GPU pack4 repeat | 11.323／16.312 | 15.932 | 2966 |
| pipe CPU8 | 11.366／17.020 | 12.542 | 3323 |
| pipe CPU16 | 14.393／26.215 | 17.407 | 3348 |

Pipeline本身讓main與render重疊，這是原本遊戲已有的能力，不能宣稱本輪替主遊戲加快到此比例。此實驗也顯示第四輪串行case的GPU pack正面訊號未延伸為pipe配置的穩定main throughput優勢；所以GPU packing不改預設。statistics開／關在此case的差異小於serial／pipe，不把query數字當無成本，也沒有證明所有profiling overhead為零。

8 workers在pipe配置改善生成接受延遲，但main throughput未明顯改善；每tile build p50約2.42ms，對照4 workers約2.11ms。16 workers build p50約2.85ms，main p95也明顯較大。camera是按update步數移動，pipe的相同路徑在較短wall time內走完，所以不能忽略完成tile數不同或把这些行當固定物理速度的飛行測試。既有observer／cache与非同步ready進程仍按原算法運作。

所有10種成功配置的終點圖片逐像素相同、完整checkpoint相同，render errors空。pipe＋GPU verify額外11,038 tiles全位元通過、pending零；verify run有額外expected arrays／GPU readback，沒有放入速度表。render thread下的SourceAsset／RenderAssets／allocator接線確實驗證過，不只在串行renderer可用。

## 完全關閉GPU queries的失敗實驗

初版控制同時關閉timestamp與statistics，觸發pinned Bevy 0.19.1的numeric readback問題：`diagnostic/internal.rs::FrameData::finish`在沒有query read_buffer時直接讀`value_buffers.get_mapped_range`，而這些buffer透過`map_buffer_on_submit`非同步映射，尚未完成時wgpu明確報`Buffer is not mapped`；panic清理階段該程序exit -11。記錄`cpu-serial-guard.log`，沒有性能資料。未改registry、沒有關掉驗證、沒有把失敗結果當CPU基線。

此控制已移除，改成明示statistics-off。RenderMetrics plugin現在於初始化核對至少一種GPU query可用，否則給明確原因與CPU `--profile`入口，避免讓已知不相容的source-frame numeric marker途徑跑到mapping錯誤。這個拒絕只影響明示render profiling；正常遊戲或CPU profiling不需要該query set。完全零queries的throughput仍未測得，不能宣稱已量到零diagnostic overhead。

## 驗證與入口

app scoped Clippy、build、最後app lib44 tests通過，實際pipeline／query控制與GPU位元驗證如上。沒有跑全workspace或改LOD/core/physics。此輪binary `target/acceptance/void-app-lod-pipeline-final`，源碼manifest存於同證據目錄。

```sh
target/acceptance/void-app-lod-pipeline-final --render-benchmark lab-log/lod-pipeline/repeat.json --load lab-log/lod-profile/surface-640.world.json --benchmark-lod-motion 120 --benchmark-motion-body aurelia --benchmark-frames 180 --benchmark-settle 20 --benchmark-pipelined --width 640 --height 360 --benchmark-image lab-log/lod-pipeline/repeat.png
```

未merge/push、未有人類GUI最終驗收。下一步是檢查可以省掉的ECS變更標記，以及GPU packing每tile小dispatch的並行利用率；不因存在compute shader就推定應取代CPU路徑。
