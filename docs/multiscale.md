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
