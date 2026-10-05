# RCS 與物理對接 lab

> 後續更新：主遊戲整合經 root 組合／GUI／錄放核對，使用者認可對接場景並授權合併後，已於 `134bb37` 合入本地 master；見 [主遊戲交付與驗收](main-flight-integration.md)。尚未 push。

> 2026-10-05 狀態更新：核心由 `4bd5322` 合入 `155a4f8`，並於 `0c60aad` 與氣動統一為 model 17／FleetCheckpoint 7。主遊戲接線暫停，新的主遊戲驗收與組合檢查未完成。以下實作／測試記錄保留分支當時的版本與 lab 範圍；分支舊檔不與 model 17 相容。現況見 [status.md](status.md)，後續工作規則見 [AGENTS.md](../AGENTS.md)。

本輪基線 master `41da390`；暫用整合 `MODEL_VERSION 15`、Fleet checkpoint 5。主遊戲預設火箭沒有 RCS／對接埠，本輪新增能力供獨立 lab 驗收，未改主遊戲操作。舊 model／checkpoint 明確拒絕；Craft 仍為 2，catalog checksum 隨新定義改變，沒有假裝舊存檔可相容。

```sh
cargo run -p void-rcs-docking-lab -j 2
cargo run -p void-rcs-docking-lab -j 2 -- --record lab-log/my-docking.jsonl
cargo run -p void-rcs-docking-lab -j 2 -- --replay lab-log/my-docking.jsonl
cargo run -p void-rcs-docking-lab -j 2 -- --verify lab-log/my-docking.jsonl
```

## 唯一狀態與施力

- `Module::Rcs` 是按零件／穩定 module ID 尋址的噴嘴：局部 `point`、單位 `direction`、推力、Isp、typed resource。`ModuleState::Rcs` 只保存 enabled；沒有額外 fuel inventory 或每幀累積的 firing cache。
- `Module::DockingPort` 按穩定 ID 尋址現有 attach node，具有距離、法向角度、相對埠速度、相對角速度限制及分離衝量。`ModuleState::DockingPort` 保存 armed；占用／捕獲完全由 PartGraph connections 表示。
- `RcsControl` 是每船的 SI force／torque 目標，並單獨保存 enabled。不是施加在船上的理想力。按 members、catalog modules 的穩定順序，用 **固定 64 次 cyclic bounded coordinate-descent** 最小化六維 wrench residual；每顆噴嘴 throttle 限於 `[0,1]`。Torque 除以最大的噴嘴距 COM lever（最小 1 m），使 force／torque 誤差有明確尺度。不能達成的目標留在 residual，不憑空補力。
- 輸出的每顆 force、point 延用 EngineForce／FuelGroup。相同 typed resource、相同 crossfeed tank pool 的 engine 與 RCS 先合組，再計算總耗油／flameout／末步推力 fraction，避免 double-spend。封閉供油、其他 owner、空油箱與停用噴嘴不參與分配。燃料質量及 COM 繼續只取 PartGraph。
- authored `rcs-pod` 有 20 kg Monopropellant、四個實際噴嘴 cluster（24 顆 80 N／240 s 噴嘴）以及 `dock`／`dock-bottom` 兩埠；使用同一 catalog、Craft、PartGraph 和 Fleet。
- RCS 分配在 `void-modules::rcs`，供油在原 `void-vessels::propulsion`，姿態／平移仍由原 Fleet owner 積分；未增加第二個物理迴圈。SAS 原有 reaction-wheel／steering 路徑獨立，lab 的轉向鍵只發 RCS torque 目標、不發 VesselControl.turn，也不開 SAS。啟用且非零的手動 RCS 會中止理想 guided burn；active RCS 時也拒絕啟動 guided burn，避免同時爭用姿態。

## 捕獲與解除

`Fleet::dock(part_a,module_a,part_b,module_b)` 先檢查不同 owner、兩埠 armed／free、node size、距離、相反法向、**埠端**相對速度（含 `ω × r`）及相對角速度；任何正常不滿足条件回傳具體 Refused reason，graph 不變。兩端取較嚴的限制。埠視為軸對稱，因此不限制绕法向的固定 twist 角；相對旋轉速率仍受限。

snapshot.position 是實際 COM：把零件座標的埠點換成 COM lever 時，必須減去該 owner 的 `centre_of_mass_local`，包括 Scene 耗油後 COM 偏離 parts origin 的情況。埠端速度的 `ω × r` 也使用同一 COM lever。同 scene 的相對位置／速度走原 Fleet 的 scene-local 精確相對量；兩個 Orbit owner 只有通過 capture gate 後才建立共同 bubble。不同現有 scene 明確拒絕。成功捕獲使用原 join 的合併、重新定 COM 和角動量計算，保持零件 ID、世界 pose、資源、質量及總線／角動量。這是閾內、按命令的非彈性硬捕獲，不會把船 teleport 到理想姿態；捕獲前不模擬磁吸牽引。閾內的原埠間距在剛體 graph 裡保留。

`Fleet::undock` 只接受兩個實際 docking modules 的 connection，再走原 graph split：兩端都 disarm，保存每個零件的世界 pose／resource／身份與剛體速度場，在埠施加相等反向的 configurable 小衝量。需明確 rearm 才能重新捕獲。原 `Join` action 保留為 debug 操作，lab 的 J 使用新 `Dock` action，不冒稱 debug join 是對接。

Checkpoint 保存 PartGraph modules／connections 和 RcsControl。新增 Rcs／RcsNozzle／Dock／Undock／ArmDock journal actions，world marks 包含 RCS control 和 module states，錄放能檢查控制與捕獲結果，並从 pending timestep 續跑。

## 視窗驗收

- `1`：兩船 COM 距離 2.15 m、埠距 0.15 m，已對準，暫停；按 J 即可驗收捕獲。
- `2`：兩船 COM 距離 8 m、埠距 6 m；直接近距交會，不需從發射台重玩。相機追蹤兩個原始 part 的世界 midpoint、按距離縮放，兩船可見，dock／undock 不因 owner COM 改變跳焦點。
- `P` 暫停／續跑；`Tab` 在這兩船的現有 owner 切換，先以 journal action 歸零舊船的手動 RCS force／torque、保留舊船 enable，再同步新船 enable。通用 `Action::Select` 仍保留逐船持續控制，只有 lab 的鍵盤 handoff 採此歸零流程。
- `W/S` 局部 ∓Z、`A/D` ∓X、左 `Shift/Ctrl` ±Y 平移；箭頭 X/Z 轉向，`Q/E` Y 轉向。平移目標 40 N，轉向 15 Nm；左 Alt 為十分之一精細控制。
- `R` RCS enable；`H` 切換所選船第一零件的 `rcs-0-0-1`，HUD 顯示 off，其餘按 module ID 顯示真實分配 throttle。橘色線是实际噴嘴 plume，從 snapshot COM 轉到 allocation 的 parts-frame mount 時也減去 `centre_of_mass_local`；紅／藍方向標記讓軸對稱圓柱的 yaw 可見。
- `J` 物理 capture；`U` 解除；`C` 重新武裝兩個 top ports。HUD 顯示埠距、module states、實際施力／力矩、residual、油量及具體拒絕原因。
- `F6/F7` 直接保存／讀取 `lab-log/rcs-docking-save.json`；`F8` 開始／結束 `lab-log/rcs-docking.jsonl`。CLI --record 可指定新檔名；journal 不覆蓋既有檔案。

建議：1→J→P→按住E觀察噴嘴／油量／方向標記→P→U→J（必須明確拒絕 disarmed）→C；F6/F7 往返，再使用2練習Shift接近、Ctrl剎車、精細平移與轉向。最後收錄一段操作並 headless verify。

## 驗證

- 初輪受影響四個 core crates：`cargo test -p void-assembly -p void-modules -p void-vessels -p void-fleet-flight -j 2`，**152 passed、0 failed、1 ignored**（既有已結案 Pebble 傾角）。沒有跑全 workspace。
- 這四 crates all-targets Clippy `-D warnings`；lab build 和 lab all-targets Clippy、fmt 均通過。
- 新增核心檢查：平移／純轉向／混合 wrench、飽和與不可達 residual、非對稱／停用／空燃料、typed 多資源、engine／RCS 共用 supply pool、crossfeed 開／封／owner 範圍、距離／法向／速度／spin 閾內外、兩個 addressed ports、未武裝／占用／自身 owner、旋轉埠端速度、捕獲／解除的身份／世界 pose／resource／線角動量，以及 checkpoint／durable replay 的一致續跑。
- TigerVNC 的 agent smoke 已核對 capture、RCS 旋轉耗油、解除／拒絕重接、F6/F7、H、F8，以及實際 journal headless verify。這不是使用者最終視窗驗收；本輪沒有 commit／push。


## Root 審查修正的回歸（2026-10-05）

Root 修正了非零手動 RCS／理想 guided burn 的姿態互斥，以及 capture gate 從實際 snapshot COM 到埠的 lever 缺少 scene-local COM subtraction。這兩處生產修正保留，新增回歸沒有覆寫它們。

`depleted_asymmetric_scene` 使用雙 pod、只有上 pod 有 Monopropellant 的不對稱供油。兩船同向 RCS 燃燒 30 s 保持 Bubble owner，耗油使 scene COM 偏離 parts origin 超過 1 cm，且保留非零 ω。第三艘對接船的位置／速度由 frames 的實際 node `apply_state` 配置；判準也由兩個 port 轉到同一 scene frame 的 State 取得，不複製 gate 的 COM 計算。兩項測試分別涵蓋 0.199／0.201 m 距離，以及 0.4 m/s 埠端速度兩側，含 `ω × COM offset`。

為驗證測試能抓住原缺陷，在本 worktree 的 ignored `target/rcs-com-mutant-workspace` 建立來源副本，僅在副本移除兩個 COM subtraction。原始修正檔案不變。副本的兩項測試都如預期失敗：实际 gap **0.199 m** 被拒為 outside capture distance；實際 tip speed **0.3997484113 m/s** 被拒為 relative port speed too high。完整輸出保留在 `/tmp/void-rcs-com-mutant.log`。

清除本 worktree 自有 target 的 `void-vessels` artifact 後，修正版 RCS／docking／guidance targeted tests **17 passed、0 failed**；vessels／fleet-flight all-target Clippy、fmt、diff check 通過。測試輸出在 `/tmp/void-rcs-review-regressions.log`。COM 回歸階段只使用自有 target，先前 model15 lab binary／GUI 證據是審查修正前版本；後續包含全部審查修正的 binary／GUI 核對見下段。


追加 lab 審查：Tab 的舊船 neutral／enable 同步及 plume COM offset 已修正。只有兩個小型 engine-free helper，預設 window feature 保持 `cargo run -p void-rcs-docking-lab` 原用法；可用 `cargo test -p void-rcs-docking-lab --lib --no-default-features -j 2` 驗證而不編譯 Bevy。兩項 headless 回歸通過：切船停止舊船耗油、保留各船 enable、journal replay 一致，未知 target 在任何 neutral action 前拒絕；耗油後 plume 逐一與 frame-tree mount 對照，舊公式誤差超過 1 cm。這些只修正 lab，沒有更改通用 Select 語義。


全部審查修正的 final lab 已精確清除共用 cache 的 17 個本地 packages 再重編（保留 Bevy／Rapier 外部依賴），default-window all-target Clippy、fmt／diff check 通過。最終私有 binary 是 `/tmp/void-rcs-docking-model15-review-final`。TigerVNC 新操作實際包含按住 D→Tab 且保持 D→放 D→Tab 回原船：目標船保留 RCS false、零實際施力，原船保留 enabled true 但控制歸零，切出後油量 19.985 kg 不再變動。另核對 preset 2 兩船可見、capture、旋轉、undock／disarmed refusal、F6/F7、H、F8。新 journal `/tmp/void-rcs-review-final-model15.jsonl` 有完整 End record，最終 binary 的 `--verify` 通過，log 沒有 panic／ERROR。截圖 `/tmp/void-rcs-review-{held,switched-held,released,back,rendezvous,refused,loaded}.png`；私有 app／TigerVNC 已按數字 PID 停止。這仍是 agent smoke，使用者最終驗收待 root 安排。
