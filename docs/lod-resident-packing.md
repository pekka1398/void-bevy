# 主遊戲 resident GPU packing 實驗

承接 `6fa08d0`，第三輪成果保存在 `work/lod-profiling` 分支。CPU 預設路徑保留；`--lod-gpu-pack` 才啟用 compute，`--lod-gpu-pack-verify` 另做逐位元讀回核對。兩者不能搭配要求保留一般 CPU mesh attributes 的 `--lod-main-world-meshes`。

## 範圍與接縫

f64 sampler、座標樹、tile origin、stitch 與碰撞都沿用原 core。GPU 僅把既有 f32 SoA 打包為 renderer 使用的 48-byte vertex，並生成原 U16/U32 grid indices。每 vertex 上傳 40 bytes；alpha、cell 與 indices 在 GPU 生成。主遊戲直接使用 Bevy MeshAllocator 的 resident slabs，不把 compute 結果讀回後再上傳普通 Mesh。

兩個永久 invisible prototype mesh 提供 pinned Bevy 的公開 layout metadata。新 tile 是 MAIN_WORLD metadata Mesh，另由自訂 RenderAsset 持有上傳 bytes 與 mesh strong handle；render-only source bytes 提取後釋放。標準 mesh asset／allocator 路徑仍負責最後 handle 消失後的回收。compute 在 camera driver 前寫完所有元素。一般 slab 透過公開 `copy_element_data` 的 zero-length 路徑發布配置，不產生 staging upload；large-object slab 暫時明確拒絕，不偷偷走 CPU fallback。此接線依賴 Bevy 0.19.1 allocator 語義，升級須重新核對。

輸入依 storage binding limit 分批；大 slab 使用 aligned window。uniform 使用裝置要求的 dynamic offset alignment。相同 vertex/index buffer window 的 bind group 在同批共用；這項後續修改尚待 runtime 重測。pipeline 尚未完成合法 async 載入時等待，其餘錯誤明確報錯。驗證 readback 在 Submit 後啟動，正常遊戲不做 geometry readback。

## 已取得的正確性證據

`lab-log/lod-resident/verify-fourth.json`：固定 model31 surface saved world，640×360。3,335 個生成 tile 的 vertices／indices 全部逐位元一致，pending／verification pending 最終為零，render errors 空。GPU image 與同 source binary CPU-control image 逐像素相同，兩份實際 `.world.json` checkpoint 完全一致。

`cycle-verify.json`：selene／cinder／ares／vesper／aurelia 三輪共 15 次切換，9,034 個 tile 全部驗證一致。每次回到同天體，allocator allocation counts 重複一致：336／335／339／330／322，包含兩個永久 prototype；未見逐輪配置數增長。末次 pending 為零、render errors 空。slab capacity 保留 203,497,272 bytes，這是 allocator 高水位容量，不能說已釋放至場景的最小容量。

app lib 44 tests 通過，含 transport／bounds／cell bits 與 ticket clone 併發取消／完成核對；後續 bind group 共用與新增 CPU telemetry 修改須再驗證。這些是 headless 真實 GPU 與測試，不代替人類 GUI 驗收。U32、expanded bounds 與高頻動態取消仍需補測。

## 測時限制與資源事件

暖機 GPU diagnostics 已與穩定採样分開，能看到 `render/lod_pack/elapsed_gpu`；穩定場景本來不再生成 tile，不可只用稳定 FPS 評估 packing。累計 render-side prepare／encode CPU 時間與 bind group 計數已加入待測版本。主世界 `lod_create_mesh` 包含 SoA 生成，不能忽略 render-side staging、allocation 與 command encoding。

最初 CPU-control／GPU-no-readback 同時存在編譯等工作，冷啟動的 5.4／5.7 秒與 pass timings 不當作可採用的性能結論；需要串行交錯重測。

2026-10-09 19:25:20，systemd-oomd 記錄 VS Code `app-code-892075.scope` 使用 10.5G，Avg10 pressure 63.85%；user service 壓力超過50%逾20秒，整個scope被殺。19:24:37另有NVMe WRITE timeout。紀錄沒有各PID歷史RSS，不能單憑此數字判定單一rustc／linker用量。後續昂貴工作串行執行，ignored `target/lod-resident-guard.py` 每秒記錄本次明確啟動process group的PID／RSS與MemAvailable／full PSI；低於2GB可用記憶體或full Avg10超過10即中止該組，不處理其他進程。測量存於`lab-log/lod-resident/*-guard.csv`。

## 待完成的判斷

串行 CPU／GPU ABBA、U32與bounds逐位元／畫面核對已補測，見下節。camera movement／refinement／eviction下的queue wait與尖峰、worker配置對主幀的影響仍需量測。GPU packing 保持 opt-in，直到整體成本證據支持採用；kernel快不等於主遊戲快。

## 串行補測

`v3-verify` 包含 U32 indices 與 expanded sea bounds，3,401 tiles 逐位元通過、pending零、render errors空，圖片／checkpoint仍與CPU control完全一致。共用bind group只建立97個groups。其後用Bevy穩定BufferId作cache key取代wgpu handle，避免mutable-key lint；app lib／tests scoped Clippy通過。最終`final-verify` binary重測3,423 tiles全部一致、groups73、pending零、render errors空，圖片／checkpoint與ABBA CPU A1完全一致；最終app lib44 tests通過，fmt／diff check通過，未跑全workspace。

guarded串行ABBA (`abba-a1,b1,b2,a2`)，同saved world、640×360、60 delivered frames；沒有編譯與GPU run重疊。四次圖片逐像素一致、實際checkpoint完全一致、full memory PSI Avg10最高0.01。

| run | 冷啟動 settle ms | 生成 rendering tiles | 主世界 mesh conversion 累計 ms | GPU prepare／encode 累計 ms |
| --- | ---: | ---: | ---: | ---: |
| CPU A1 | 5395.13 | 3487 | 43.53 | 不適用 |
| GPU B1 | 5223.88 | 3474 | 42.82 | 15.05／69.24 |
| GPU B2 | 5597.83 | 3463 | 44.75 | 14.24／93.35 |
| CPU A2 | 5476.48 | 3398 | 40.06 | 不適用 |

GPU groups73／74，packed數等於main conversion數，沒有verification讀回。非同步build完成順序造成seam rebuild數不同，所以需同時列tile數；不能把這些冷啟動秒數當純packing時間。CPU ordinary Mesh render-side extraction／allocation／packing未以相同累計計量單獨拆出，不能拿GPU總和與CPU main conversion單項直接比較。現有證據未顯示穩定整體優勢，GPU packing保留實驗，不改預設。

此次獨立build的rustc RSS峰值約1.4GiB，預設rust-lld約3.5GiB時guard因full PSI10.76中止；改bin-only `cargo rustc -p void-app --bin void-app -j 2 -- -C link-arg=-Wl,--threads=2`後成功。兩threads linker峰值約3.3GiB，並非記憶體變成很小。對845MiB debug binary做objcopy另引發壓力，該工作已由guard中止；驗收binary以hard link保存，避免額外大檔複製。不把這些資源事件混入效能結論。

最終app lib測試使用guarded `taskset -c 0,1 cargo test -p void-app --lib -j 2`，限制本次compiler／linker／tests的CPU affinity，避免大量linker threads同時觸碰檔案。44 tests通過，linker每秒RSS樣本峰值約2.1GiB、full PSI0。此affinity只用於驗證，ABBA效能run未限制CPU。

直接執行驗收程式`target/acceptance/void-app-lod-resident-final`；加`--lod-gpu-pack`開啟實驗。ignored源碼manifest／報告在`lab-log/lod-resident/`。沒有merge／push或人類GUI最終驗收。
