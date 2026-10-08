# 濺落卡頓：model26基線與model27候選（2026-10-08）

## 目前候選

使用者要求嘗試不反向的平滑阻力更新。`work/splashdown-performance` 的model27
在Contact Scene將水阻力改為accepted-boundary impulse；浮力仍按原排水體積、
浮心、有效重力計算。Scene保持原nominal cadence，不再由水阻力拆整個世界的步。
目前可玩、待人類判斷入水動態，未合入master；操作見[water-review.md](water-review.md)。

COM上的平方阻力更新為 `v / (1 + dt*c*|v|/mass)`。偏心作用點使用
`B = I/m - [r]× I_world^-1 [r]×`，解 `(I + dt*c*|u|*B) u_next = u`，
同一衝量同時更新平移及旋轉；旋轉阻尼另做implicit tensor更新。
各零件依既有順序更新同一剛體，不clip浮力／重力／引擎／碰撞後的總速度。
阻力用連續力更新後的預測速度，native resistance接口不再額外補半個contact衝量。

純阻力更新耗能，不是跨碰撞、浮力及幾何改變的全步能量證明。
浸水幾何每接受步凍結；海面不做流體／波浪模擬，阻力仍按浸水體積縮放。
不同零件依序splitting；高速入水穿入深度、翻轉及恢復須人類看遊戲。
Orbit保留原adaptive連續水力求值；主遊戲近海collision Scene使用新衝量。

模型升至27，拒絕26的journal／save；world4、FleetCheckpoint12、Craft3不變。
`crates/fleet-flight/fixtures/water-performance-world.json` 是明確取出的原InitialWorld
配方，不是遷移或放行舊recording。以下量測及算法是model26歷史基線。

## 原基線


使用者在 model26 水場景的入水瞬間看到明显停頓。此次定位使用原候選二進位
`c7c638b`／SHA256 `fab9667f72cdfecfb01f91c77cf123d49a1eaad13c8dcc084ebaedd08dd52009`，
沒有修改候選物理模型或降低渲染品質。原先四功能檢查未量測入水幀耗時，不能當作
水的即時效能已通過驗收。此效能問題待修正，主線尚未合入四功能。

## 量測

Headless 重建原 `model26-water-01.jsonl` 的 InitialWorld，使用主遊戲同一
`water::splashdown`，每次 Advance 0.05s；沒有 renderer、recording/mark 或存檔寫入。
獨立計時水步長 bound 和軌道預測；這兩項不算在 Advance 耗時內。

| 區間／量測 | 第一次 | 第二次 |
| --- | --- | --- |
| 入水前 T+0.15–0.6s，Advance 中位數 | 4.29ms | 6.58ms |
| 入水 T+0.7–2s，Advance 中位數 | 239ms | 308ms |
| 入水區間最慢 Advance | 1.83s | 3.01s |
| 漂浮 T+6–12s，Advance 中位數 | 62.6ms | 92.9ms |
| 入水區間最小 stable step bound | 0.038142ms | 0.038142ms |
| 6000s coast prediction 最慢讀值 | 0.183ms | 0.147ms |

两次每行的時間、高度、速度、水力和 bound 數值完全一致；wall time 會受主機其他
工作影響。第一次最慢在 T+1.10s，advance 耗時1.83s。T+1.05s 水力仍為0，
保守入水 envelope 已提前縮步，單次 Advance 已耗時1.14s。

TigerVNC 主遊戲未修改的同一濺落場景，simulation span 最慢 **2.629s**；
CPU draw_lod_overlays 平均 **6.26ms**、最慢22.14ms；持續漂浮後 simulation 的中位數
89.5ms。這是 system wall duration，不是 process CPU 或 GPU timestamp。
GPU time 沒單獨量測；但無 renderer 仍重現秒級停頓，足以定位主要瓶頸在物理推進。
Profiler 的 frame_interval 使用 Bevy Time，可能夾到250ms，不能把它當真實卡住時間。

## 程式路徑

- `void_modules::water::VesselWater::stable_step_in` 以完整船殼體積、速度、保守力臂
  和逆慣量界線估計阻力鬆弛 rate，返回 `min(maximum_dt, 0.2/rate)`。接近海面就啟用
  conservative entry envelope；通常1/60s（16.67ms）在此例縮至約0.04ms，約400倍。
- `Fleet::step_all` 的 while 將一個正常步拆成許多 accepted substeps；每個小步呼叫
  `set_physics_step` 和 `step_all_accepted`，推進全部 Scene／Orbit 船、熱／資源等，
  不僅是入水艙體。這放大了全世界計算量。
- 小步中 `VesselWater::new` 重複複製零件資料，`displacement` 重建32面柱／錐／box
  hull、裁切／cap／體積重心；反覆查環境與 frame，另外有 native timestep retiming。
  目前未將每項成本細分；不能假稱已證明單一 allocation 或 renderer 是全部根因。

本次沒有發現以 NaN／浮力發散造成停頓的證據；主要是同步 simulation 耗時失控。
浮起／下沉測試通過不能代替即時效能測試。修正需要改善拆步範圍、保守界線與重複
求值，保留數值穩定、跨 owner、accepted resources、碰撞與錄放語義；不隨意夾大
最小步長、不降物理精度掩蓋問題。

## 證據與重現

測量分支 `work/splashdown-performance`／`void-bevy-water-perf` 只增加一個 readonly
bound getter 與 headless example。候選 integration 的 executable 沒有換掉。
基線9c9d9a4的測量命令（current model27會拒絕此26 journal）：

```sh
cargo run -p void-fleet-flight --example splashdown_timing -j 2 -- \
  /home/pekka/Archives/VOID/2026-10-08/four-features-review/model26-water-01.jsonl
```

GUI 使用 `target/acceptance/void-app --splashdown --profile <new-profile.json>`，以
TigerVNC 操作 P 暫停、F9 完成 profile。證據保存於
`/home/pekka/Archives/VOID/2026-10-08/water-performance/`，包括兩份 CSV、原 GUI profile、
截圖、量測摘要和 SHA256；measurement lib/example Clippy -D warnings 和 fmt 通過。
