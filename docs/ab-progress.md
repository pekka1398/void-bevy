# A／B 完成核對

本次目標：完成 A（assembly＋vessels 整合主遊戲與完整飛行世界存檔）和 B（profiling、真實碰撞疊圖、record/replay、差分／不變量與必要觀察接口）。依使用者要求，實作、headless 檢查與整合繼續完成；使用者視窗驗收留待之後補做，agent 不開啟或操作 OS 視窗。

## 實作與證據

| 要求 | 實際接線與驗證 | 範圍／限制 |
| --- | --- | --- |
| A：assembly＋Fleet 成為主遊戲基礎 | `app/src/main.rs` 與 `fleet-flight-lab/src/main.rs` 都進入 `fleet_game::run`。main 正式依賴 assembly／Fleet；`--craft` 接受 assembly 匯出的 Craft JSON，渲染任意 catalog parts。`fleet-flight/tests/integration.rs` 驗證自訂 craft 分級、逐船油門／SAS、air、pressure／燃料、三 owner 阻力差分與步長收斂 | assembly 編輯器保留獨立 lab；主遊戲讀取其 craft。舊 PartJointRocket 保存為 `legacy_flight` 回歸 example |
| A：保留主遊戲能力 | 共享 runtime 接回 navball、地形／大氣／海／雲／星空、多天體 map、標籤焦點、連續 coast prediction、逐船 PlanEngine、機動編輯／apsis／導引執行、九級 warp gates 與機動前攔截。`fleet_game` renderer-free tests 驗證 main scene／navball、真實鍵盤機動／warp／相機接線；`guidance.rs`／`plans.rs`／`warp.rs` 驗證燃燒邊界、flameout、切船、手動／contact 中止及保存續跑。真實 Image GPU benchmark 也驗證正式 SceneryPlugin、UI／大氣 pass 啟動 | 導引沿用既有理想姿態模式；有限轉向／RCS／真正 docking capture 是後續功能。舊 main 從未啟用 crash_detection，毀損不是整合遺失 |
| A：完整 live 世界存讀 | `FlightCheckpoint` 保存 Fleet graph／fuel／controls／SAS／guidance／pending、orbit／ground／bubble owners 的完整 native caches、計畫、warp intent、presentation；原子寫檔、catalog／model／Rapier ABI 驗證。`checkpoint.rs` 檢查 awake ground、混合 owners、睡眠一天 rails、native cache／graph 損毀拒絕；guidance／presentation tests 覆蓋 active burn／camera。`app/tests/fleet_cli.rs` 由正式 binary 跨程序 verify-save；相同命令續跑要求完整 mark 相等 | 沒有跨模型／Rapier 版本 migration；不以重播飛船輸入替代直接世界存檔 |
| B：操作錄放與自動回歸 | Fleet Action journal 記錄 control／stage／spawn／select／plan／warp／camera／view／EndFrame；marks 比對所有船、parts、owners、graph、fuel、SAS、bodies、plans、presentation。core session／presentation tests 驗證增量 replay、暫停 frames、修改輸入拒絕、續玩。main renderer-free 新增自動 orbital prediction 反覆刷新後仍可 headless replay／direct restore，並續跑完整 mark 相同 | semantic commands 是 replay 的權威輸入；重播不依目前 wall delta 或 OS 鍵盤 |
| B：崩潰與重設／載入後錄製 | Intent 在執行前 sync，Commit／Mark／End 也 sync。`durable.rs` 與正式 binary 的 CLI test 用真實獨立程序崩潰，驗證 unfinished intent、torn EOF、完整 malformed line 拒絕、ResetWorld／LoadWorld 不丟既有 journal。`--recover-recording` 保存可驗證的 committed prefix 和未完成操作報告 | 恢復需顯式命令，不猜測／執行未提交的操作；F8 正常完成錄製且可繼續遊戲 |
| B：真實碰撞疊圖與觀察接口 | main／lab 共用 F2 mesh wire、F3 tile boundaries、F4 native colliders、F5 terrain visibility。船體由 Fleet→ContactWorld 讀 native shape／local transform，地形由實際 collider triangles 讀回。`landing/tests/contact.rs::body_overlay_reads_live_collider_shape_and_local_transform` 故意改 native shape／local pose，並驗證 recenter；main scene tests 建立疊圖。state／fuel／rails blocker／owners／collider APIs 都是正常公開觀察介面 | 線條可見性、視覺對齊和實際操作手感留給使用者視窗驗收 |
| B：profiling | `void-diagnostics` 提供 CPU system wall spans／traceEvents 與 p50／p95；main／lab 的 `--profile`、F9 與 headless `--verify --profile` 共用。RenderDiagnostics 以 GPU frame tag 對齊非同步回讀，保留每 pass CPU／GPU／pipeline statistics；render-metrics feature 回讀實際 indirect count slots。`--render-benchmark` 提供完整 main surface／orbit／map 的 settle→run→drain、場景 checkpoint 與 CPU／render reports。原生 GPU test 驗證 orbit 與 saved-scene 重測；surface／map 已實際離屏執行，UI／大氣／draw 指標存在且 errors 為空 | GPU query 依硬體能力，缺值不補零；重疊 pass 不相加，feature instrumentation 有成本。本機 perf wrapper 可用但 kernel paranoid=4 拒絕 native stack sampling；CPU／GPU 管線已有實測，未更改 kernel 設定 |
| B：接縫差分、不變量與失敗 corpus | landing/seams 覆蓋 frame／equations／floating-origin／rails／contact handoff；assembly 分離掃描；Fleet flight air owner 差分。新增 `void-seam-check` core＋CLI，六家族涵蓋 Fleet 分離／本地 join／遠方 FrameEphemeris join／orbit-bubble-orbit／split-frame／Traveller-direct N-body；1200 案例通過且不放寬既有門檻。實際輸入／panic reason sync 保存；單檔／corpus CLI、跨程序失敗重跑、export-before-execution、i128 字串及 NaN 拒絕均有測試 | 掃描覆蓋的船型／參數／時間範圍見 [seam-check.md](seam-check.md)，不宣稱任意狀態無 bug |

## 本次驗證

- `cargo test --workspace --all-targets -j 2`：**281 passed、0 failed、4 ignored**。其中一項為明確 opt-in native GPU test，其餘保留原有 ignored 檢查；沒有放寬 TS 門檻。
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings` 與 `cargo fmt --all -- --check` 通過。
- native render-metrics GPU test 已另行執行並通過，surface／map preset 亦在 RTX 5060 Laptop／Vulkan 上實際離屏完成；正式 main UI／大氣 pass、GPU count-buffer readback 與 scene checkpoint 重測都有證據。它使用 Image，未建立 OS 視窗。
- `void-seam-check --count 200 --seed 24301`：**1200 passed、0 failed**；不混入 workspace test count。
- 最後補充的 main orbital prediction／replay／checkpoint 續跑測試通過，彌補僅在 pad 上測 renderer cadence 的較弱證據。

A／B 的程式與上述驗證已完成。使用者 GUI 驗收仍未替代：手動飛行、不同 edge cases、線框可見性／對齊、外觀／拖曳／不同 GPU／平台。這些依原要求之後補做；發現問題後可使用 journal、direct checkpoint 與 seam corpus 重現並加入回歸。

後續對接／RCS、EVA、車輛、視覺精修、效能優化、科技樹／主線等，不是本次 A／B 的完成條件。存檔與驗收工具現在可供那些功能直接使用。

後續修正：main／Fleet lab 預設船已換為 assembly 格式的原 7620 kg／9.6 km/s 火箭。命令 journal 改為 opt-in：沒有 `--record` 就不保留命令／marks，F8 後釋放紀錄，詳見 [fleet-flight.md](fleet-flight.md)。

本次修正驗證：workspace all-targets 283 passed、0 failed、4 ignored；最終新預設船的 app library 15 項全通過；workspace all-target Clippy（`-D warnings`）與 fmt 通過。GUI 驗收及 commit／push 留待使用者後續安排。
