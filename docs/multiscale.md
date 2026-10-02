# multiscale：從公分到光年的座標與耦合

`crates/multiscale`（`void-multiscale`）移植 `lab/multiscale` 的座標與物理，不依賴 Bevy。

| TS | Rust |
| --- | --- |
| `SplitPosition.ts` | `split.rs`：`SplitPosition`，每軸整數格（2^32 m）加 float64 offset。lab 的格是 bigint，這裡是 `i128`（±1.7e38 格） |
| `CoupledWorld.ts` | `world.rs`：`CoupledWorld`，多個恆星系在同一個直接 N 體世界；系統質心用 split 位置，天體用局部 float64；Yoshida-8 與五次 Hermite 取自 `void-orbit` |
| `Frames.ts` | `traveller.rs`：`FramedState`、`absolute`、`reframe`、`nearest_frame`（5% 遲滯） |
| `Traveller.ts` | `traveller.rs`：`Traveller`，無質量探針滑行，Dopri5，每步從 split anchor 起算 |
| `Fixtures.ts` | `fixtures.rs`：`planetary_system`、`wide_seeds`、`wide_world`、`transfer`，以及 `LIGHT_YEAR`、`AU`、`YEAR` |

為此 `void-orbit` 的 `HermiteBasis` 加上 `acceleration`，`yoshida8_sequence` 改為公開。

還沒移植：lab 的 `FrameEphemeris` 與 `Encounter`（兩艘 assembly 船在遠方恆星系相撞、debug 合併）。它們把 `vessels` 的 `Fleet` 放進隨系統質心移動的座標系，需要 orbit 的 `Ephemeris` 能換成這種來源（lab 是繼承 `Ephemeris`）。這會動到另一個 session 正在做的 `void-vessels`，等它完成後再做。

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

`multiscale-check.ts` 15 項中的 12 項，門檻相同；另外 3 項需要 `FrameEphemeris` 與 `Fleet`。印出的數字與 lab 相同：直接 N 體最大誤差 9.54e-7 m、潮汐位移 3.353 m、交接航程誤差 7.87e-3 m、76,703 天體步與 15,353 探針步、102.369 年交接。

唯一不同是「一天步長與半天步長十年差異」：這裡 0.544 m，lab 0.564 m（門檻 1 m）。差在上面那個 ulp 的初始狀態；從 lab 的初始狀態開始，這裡也是 0.564 m。

## example `multiscale`

`cargo run -p void-app --example multiscale`：lab 頁面的 01 恆星際航行。預設暫停。

- P 開始／暫停、R 重設、N +1 天、Y +1 年、T +10 年。
- 1 恆星群（8 光年）、2 恆星系（3 AU）、3 行星（三倍半徑）、4 旅船旁（18 m）；拖曳環繞、滾輪縮放（2 m 到 30 光年）、點標籤切焦點。
- Up/Down 選設定、Left/Right 調整：世界放置（三萬光年外／原點附近）、旅船初速（0.0001–0.05 c）、時間倍率（1×、4×、每秒 1 年、每秒 10 年）、焦點、旅船座標框架（手動換框架，交接紀錄顯示 Δp、Δv）。標 `*` 的會重設場景。
- 每畫面最多 24 個探針步，追不上時顯示 ADVANCING 和目標時間；模擬時鐘只走到實際積分完成的位置。

繪圖照 lab 的 `WorldView.ts`：先扣掉焦點的 split 位置，再除以相機距離的千分之一當繪圖單位，最後才轉成 f32。天體是真實半徑的球，太小時只畫標籤上的點；恆星同一像素內的行星標籤隱藏；500 AU 內畫行星的密切橢圓（`void_view::ellipse_points`）；綠色旅船是放大的定位標記，線是最近 1000 個位置的航跡。

已在 VNC 上驗收：每秒 10 年執行，102.369 年從 Aster 交接到 Beryl（Δp、Δv 皆為 0），約 220 年到 Beryl 附近；行星與旅船旁的近距離視角正常。
