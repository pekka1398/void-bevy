# Fleet 氣動力與力矩 lab

本輪以 master `41da390`（model 13）為基線。分支 model 16，尚未 commit／合入；主遊戲預設 craft 不增加翼或降落傘，且維持明確的 `AirDynamics::ForceOnly` 物理配置；完整通路只由 lab 顯式 opt-in。熱、燒蝕、破壞不在範圍內。

## 共用施力契約

`void_modules::Wrench` 的 `frame: FrameId`、`reference_point: DVec3`、`force: DVec3`、`torque: DVec3` 全為明確 SI／f64。`torque` 是**關於 reference_point**的總矩。`at_point(frame, reference, point, force, intrinsic_moment)` 只做一次 `(point-reference) × force + intrinsic_moment`；`at_offset` 以已知局部力臂避免遠方 absolute COM 加減捨入；`about` 改參考點，`in_frame` 使用共用 FrameTree 轉點並旋轉力／矩，`add` 拒絕不同 frame 或 reference 的相加。不應把已關於 COM 的矩再加一次 r×F。

`engine::Thrust::wrench_in` 與 `vessels::EngineForce::wrench_in` 接受 parts-frame 的 placement 和 query-frame 的 moment reference，供 engine／RCS 消費者共用。既有 `Propulsion.force/torque` 保留 parts axes／COM 語義及供油 groups，耗油仍由 `burn` 接受步提交，沒有第二套 resource／physics runtime。

`VesselAir::wrench_in(at, query, COM State, parts_to_query_rotation, angular_velocity_relative_to_query)` 為純 trial 求值。每個 body／wing／chute 作用點都取自己的環境：point velocity 為 COM velocity + ω×r，環境再轉至大氣所屬天體地表以減去局部風。因此不在 aero core 再加第二次 ω×r。body／wing 沿用 `void-aero` 的係數、polar 與 intrinsic pitching moment；chute 以 attachment point 建立偏心矩。`Fleet::aerodynamic_wrench(id)` 提供只讀 HUD／驗收觀察接口，不準備或提交 module state。

新增 `Module::LiftingSurface`／`LiftingSurfaceDefinition`：part-local point、chord、normal 和既有固定翼係數，state 為 Passive，非法非有限／非正交／超界係數直接拒絕。`ParachuteDefinition.point` 是明確 part-local attachment point；原有傘 authored 值為零。catalog 新增 `aero-stabilizer-pod` 和 `eccentric-chute-pod`，既有 flight rocket 保持。

## 明確模式與積分擁有者

`FleetOptions.air_dynamics` 為必要的序列化欄位，enum 為 `ForceOnly`／`ForceAndTorque`。`ForceOnly` 是原有 COM body-air 取樣、零 rotational point velocity、frozen-air attitude 與原 staggered push／midpoint-mass 路徑；原有主遊戲和 multi-body 預設保留。`ForceAndTorque` 才啟用下列耦合與接縫修正。`InitialWorld.air_dynamics` 同樣明確存入 recording／reset；aerolab 呼叫 `with_air_dynamics(ForceAndTorque)`，不是 runtime 的隱藏切換或 fallback。checkpoint 缺少模式直接拒絕。

- Orbit 在有大氣的世界或旋轉／施矩時，把 accepted leg 限至 Fleet fixed step。先用起點矩預測中點姿態與局部氣流，再用中點矩做 `void-rotation` 二階 split；既有 Dopri translation propagator 每個 trial stage 看到該 stage 的姿態、角速度和旋轉後的引擎力。Control 在此只提供質量流，source 供應旋轉的推力，避免重複施力。accepted leg 後才 burn／recenter，下一步重新計算 live 質量、COM、慣量。
- Bubble／Ground 延續 ContactWorld 的 staggered leapfrog translation 和既有接觸求解。空氣矩用中點預測並交给同一 `apply_local_torque`。bubble origin 已獨立積分到兩端；純 trial 的中點 frame 使用兩端位置／速度的 cubic Hermite interpolation，並以相對差值保持附近精度。無 body-centre 線性外推。
- Scene 的 `push` 現在保存**新邊界的瞬時 load**。舊碼保存上一時刻姿態的力，再用 previous/current 平均，旋轉推力會延遲半步。相應 mass 分母用同一邊界質量，避免 midpoint mass 與 boundary reconstruction 重算半步耗油。原有 turning-burn owner 門檻未放寬。
- 睡眠地面船的被動空氣不喚醒；施矩、油門等 active 控制仍能喚醒。動態質量僅在真正有消耗時更新 Rapier pieces；被動零消耗不呼叫會 wake 的 mass setter。rails 拒絕 moving vessel 的非零 aerodynamic force／torque，sleeping ground 可 idle rails。
- 原 propagator 在 `Some(AirSource)` 的 advance 已重新求導；真正 cache 缺口是 **Some→None detach**，同 Control 可能沿用場存在時的 FSAL derivative。新增 `PropagationRun::invalidate_force_derivative`，只在 accepted leg 改／卸載 source 時失效，保留 step hint／impact，不在 trial evaluation 失效。純 vacuum None→None 不重置。
- 最後 flameout interval 小於 clock ulp 時，時間端點可能與起點同一 f64；它是明確的 fuel exhaustion event。只在 `end == t + seconds_to_flameout` 時提交該精確消耗，姿態不呼叫 dt=0 的 rotation step。可表示的小 dt 照常積分，無 fuel／time epsilon fallback。

完整模式在有大氣的世界即使位於 ceiling 外，也使用 coupled fixed steps，防止第一個穿入氣層的 leg 漏算矩。因此 Full 高空／warp 目前較慢；尚未加入 ceiling event 跳步最佳化。ForceOnly 和無大氣世界保留原路徑。

### 理想機動導引的例外

既有 maneuver 的 ideal pointing 仍是理想 actuator：燃燒時以 thrust law **指定姿態**，其 actuator 抵銷氣動矩，accepted angular velocity 為零。新 `GuidedAirSource` 在所有 translation trials 和 accepted endpoint 共用同一既有 thrust-law evaluator，避免 frozen air 姿態與推力／accepted 姿態不一致。這個模式不聲稱有限慣量的完整氣動姿態耦合，也不模擬理想 actuator 的轉向速率／能量。燃燒前、取消或完成後回到 physical air／manual／SAS 路徑；最小測試確認解除時不另加姿態／角速度衝量，之後矩才按物理時間產生角速度。

耗散檢查適用於無主動控制、無 intrinsic pitching moment 的 drag／固定翼示例，測量相對空氣的 `F·v + τ·ω <= 0`。不把它泛化到所有工程 pitching-moment 係數或被主動控制的翼面。

## 視窗驗收

```sh
cargo run -p void-aero-flight-lab -j 2
cargo run -p void-aero-flight-lab -j 2 -- --record lab-log/aero-flight.jsonl
cargo run -p void-aero-flight-lab -j 2 -- --replay lab-log/aero-flight.jsonl
cargo run -p void-aero-flight-lab -j 2 -- --verify lab-log/aero-flight.jsonl
cargo run -p void-aero-flight-lab -j 2 -- --verify-save lab-log/aero-flight-save.json
```

1. `1` 固定翼：5 km、80 m/s、初始偏角及 spin 0.3 rad/s，`P` 開始；觀察綠色翼面、風標回正、振盪衰減。`2` 相同初始 spin 的無翼 pod 作對照。
2. `3` 中置傘、`4` 偏心傘：`D` armed、`P` 開始。觀察 semi／full area、姿態和 torque；偏心傘 rotation 後力臂可逐漸對齊，所以末態 torque 可能小於剛 deploy 時。`C` 切傘，不能 redeploy 已 cut 的傘。
3. 黃箭頭為總空氣 force 方向，粉紅箭頭為關於 live COM 的 torque 方向，箭頭長度固定；HUD 有 Newton／N m／rad/s，不能從箭頭長度比較大小。綠矩形為固定翼作用區。collider 仍是 authored 零件 cylinder/cone，翼面的額外碰撞 mesh 尚未建立。
4. `T` 啟用既有 SAS（零油門）；`P` 暫停，`W` 切 1x／4x，`Tab` 切船。`R` rails request 會顯示明確 blocker。
5. `F6` 保存、`F7` 還原；`F8` 開始／結束 durable recording。在半開傘、全開傘、旋轉中翼面各試一次存讀；headless verify 會重算實際 recorded actions／marks。

使用者最終視窗驗收仍待完成。本輪 agent 使用 TigerVNC :12／5912 核對過四個 preset、deploy／full、F6/F7、GUI journal headless verify；這不是使用者的最終驗收。

## 數值證據

`modules/tests/wrenches.rs`：reference shift／frame rotation 不重複 moment arm、對稱零矩、偏心傘精確矩、trial state 不變、零空氣極限、spin 0.5／5／50 rad/s 的局部風阻尼和耗散。

`vessels/tests/aero_wrenches.rs`：orbit／bubble 耦合 dt 收斂、owner 比較、矩阻擋 rails、vacuum free rotation、trial sampling 純性與 checkpoint 精確續跑、偏心傘進入兩 owner、sleep air／idle rails、sub-ulp flameout／可表示小 dt、ideal guidance 解除。

固定翼 2 s，reference dt=1/960：

| owner | dt | position error m | velocity error m/s | attitude error rad |
| --- | --- | --- | --- | --- |
| orbit | 1/30 | 0.027276 | 0.011931 | 7.4597e-4 |
| orbit | 1/60 | 0.005810 | 0.002598 | 1.8539e-4 |
| orbit | 1/120 | 0.001324 | 0.000599 | 4.4875e-5 |
| bubble | 1/30 | 0.024446 | 0.009883 | 7.2138e-4 |
| bubble | 1/60 | 0.005076 | 0.002066 | 1.7763e-4 |
| bubble | 1/120 | 0.001136 | 0.000464 | 4.2804e-5 |

dt=1/120 的兩 owner 差 0.000203 m、0.000146 m/s、2.14e-6 rad。在完整配置下的 4 s burning+turning owner 差 0.0114 m（原門檻 0.05 m）；預設 ForceOnly 的原直線 burn、turning burn、fuel recenter、sleep、gravity／handoff／golden 檢查照舊保留。

checkpoint／recording 使用分支 model 16 及新 catalog 的嚴格匹配；old model／catalog 直接拒絕。FleetCheckpoint 分支版升到 **6**（模式是必要欄位，與 RCS 分支 5 區分）；InitialWorld 同樣有必要模式欄位。Craft version 2 保持；recording format 1／world checkpoint wrapper version 1 的必要 InitialWorld 欄位形狀已增加模式，並由嚴格 model 16 檢查阻止舊檔混用；既有 model-13 的存讀／錄放不能冒稱相容。最終整合的 model version 由 root 另統一。


最終驗證：核心套件 109 passed、1 既有 ignored；其後新增 ceiling-entry 回歸並重跑氣動 Fleet 檔 9 passed（合計 110 個不同核心 tests）。FleetFlight 八個整合 suite 54 passed；最後氣動 recording suite 再跑 1 passed。`void-modules`／`void-vessels`／`void-assembly`／`void-orbit`／`void-fleet-flight`／`void-aero-flight-lab` targeted all-targets Clippy `-D warnings` 通過；主 app 的受影響 `multi_body` example `cargo check` 通過。無全 workspace 編譯／測試。

先對 lab 的 17 個 local 傳遞 package 精確 `cargo clean -p`，未清外部 Bevy／Rapier，再以本 worktree 路徑重編。證據在 `lab-log/aero-flight/fleet-flight-tests.log`、`final-build.log`、`final-wrench-tests.log`、`final-clippy.log`、`final-camera-build.log`。最終私有 binary `lab-log/aero-flight/void-aero-flight-lab` SHA256 為 `760307de10ac7e9a719c4121f27c101487b3763aaed8618bf661b36dbde40b2e`。其 `--verify lab-log/aero-flight.jsonl` 和 `--verify-save lab-log/aero-flight-save.json` 均通過 T+4.300 s。最終截圖為 `lab-log/aero-flight/final-preset1.png`、`final-fins-running.png`、`final-no-fins.png`、`final-centered-chute.png`、`final-eccentric-semi.png`、`final-eccentric-full.png`；較早檔案皆是 development artifacts。


Root review 補上兩個模式一致性約束：`FlightCheckpoint::capture/restore` 必須使 live／restored `Fleet.options.air_dynamics` 等於 `InitialWorld.air_dynamics`；`world_mark` 明確包含 `airDynamics`，所以相同零 load 初態也不能混淆兩種配置。錄放 header 原已完整比較 `base.initial` 與 `recording.initial` 的 JSON，因此必要模式欄位一併核對。新增 conflicting-mode capture／restore 與 mark 分離回歸；root targeted FleetFlight 氣動檔 2 passed。mark shape 更動後以新 binary 重新從 GUI 產生 journal／save；較早檔案完整保留為 development artifact，沒有手動改寫 marks。


模式 mark 修正後的最終重新驗證檔：`mark-final-clean.log`／`mark-final-build.log`（17 local packages 精確 clean 後全部純分支重編，42.29 s）、`mark-final-gui.log`、`mark-final-verify.log`、`mark-final-verify-save.log`。新 GUI journal 與 save 真正由上述最終 binary 產生並各驗證至 T+4.300 s。新截圖 `mark-final-wing.png`／`mark-final-eccentric-semi.png`／`mark-final-eccentric-full.png`；舊 journal／save／binary 保留為 `pre-mode-mark-*`。完整可套用 review patch 為 `lab-log/aero-flight/review.patch`，含 root checkpoint／mark／test 修正及所有新增實作檔案（不含輸入 spec、build outputs 或 GUI artifacts）。
