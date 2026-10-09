# GPU packing 的批次 dispatch

承接`bf42cb9`。`--lod-gpu-pack-batch`是獨立GPU實驗入口：每組相同vertex/index storage window的tile使用32-byte Params storage array，二維dispatch的Y選擇tile、X處理其vertices/cells。一次group含不同resolution也按最大vertex count dispatch，各job用自己的n/vertices界限；input總大小和dispatch Y均依device limits分批。需要4個storage bindings；原每tile dynamic-uniform路徑需要3個。沒有另作地形、座標或seam計算，兩路都只傳原f32位元。

同120-update camera路徑、640×360、180 delivered captures、pipelined rendering，串行guarded ABBA，正常run沒有geometry readback：

| run | packed tiles | dispatch calls | GPU pack p50／p95 ms | main interval p50／p95 ms | CPU encode累計ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| per-tile A1 | 10862 | 10862 | 0.270080／0.395776 | 10.952／15.241 | 233.34 |
| batch B1 | 11002 | 227 | 0.034816／0.048128 | 11.503／15.965 | 230.25 |
| batch B2 | 11063 | 227 | 0.034560／0.048640 | 11.717／16.053 | 230.57 |
| per-tile A2 | 11017 | 11017 | 0.261376／0.389888 | 11.289／15.487 | 235.08 |

GPU kernel約快7.5倍，CPU encode總成本卻近似；並行利用率改善沒有變成主幀改善，batch main interval在此對照甚至略大。因此batch仍opt-in，沒有把component加速說成遊戲FPS提升。tile數受非同步完成與camera每update步進影響，表中明列，不把不等工作量當完全固定batch。

五組motion資料（ABBA＋verify）圖片逐像素相同、完整checkpoint相同、render errors空。batch verify11,049tiles全位元通過、dispatch244、pending零。另在render thread下用U32／sea bounds、5天體三輪15次切換，9,222tiles全位元通過、dispatch296、pending零；同一天體allocation counts每輪相同336／335／339／330／322，包含永久prototypes。slab capacity保持220,222,264 bytes，是高水位capacity，不能說回到最小場景容量；最終圖亦與第三輪同cycle對照相同。

app lib45 tests、scoped Clippy、build/fmt/diff check通過。沒有改core或physics／存檔版本、沒有跑全workspace。驗收binary `target/acceptance/void-app-lod-batch-probe`；正常GPU預設仍是原per-tile路徑，一般遊戲預設仍是CPU Mesh。原始證據／manifest在`lab-log/lod-batch/`；沒有merge/push或人類GUI最終驗收。

CPU prepare／encode與GPU timestamp分開，kernel span不含所有前置配置、staging或回收。參數buffer與input每批建立，CPU metadata收集／Assets／allocator／buffer上傳仍存在；現有證據支持先改善這些CPU／資料流成本，未支持改GPU為預設。不能把這次dispatch結果推廣成GPU f32重新取樣f64 terrain的正確性證明。
