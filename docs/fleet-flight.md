# Fleet 飛行整合與世界操作紀錄

此工作依序調查 assembly／Fleet 承接主遊戲的缺口，建立獨立核心 `void-fleet-flight` 與 Bevy 程式 `void-fleet-flight-lab`。使用者已要求先完成 A／B，視窗驗收之後補做；主遊戲現已引用 Fleet runtime 與 assembly 零件；舊固定兩級火箭移到 `void-app --example legacy_flight` 保存回歸場景。

## 調查結果

| 項目 | 原本能否直接承接 | 第一輪處理／後續缺口 |
| --- | --- | --- |
| 自訂船、供油、分級、多船 | Fleet 已引用 assembly graph，並有部件／接點快照 | lab 直接讀 assembly 匯出的 craft JSON，以同一艘船建立地面與軌道實體；不是重寫固定兩級火箭 |
| 行星地形、ground／orbit／bubble | Fleet 已有 owner 交接與地形串流 | 使用主遊戲 GamePlanet 的行星／terrain 設定，獨立畫 LOD 地形與從 Fleet 讀回的碰撞線 |
| SAS | Fleet 已有逐船控制器，能跨 owner 重設框架 | 直接使用既有 SAS；沒有新增順行、目標等模式 |
| 時間加速 | Fleet 有 rails blocker、交會／高度帶攔截 | lab 提供 1／2／4 倍 physics、20／100／1000 倍 rails；阻擋時顯示原因，攔截後回 1 倍；尚未搬主遊戲依高度細分的 warp 上限 |
| 空氣施力 | Fleet 原本完全沒有 AirSource／AirField 接線 | 新增可選 FleetEnvironment；新核心提供各零件阻力，Orbit 每個 Dopri stage 評估、接觸世界逐步評估，bubble 與 ground 都有施力；rails 也保留空氣 |
| 噴嘴氣壓 | assembly catalog 的 Engine 只有真空 thrust／Isp，沒有出口面積 | 第一輪在 FleetAir 用顯式 engine ID → nozzle area 表，large 0.12 m²、small 0.15 m²；壓力降低引擎力、真空質量流率保持不變。未修改既有 catalog／golden；未評為正式零件資料模型定案 |
| 滑行預測 | predict_coast 原本只接受具體 Ephemeris，但 Fleet 持有 EphemerisSource | 改為接受既有 trait，可共用預測；與主遊戲一樣畫真空滑行。lab 的 C 是當下 600 s 預測快照，非持續刷新 |
| 有限燃燒機動 | FlightPlan 與 Fleet staged engine 已可共用 | 已接逐船計畫、編輯／apsis／參考天體、第一個機動的理想軌道導引、存檔／重播；第一 fuel-group flameout 為規劃上限。手動控制、分級、進入 contact physics 明確中止，切船不取消 |
| 撞擊毀損 | PartJointRocket 有 Crash，Fleet 沒有等價船／零件毀損政策 | 尚未接。這也是主遊戲替換前需要保留或明確重設的行為 |
| navball／map／scenery | renderer 部分可共用，但主遊戲 Game 持有大量固定上級／兩級假設 | 第一輪只共用地形 tile 與零件資產；沒有完整 navball／多天體 map／scenery 外觀 |
| 錄放 | 現有 session 格式只記兩級火箭的 mark，Game／step 在 app binary | 已提供 Fleet Action journal、逐船／零件／owner 完整 mark、增量視窗 playback 與獨立程序 headless verify；相機與純視覺操作尚未列入 Fleet 紀錄 |
| 存檔／checkpoint | craft JSON 只保存組裝，Fleet live graph、owners、controls、lit／staged、pending time 沒有存讀介面 | 已能以初始完整行星／terrain／craft 與操作紀錄重建並續玩，保留原 solver／SAS 歷史、錄放保留初始操作歷史；另有直接 checkpoint 保存 live 狀態與完整 native owner cache，不必重播船的操作 |

## 已建立的驗收入口

```sh
cargo run -p void-fleet-flight-lab
cargo run -p void-fleet-flight-lab -- --craft /path/to/void-craft.json
cargo run -p void-fleet-flight-lab -- --planet pebble
cargo run -p void-fleet-flight-lab -- --planet aurelia --vacuum
```

預設 Aurelia＋主遊戲 layered terrain，暫停開始。示範船是 assembly 的 craft（4590 kg、下級 90 kN），不是主遊戲的 7620 kg／120 kN 火箭。因此不以主遊戲軌跡相同或必須能進低軌道作這輪判準。外觀使用 assembly-lab 的實際資產；地形暫用 StandardMaterial，沒有複製完整 scenery 管線。

- P 暫停；Space 分級；Shift／Ctrl 節流，X 關閉；WASD QE 轉向；T SAS。
- Tab 切船（保留每船油門／SAS，清除離開船的手動轉向）；沒有 command 零件的船不提供轉向／SAS。
- N 在發射點附近生成同一 craft，每次再遠 30 m；O 直接建立同一 craft 的 400 km 軌道場景並切焦點。兩者都在同一 Fleet 時鐘中運行。
- `,` `.` 調倍率；C 畫當下真空滑行預測快照；R 回初始場景並暫停。
- 滑鼠左拖環繞、滾輪縮放；F2 繪圖線框、F3 tile 邊界、F4 實際碰撞地形與船體線、F5 地形顯示。船體線讀回 Rapier 實際形狀與 collider local transform；packed Orbit 沒有 native collider，所以不畫虛構碰撞線。曲面只在顯示時三角化，物理仍使用原解析形狀。

空氣與主遊戲維持 force-only 範圍：沒有氣動力矩、旋轉阻尼、熱、燒蝕或翼面。幾何用各零件尺寸、姿態與 live connections 更新；分離後原先被遮住的端面重新暴露，同半徑連接遮住端面，不同半徑保留肩部。不是完整氣動遮蔽演算法。

FleetEnvironment 會在每段起點建立純 AirSource，固定當段幾何／姿態；天體中心在短段內按起點速度線性外推，段長以 flight_chunk_seconds 限制。接觸步則取當下船的質心狀態計算加速度，與既有半步 thrust push 一起保存／交接。供油只由 Fleet 原路徑消耗一次。無環境時保留原 lab 行為，既有 golden／行為門檻不調整。

## 驗證與下一輪

本輪 `cargo test --workspace --all-targets`：226 passed、0 failed、3 ignored；workspace Clippy（`-D warnings`）、fmt 與新 lab build 通過。未開啟或操作 GUI。

Headless 檢查涵蓋自訂 craft 匯入→地面建船→分級與控制保留、氣壓推力與耗油、三種 owner 的阻力、高軌道真空對照、預測不推進 Fleet 時鐘，以及不開 OS 視窗的 Bevy 系統初始化／疊圖更新。有空氣的 16 s 滑行另做 Orbit／Bubble／rails 差分：rails 與 Orbit 相同；Bubble 在 1/60 s 下約差 0.287 m、0.0305 m/s，1/240 s 下約差 0.072 m、0.00763 m/s。測試要求四倍細分時誤差至少縮到 30%，並限制細分後的位置／速度誤差；此一階誤差來自接觸世界逐步取樣速度相依阻力，不能把它稱作逐位元一致。這些不能代替使用者的視窗操作驗收。

第一輪後優先補逐船錄放／checkpoint 與機動計畫的介面，並檢查撞擊毀損與主遊戲所有兩級假設；使用者已要求視窗驗收之後補做，這些介面補齊後繼續主遊戲改接。此階段不是 docking／RCS 或完整遊戲存檔。

## Fleet 存讀與錄放

```sh
cargo run -p void-fleet-flight-lab -- --record lab-log/fleet-session.json
cargo run -p void-fleet-flight-lab -- --load lab-log/fleet-save.json
cargo run -p void-fleet-flight-lab -- --replay lab-log/fleet-session.json
cargo run -p void-fleet-flight-lab -- --verify lab-log/fleet-session.json
```

F6 保存，F7 載入並暫停；路徑由 `--save <file>` 指定，預設 `lab-log/fleet-save.json`。`--record` 會在正常關閉視窗或按 F8 時完成寫檔；F8 後仍能繼續遊戲。現在的錄製不是逐條刷入硬碟，崩潰途中尚未寫出的部分不保留，這個缺口列在 A／B 進度中。

F6／F7 與 `--load` 現在使用直接世界 checkpoint：完整行星設定、live graph／燃料／控制／SAS／pending substep、orbit 傳播歷史、ground／bubble 的 native 接觸世界與 frame／半步狀態。native handles 與它們所屬的完整 arenas／contact caches 一起序列化，帶 Rapier ABI 版本檢查；不是把單獨 handle 當成持久化零件 ID。星曆依初始 SystemSpec 重建到原取樣邊界；船不重播操作。載入先驗證 graph／owner／native handles，再比對完整 world mark。

`--record`／`--replay`／`--verify` 使用另一種檔案：完整初始世界＋命令及結果的 journal。從 checkpoint 續玩後開始的錄放以 checkpoint 作為 base，不必帶入存檔前的歷史。只讀 sim 觀察接口避免 UI 繞過 journal 修改世界。

Mark 檢查所有船、零件位置／姿態／燃料／staged／lit／firing、控制、SAS phase／target、scene／frame origin／睡眠、連接圖、星曆狀態、Fleet 時鐘／pending 時間和選取船。版本、catalog、缺失／亂序 mark、命令結果或世界狀態不同均明確 panic。寫檔在同目錄 temporary file 完成 fsync 後 atomic rename，失敗不覆寫原存檔。

`--load` 直接載入 checkpoint 並可續玩；`--verify-save <file>` 在建立 Bevy App 前驗證直接存檔；`--replay` 由初始世界逐格播放，用錄下的 Advance 時間而不是現在的 wall delta，逐一比對 mark，結束後暫停並可續玩。`--verify` 在建立 Bevy App 前完成重播／比對，不開 OS 視窗。相機／純視覺設定沒有納入此版紀錄，重播時可以自行查看場景。

限制：native cache 是固定 Rapier／模型版本的存檔，不承諾跨版本相容；星曆重建仍與天體模擬時間相關。Journal verify 重跑操作，成本與操作數相關，直接 checkpoint 載入則無此船舶操作成本。模擬規則變更必須提高 MODEL_VERSION，舊紀錄會明確拒絕；目前沒有跨版本 save migration。

新增 headless 檢查：空氣／真空多船保存→載入→續玩、pending substep、分級、控制與 SAS、睡眠後一天 rails、atomic 覆寫、版本／catalog／mark／輸入變更拒絕、增量 playback 與整段 reload 一致，以及實際 lab binary 三個獨立程序驗證與變更輸入失敗。以上仍未做 GUI 驗收。

## CPU 量測

```sh
cargo run -p void-fleet-flight-lab -- --profile lab-log/fleet-profile.json
cargo run -p void-fleet-flight-lab -- --verify lab-log/fleet-session.json --profile lab-log/verify-profile.json
python3 tools/profile-native.py --output lab-log/native-perf.data --stacks lab-log/native-stacks.txt -- target/debug/void-fleet-flight-lab --verify lab-log/fleet-session.json
```

`void-diagnostics` 收集系統 wall duration，報表有 sample count／min／mean／p50／p95／max（ms，nearest-rank quantile），並輸出 traceEvents complete spans（時間線 ts／dur 使用 µs）。lab 的 simulation 與 draw／LOD／overlays、frame interval 分開量；headless verify 也可量完整重建。frame interval 包含等待，CPU 系統 wall time 也可能含排程，不宣稱是 process CPU time 或 GPU 時間；尚未接 GPU／draw-call 統計。

Native sampling 工具使用 perf `cpu-clock:u`、99 Hz、DWARF call graph，可輸出 perf.data 與原始 stack samples；`--dry-run` 可檢查命令，錯誤會直接失敗。已確認此機器 `/usr/bin/perf` 可用，但實際探測受到 `perf_event_paranoid=4` 拒絕，未更改核心設定，也未產生冒充取樣的資料。手動使用視窗執行檔時由使用者操作，agent 僅跑 headless／dry-run。

目前整體 workspace 回歸：236 passed、0 failed、3 ignored；workspace Clippy（-D warnings）與 fmt 通過。最後的 Fleet／diagnostics／lab 全 target 檢查亦通過；尚未宣稱 A／B 全部完成，剩餘工作見 [ab-progress.md](ab-progress.md)。F9 可以結束並寫出目前 CPU profile。

直接存檔驗證涵蓋 awake ground 的 pending／SAS／燃燒、ground＋orbit＋bubble 混合所有權、睡眠後一天 rails 與續接 physics、native cache／graph 損毀拒絕，以及實際 binary 的跨程序 `--verify-save`。每個直接快照載入後再走相同命令，要求完整 mark 相等。船體疊圖另以直接修改 Rapier 形狀與 local transform 的測試，確認觀察的是實際碰撞體，並確認 recenter 不改 body-local mesh。

直接 checkpoint／船體疊圖補齊後，workspace 全 target 回歸：242 passed、0 failed、3 ignored；workspace Clippy（-D warnings）與 fmt 通過。主遊戲改接仍未完成，以上只代表本批新增能力與既有檢查通過。

## 主遊戲改接

`void-app` 與 `void-fleet-flight-lab` 的 binary 現在只有啟動入口，兩者都使用 `crates/app/src/fleet_game.rs`。控制命令、Fleet 時鐘、任意 craft 零件、逐船 SAS／油門、直接存讀、journal 錄放、實際碰撞線與 CPU profile 共用，避免主遊戲再保留第二套物理接線。主遊戲使用同一行星設定的 ground／ocean／atmosphere／cloud／star 管線、navball 和 map；lab 保留簡單地形材料。存檔載入時重建對應地形與視覺資源。

```sh
cargo run -p void-app
cargo run -p void-app -- --craft /path/to/void-craft.json
cargo run -p void-app -- --load lab-log/fleet-save.json
cargo run -p void-app -- --verify-save lab-log/fleet-save.json
cargo run -p void-app -- --record lab-log/fleet-session.json --profile lab-log/fleet-profile.json
cargo run -p void-app --example legacy_flight
```

主遊戲預設 assembly 的 demo craft（4590 kg），可直接用 assembly 匯出的其他 craft，並非先前固定 7620 kg 火箭的逐零件映射；原來火箭與原數值回歸保留在 legacy example。主遊戲新開世界開始運行，lab／載入世界暫停開始。Tab 切船，Shift+Tab 循環天體焦點；map 標籤可點，G 切 inertial／surface path，K／L 切高度／速度读數。時間倍率沿用主遊戲九檔與高度限制；接觸、交會、燃燒阻擋由整個 Fleet 判斷。

獨立場景與主遊戲材料／儀表／map 的 headless Bevy 初始化檢查通過，含 Aurelia → Luna 世界替換；不建立 WindowPlugin 或 renderer。主遊戲 binary 另有獨立程序 --verify-save／--verify 檢查。GPU 畫面仍留給使用者驗收。逐船機動計畫保存／執行與撞擊毀損尚未完成，A／B 持續進行。

共享 runtime／主程式第一階段改接後 workspace 全 target：244 passed、0 failed、3 ignored；workspace Clippy（-D warnings）與 fmt 通過。機動執行／毀損等剩餘項目仍需完成，不能把這個測試結果當作 A／B 完成。

## 逐船機動與保存

main 和 Fleet integration lab 使用同一組操作：M 新增機動，`[`／`]` 選取，方向鍵調 prograde／normal，PageUp／Down 調 radial，Home／End 調開始秒數（Shift 放大步幅）；Y／U 將燃燒中心放在下一次近／遠拱點，V 選參考天體或 Auto，Delete 刪除。B 執行第一個機動，Escape 中止。橙色線為機動預測；所有編輯與執行均寫入 Action journal。

計畫屬於 vessel ID，包含每個 ManeuverSpec、選取機動、已完成數、預測軌跡／積分記憶、執行狀態與中止原因。切船後原船繼續燃燒，直接存檔在燃燒前與中途都保存 Fleet guidance 的起止時間、方向 law 和 engine group rating；載入繼續而不重新點火。完成時移除第一個機動、保留後續機動，後續須再按 B。Auto 在執行前以預測的點火位置選 dominant body。

這是與 orbit FlightPlan 相同的**理想軌道導引**：方向 law 在每個 Dopri stage 求值，顯示姿態跟隨推力方向，沒有有限轉向時間／RCS 模型。任意 craft 使用合力的本體軸，不假設 +Y 是推力軸；不平衡力矩、無控制零件、無已分級引擎／供油、跨第一個 fuel-group flameout、contact owner 都明確拒絕。啟用時停止原 SAS hold，避免舊姿態目標與導引衝突；結束後可重新啟用 SAS。分級、手動控制或 orbital→contact 交接中止導引並記錄原因，不用此模式強制旋轉接觸剛體。

Fleet 在軌道積分 leg 內切開燃燒起止時刻，因此另一艘地面船造成全世界 1/60 s 固定更新時，0.007 s 等非整步點火時間仍按指定時長耗油。預測是固定 engine 的真空計畫；實際大氣阻力與噴嘴壓力仍由 Fleet environment 求值，低空不保證達到預測 Δv，重心也會隨耗油移動。ground/bubble 出入的行為需由使用者之後視窗驗收。

模型版本提高到 2、Fleet native checkpoint schema 提高到 2，舊版本明確拒絕，目前沒有跨版本遷移。headless 檢查涵蓋非整步燃燒與獨立軌道速度對照、armed／running 直接存讀後逐步完整狀態一致、切船保留機動、手動／分級／contact 中止、計畫與 journal replay，以及主程式按鍵接線（無 WindowPlugin／renderer）。

逐船機動這批 workspace 全 target 回歸：253 passed、0 failed、3 ignored；workspace Clippy（-D warnings）及 fmt 通過。整體 A／B 仍有 ab-progress.md 列出的缺口，尚未宣稱全部完成。

## 機動前快轉

Z 開始或取消機動前快轉；第一個有效機動須在 30 秒之後，且 Fleet 允許 coasting rails。main／lab 都沿用各船高度對倍率的限制，Advance 在核心內最多只走到開始時間前 30 秒，再切回 1x；一次大 wall delta 或讀檔後續玩也不能跨過此邊界。這一步不點火，仍由 B 啟動機動。Tab 切船不改快轉目標，手動控制、分級、編輯、執行或取消機動會中止；新交會、高度帶或 target 的 contact 所有權也會停下。

目標 vessel ID、開始／停止時間及中止原因納入直接 checkpoint、Action journal 和完整 world mark。模型版本為 3。headless 測試涵蓋大步長精確停止、中途保存／載入／重播、切船保留目標、手動操作取消，以及新交會先於燃燒前邊界中止。

機動前快轉這批 workspace 全 target 回歸：256 passed、0 failed、3 ignored；後續新增的視窗時間／按鍵接線測試連同 app library 13 項全數通過。workspace Clippy（-D warnings）與 fmt 通過。
