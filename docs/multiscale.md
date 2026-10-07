# multiscale：從公分到光年的座標與耦合

`crates/multiscale`（`void-multiscale`）移植 `lab/multiscale` 的座標與物理，不依賴 Bevy。

| TS | Rust |
| --- | --- |
| `SplitPosition.ts` | `void-frames` 的 `split.rs`（座標樹統一時從本 crate 移過去，根層精確相減也用它）：`SplitPosition`，每軸整數格（2^32 m）加 float64 offset。lab 的格是 bigint，這裡是 `i128`（±1.7e38 格） |
| `CoupledWorld.ts` | `world.rs`：`CoupledWorld`，多個恆星系在同一個直接 N 體世界；系統質心用 split 位置，天體用局部 float64；Yoshida-8 與五次 Hermite 取自 `void-orbit` |
| `Frames.ts` | `traveller.rs`：`FramedState`、`absolute`、`reframe`、`nearest_frame`（5% 遲滯） |
| `Traveller.ts` | `traveller.rs`：`Traveller`，無質量探針滑行，Dopri5，每步從 split anchor 起算 |
| `Fixtures.ts` | `fixtures.rs`：`planetary_system`、`wide_seeds`、`wide_world`、`transfer`，以及 `LIGHT_YEAR`、`AU`、`YEAR` |

為此 `void-orbit` 的 `HermiteBasis` 加上 `acceleration`，`yoshida8_sequence` 改為公開。

`FrameEphemeris` 已移植到 `ephemeris.rs`：多個 view 共用 `Rc<RefCell<CoupledWorld>>` 的歷史，輸出相對選定系統質心的天體位置／速度與原點加速度，包含所有系統的引力來源。`void-orbit::EphemerisSource` 是共用介面，VesselPropagator、ContactWorld、PlanetFrame 與 Fleet 接受此來源；一般 Ephemeris 的積分方法不變。Fleet 持有 `Box<dyn EphemerisSource>`。歷史裁剪只能由共用 world 的擁有者執行，adapter 的 `forget_before` 明確 panic。

座標樹：`CoupledWorld` 本身是 `void-frames::FrameSource`（每個系統的 split 質心、每個天體相對自己的系統），`frames(origin)` 建出所有系統與天體的座標系；`FrameEphemeris` 的樹視角轉給它。example 與 multiscale-lab 經樹畫到掛在焦點下的相機。見 [frames.md](frames.md)。

`Encounter` 位於獨立的 `void-multiscale-lab`（`src/lib.rs`），用同一個 Fleet 進行碰撞與合併。`void-multiscale` 核心不依賴 vessels／assembly，既有 void-app 範例不因這個場景引入 assembly。

和 lab 的差別：錯誤一律 panic（lab 是 throw），只有 `SplitPosition::deserialize` 回傳 `Result`；天體名稱是 `Aster Star` 而不是 `Aster · Star`。

## 檢查

`cargo test -p void-multiscale`（debug 約 8 秒）。

### 與 lab 對照（`tests/golden.rs`）

對照資料由 `golden/multiscale.ts` 從 lab 產生，全部逐位元相同：

- split 運算 200 組：translate、compose、difference、relative、Kahan drift（含修正量）、超出格的進位；lab 的 JSON 可讀入，自己的 JSON 可往返。
- 緊湊三系統世界到 2000 秒，原點與 10^24 格的錨點兩種放置：每個系統的原點、速度、加速度與每個天體的狀態，以及一點的引力。
- 寬距三系統（相隔數光年、放在三萬光年外）十年的狀態，與跨光年的框架往返。
- 探針：緊湊世界 A→B 的交接；寬距世界 210 年航程（76,703 個天體步、15,354 個探針步），102.373 年從 Aster 交接到 Beryl，位置與速度跳動為 0。

寬距測試從 lab 的初始天體狀態開始：orbit 的 `build_system` 用 fdlibm 的 sin/cos 解 Kepler，V8 用自己的，兩者差一個 ulp。

### lab 的檢查（`tests/checks.rs`）

`multiscale-check.ts` 的原有檢查，加上 `tests/ephemeris.rs` 的移動原點檢查，以及 `multiscale-lab/tests/encounter.rs` 的雙船碰撞與合併，現在涵蓋原 TS 全部 15 項。使用原門檻；新測試另外檢查共用 world 延伸、各天體與批次查詢一致，以及合併線動量。印出的數字與 lab 相同：直接 N 體最大誤差 9.54e-7 m、潮汐位移 3.353 m、交接航程誤差 7.87e-3 m、76,703 天體步與 15,353 探針步、102.369 年交接。原點與 30,000 光年放置的碰撞在 40 s 都有 2.501620 m 分離、約 0.002671 m/s 相對速度，與 TS 輸出一致；比較門檻為位置 1e-4 m、速度 1e-5 m/s。

執行：`cargo test -p void-multiscale -p void-multiscale-lab`。

唯一不同是「一天步長與半天步長十年差異」：這裡 0.544 m，lab 0.564 m（門檻 1 m）。差在上面那個 ulp 的初始狀態；從 lab 的初始狀態開始，這裡也是 0.564 m。

## example `multiscale`

`cargo run -p void-app --example multiscale`：lab 頁面的 01 恆星際航行。預設暫停。

- P 開始／暫停、R 重設、N +1 天、Y +1 年、T +10 年。
- 1 恆星群（8 光年）、2 恆星系（3 AU）、3 行星（三倍半徑）、4 旅船旁（18 m）；拖曳環繞、滾輪縮放（2 m 到 30 光年）、點標籤切焦點。
- Up/Down 選設定、Left/Right 調整：世界放置（三萬光年外／原點附近）、旅船初速（0.0001–0.05 c）、時間倍率（1×、4×、每秒 1 年、每秒 10 年）、焦點、旅船座標框架（手動換框架，交接紀錄顯示 Δp、Δv）。標 `*` 的會重設場景。
- 每畫面最多 24 個探針步，追不上時顯示 ADVANCING 和目標時間；模擬時鐘只走到實際積分完成的位置。

繪圖照 lab 的 `WorldView.ts`：先扣掉焦點的 split 位置，再除以相機距離的千分之一當繪圖單位，最後才轉成 f32。天體是真實半徑的球，太小時只畫標籤上的點；恆星同一像素內的行星標籤隱藏；500 AU 內畫行星的密切橢圓（`void_view::ellipse_points`）；綠色旅船是放大的定位標記，線是最近 1000 個位置的航跡。

已在 VNC 上驗收：每秒 10 年執行，102.369 年從 Aster 交接到 Beryl（Δp、Δv 皆為 0），約 220 年到 Beryl 附近；行星與旅船旁的近距離視角正常。


## 雙船碰撞 lab

`cargo run -p void-multiscale-lab`：預設在 30,000 光年外，Aster 行星上空 400 km 的兩艘 pod＋tank，暫停開始。共用 assembly 的零件外觀，碰撞仍由 Fleet 的實際 contact world 處理。

- P 開始／暫停，N 單步 1/60 s，F 前進 10 s，T 前進 40 s，V 切換 1×／4×，R 重設。
- J 以實際接點合併；距離超過 0.25 m 明確拒絕。可先按 T 一次，讓船相撞後再合併。
- O 切換 30,000 光年外／原點附近並重設。
- 1 船旁，2 行星，3 恆星系，4 星群；拖曳環繞，滾輪縮放。小於可見尺寸的天體有定位十字。
- HUD 顯示船的 owner、scene、質量、相對位置／速度、接點距離及 split 座標。

所有繪圖座標先做 split 相減與距離縮放，再轉 f32。已通過 headless 系統存取檢查與物理測試；視窗外觀由使用者在本機驗收，agent 實作時沒有開啟視窗。

## 主遊戲多星系第一輪（work/galactic-flight，2026-10-07）

`void-app --stellar-neighborhood` 建立 Sol、Beryl、Cygnus 三個虛構恆星系；相鄰系統
相隔 4.24 與約 5.92 光年，主船仍是正常地面火箭，燃料／引擎／分級沿用既有模型。
不增加特殊引擎、傳送、蟲洞、相對論或整個銀河重力。銀河座標仍是精度配置，沒有
銀河勢能模型。天體互相積分仍與既有 orbit 一樣使用質點；天體的 J2 資料保留，
船受力則繼續使用共用 `void_orbit::gravity` 的 J2 與正確自轉軸，沒有偷偷關掉。

`--stellar-neighborhood --stellar-fixture` 是明確的驗收初始場景：Sol 地面主船、
Beryl 地面船與 Cygnus 軌道船；預設暫停。這些遠方船是初始配置，不代表火箭已完成
星際航程。Tab 切船、普通引擎／姿態／分級操作仍有效；F6/F7 存讀、record/replay
沿用同一套 command journal。Ctrl+Home 看 12 光年鄰近星系，點恆星標籤切焦點，
滾輪可縮放到 30 光年，Home 回船。HUD 顯示所選船的系統；視角改變不移動船。

### 精確狀態與 owner 接縫

- `Vessel` 與 contact `Scene` 持有 `SystemId` 和系統內 `SplitPosition` 錨點；
  Orbit 的原有 `PropagationRun` 積分錨點附近的 f64 殘量，沒有第二份船 runtime。
  接受的邊界才把殘量納入 split 錨點並重啟導數；燃料、零件圖、姿態、慣量不因此重造。
- `FrameTree` 的 split anchor 節點掛在恆星系之下。近船在星際空間也先精確相減
  split 錨點，再處理局部 f64／渲染 f32；不把數光年的絕對 f64 當接觸座標。
- 系統選擇使用 5% 遲滯。實際滑行越過界線才換框架；split 絕對位置、速度及
  慣量／零件 ID 保留。換框架不替換引力来源，所有天體仍在同一 `CoupledWorld`。
- 舊 `Fleet::snapshot` 的位置是主來源座標下的觀察值，远方可能损失小量精度，不能
  再用于物理。物理／驗證使用 `precise_snapshot` 的 split `position`、`anchor`、
  `residual`，或 `vessel_anchor_frame`、`part_frame` 与 `body_fixed_state`。
  `local` 是系統質心下的顯示值；星際殘量才是控制／積分入口。
- 物理来源只在 owner 求值期間切局部系统／split offset，結束後恢復主來源。
  讀取大氣、熱、地形、碰撞與相對速度使用相應樹／query frame。存檔拒絕仍留在
  暫時來源的非法狀態。機動計畫另外保存自己的固定 split 錨點，不隨主船重錨漂移。

### 存檔與相容性

此分支的世界描述 schema 是 4，Flight model 是 21，Fleet checkpoint 是 9；整合
分支可以再指定組合版本，不自動讀舊規則或修補舊存檔。多星系 checkpoint 必須
保存 `CoupledCheckpoint`，包含 live 狀態、Kahan 補償量、保留的 Hermite 樣本、
步長及初始 seed／天體摘要。恢復直接接續，不從起始年代重新積分數百年。缺少
耦合狀態、界線不符、seed 位置／速度／軌道、天體 J2／SOI 改變，都明確拒絕。
`LaunchSplitState` 是可錄放的明確初始狀態指令，不能冒稱是正常推進或轉移功能。

### 驗證範圍與限制

針對性測試覆蓋遠方 Orbit 船、遠方 Ground 地形／大氣、數光年處 Bubble 船的
公分相對量、對接／解除與存讀、真實滑行系統交接、遠方分級燃燒和 journal 精確
狀態核對；保留原 golden 門檻。本分支 GUI／整合及人類驗收由主 agent 另外記錄，
核心測試通過不當作這些驗收已完成。

真實普通火箭的星際旅程很長。本輪交付多星系世界、同船控制與精度接縫，不承諾
數分鐘內完成數光年航程。時間推進仍受原 rails 條件、天體／軌道步長及積分預算
限制；不得以跳時、換船或省略引力來源掩蓋效能不足。兩個鄰近星系目前使用明確
配置的星球／衛星與既有外觀資產，沒有新增逐顆專屬美術。
