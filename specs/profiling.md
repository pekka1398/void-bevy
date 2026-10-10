# 量測工具：知道遊戲的時間花在哪裡

## 問題

現在沒辦法知道遊戲跑的時候，各個階段和功能各花多少時間、瓶頸在哪裡：

- `--profile`（`void_diagnostics::Profiler`）是自己寫的，整個遊戲只有 4 個量測點。
- 看不到 Bevy 每個 system、render 每個階段花的時間。
- 完全沒量 GPU 時間。
- 之前找瓶頸只能用 gdb 暫停程式取樣，結果很粗，也不能每次都這樣做。

## 要做的

### 1. CPU：Bevy 內建的 tracing 接 Tracy

- 用 Bevy 的 `trace_tracy` feature，在 `void-app` 開一個 cargo feature 控制。平常編譯不開，不影響效能。
- 開了以後每個 Bevy system、render 階段、每幀都有時間紀錄。另外在我們自己的熱點加 span，至少包括：
  - 模擬：`FlightSession::execute`、`advance_on_rails` 和裡面的各項檢查、船的推進、星曆延伸；
  - 地形：LOD 選擇、tile 生成；
  - 地圖：軌道線和預測；
  - 導航搜尋（背景執行緒）。
- Tracy 的 GUI 版本要和 Bevy 用的 `tracy-client` 對得上。原始碼和編好的程式放在 `ref/`。
- 要能不開 GUI 直接錄下來（`tracy-capture`），再匯出成文字（`tracy-csvexport`），讓 agent 自己看結果。
- 舊的 `void_diagnostics::Profiler` 和 `--profile` 在同一個 commit 刪掉。如果 `void-diagnostics` crate 因此變空，整個刪掉。
  - 實際：`Profiler`、`--profile`、F9 都刪了；`void-diagnostics` 沒有變空，改放 `zone!`／`plot!` 兩個 macro（開 feature 時是 Tracy zone／plot，沒開時什麼都不編進去），各個 crate 的熱點都用它。

### 2. GPU：每個 pass 的時間

- 用 Bevy 內建的 render diagnostics（timestamp query）量 GPU 上每個 pass 花多久，包括大氣、雲、地面、海洋、地圖。
- 這些數字要出現在 Tracy 裡，或者出現在下面的 DEV 數字裡。
  - 實際：兩邊都有。開 feature 時 Bevy 自動加上 render diagnostics，GPU pass 出現在 Tracy 的 GPU 列；DEV 面板和 `--bench` 報告讀同一份資料（一般編譯只有 `--bench` 會打開 timestamp query）。大氣和雲是同一個 shader，分成 `void_air_body`（每個有大氣的星球一次）和 `void_air_resolve`，沒辦法再把雲單獨拆開。

### 3. 遊戲內的 DEV 數字

DEV 面板固定顯示：

- 幀時間（平均和最差）；
- 一幀裡模擬占了多少時間；
- 實際加速倍率和設定倍率。

照 `guides/ui.md`，數字的位置固定，不會因為狀態改變而移動。

- 實際：幀時間是這一幀 `First` 開始到下一幀 `First` 開始（自己量，不用 Bevy 在幀開始前算好的 delta），所以最新一筆永遠是上一幀。replay 裡同一幀有 R 或讀檔時，實際倍率用該幀 `Advance` 指令推進的模擬秒數，不用時鐘差。

### 4. 固定的量測情境

用一個指令跑完一組情境，每個情境跑固定秒數，最後輸出一份文字報告，列出幀時間、模擬時間、GPU 時間、實際倍率和記憶體。照 AGENTS.md 第 8 條，情境只改船的起始狀態，世界就是主遊戲的世界：

| 情境 | 內容 |
|---|---|
| 地面 | 發射台上，1× |
| 低軌道 | 按 O 進入 Aurelia 軌道，10k×、100k× |
| 遠距離 | 離 Aurelia 100 萬 km，100k× |
| 地圖 | 低軌道，打開地圖 |
| 近看有大氣的星球 | 低空，有雲、海 |
| 起降 | 再入大氣到降落 |

實際做法（`--bench <報告>`，見 `guides/profiling.md`）：每個情境先 R 重設，再用 O 或 DEV「放置船」的 `Action::Place` 擺好船；先 1× 跑 4 秒讓發射台上的船停穩，再要求倍率跑 4 秒，然後量 10 秒。起降拆成兩個情境：`reentry`（40 km、1,800 m/s、往下 12°、4×）和 `landing`（離地 30 m、往下 3 m/s、1×；settle 期間保持暫停，量測開始才放開，所以量測段包含下降、觸地和之後停在地上），因為一個 10 秒的量測段裝不下整段再入到降落。地圖情境用 1×。100k× 低軌道會被遊戲本身的軌道高度限制降到 10k×，報告照實寫出要求和生效的倍率。

這組情境之後每次合併都要跑，用來比較改動前後的數字。怎麼跑寫進 `guides/`（例如 `guides/profiling.md`），讓之後的 agent 知道要用它，不要自己另外做一套。

## 不做

- 不修任何效能問題。這份 spec 只做量測。量完把結果交給我，我再決定先修哪裡（`specs/warp-speed.md` 是候選之一）。

## 驗收

1. 開 Tracy feature 跑遊戲，我在 Tracy GUI 裡看得到各 system、自己加的 span、GPU pass 的時間。
2. DEV 面板看得到幀時間、模擬時間、實際倍率。
3. agent 跑一次固定情境，交出報告，並根據報告說明目前各情境的瓶頸在哪裡。
4. 不開 feature 的一般編譯，啟動時間和加速倍率不比現在差。
