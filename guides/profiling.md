# 量測：時間花在哪裡

量效能只用這裡的三樣東西，不要自己另外做一套計時。

| 工具 | 看什麼 | 怎麼開 |
|---|---|---|
| DEV 面板的 PERFORMANCE | 最近 2 秒的幀時間（平均、最差）、模擬占多少、實際和設定倍率、GPU pass | 一般編譯就有；GPU 那行要 `--features profiling` 或 `--bench` |
| `--bench <報告>` | 固定的一組情境，每個情境同樣秒數，輸出文字報告 | 一般編譯就能跑 |
| Tracy | 每個 Bevy system、render 階段、GPU pass、我們自己的 zone，逐幀 | `cargo build -p void-app --features profiling` |

## 固定情境（`--bench`）

每次合併進 master 前跑一次，和上一次的報告比較。情境定義在 `crates/app/src/fleet_game/perf.rs` 的 `SCENARIOS`：主遊戲世界，每個情境先 R 重設，只改船的起始狀態（O、DEV「放置船」用的同一個 `Action::Place`）和倍率；每個情境先等 4 秒（tile 載入、警告解除），再量 10 秒。

```bash
cargo build -j 2 -p void-app
# 在 :7 上跑（guides/computeruse.md），視窗拉到 1440×900；失焦也照跑
DISPLAY=:7 systemd-run --user --scope --quiet --slice=void-agent.slice -- \
  target/debug/void-app --bench <報告.txt>
```

跑的時候不要同時編譯或跑別的重東西，數字才能比較。報告開頭的表格每個情境一行：

- `avg/p95/worst ms`：幀時間（兩幀開始之間的實際時間）。
- `sim ms`、`sim%`：`simulate` system（推進飛行、加速、滑行預測）每幀的時間和占幀時間的比例。
- `gpu ms`：最上層 render pass 的 GPU 時間總和；下面各情境的段落列出每個 pass。大氣和雲是同一個 pass `void_air`（每個有大氣的星球一次）；地面和海是 `main_opaque_pass_3d` 裡的 `GroundMaterial`；地圖的線是 gizmo，畫在 `main_transparent_pass_3d`。
- `requested / in effect / actual ×`：要求的倍率、警告限制後實際生效的倍率、實際每秒推進的模擬秒數。生效的比要求的低時，段落裡的 notice 寫原因。
- `RSS MiB`：情境結束時的常駐記憶體；段落裡另有整個程式的峰值。

## Tracy

Tracy 的版本要和 Bevy 用的 `tracy-client` 一致：Bevy 0.19.1 → `tracing-tracy 0.11.4` → `tracy-client 0.18.3` → `tracy-client-sys 0.27.0` → **Tracy v0.13.0**（protocol 76）。原始碼、建置腳本和編好的程式在 `ref/tracy/`（`bin/` 下有 `tracy-profiler`、`tracy-capture`、`tracy-csvexport`）。升級 Bevy 後，看 `tracy-client-sys` 的 `tracy/common/TracyVersion.hpp` 換對應的 Tracy，用 `ref/tracy/build.sh` 重編。

```bash
cargo build -j 2 -p void-app --features profiling --target-dir target/profiling

# 開 GUI 看（使用者在自己的桌面）：先開遊戲，再在 Tracy 按 Connect
target/profiling/debug/void-app &
ref/tracy/bin/tracy-profiler

# agent 不開 GUI：錄下來再匯出成 CSV
ref/tracy/bin/tracy-capture -o run.tracy -f -s 120 &      # 等遊戲連上，最多錄 120 秒
DISPLAY=:7 target/profiling/debug/void-app --bench report.txt
ref/tracy/bin/tracy-csvexport run.tracy > zones.csv          # 每個 zone 的次數、總時間、平均
ref/tracy/bin/tracy-csvexport -e run.tracy > self.csv        # 扣掉子 zone 的 self time
ref/tracy/bin/tracy-csvexport -u -p run.tracy > events.csv   # 每一次 zone 和 plot 點，含時間戳
```

- `--bench` 在 Tracy 裡畫一條 plot `bench scenario`：量測期間是情境編號（1 開始），量完歸 0。用 `-u -p` 的時間戳就能把 zone 分到各個情境。
- 其他 plot：`frame ms`、`simulate ms`、`warp set`、`warp actual`。
- 開 Tracy 會讓每個 zone 多花一點時間（幾十到幾百 ns），量出來的倍率會比一般編譯低；比較快慢用一般編譯的 `--bench`，找原因才用 Tracy。

## 自己的 zone

要量一段自己的程式碼，用 `void_diagnostics::zone!("名稱");`，計到所在的 block 結束；要並排的 zone 各包一個 block。沒開 `profiling` 時它什麼都不編進去，所以熱點也可以放。目前的 zone：

- 模擬：`FlightSession::execute`、`FleetFlight::advance`、`FleetFlight::update_plans`、`FleetFlight::predict`、`Fleet::advance`（`reconcile`、`step_all`、`step_scene`、`advance_orbit`）、`Fleet::advance_on_rails`（每個 `rails chunk` 裡的 `rails_blocker`、`thermal_rails_blocker`、`ground_for`、`rails_coast_chunk_seconds`、`distant_coast_clear`、`band_safe_seconds`、`rails pair gate`、`rails propagate vessel`、`air_source`、`rails idle scene`、`commit_parachutes`、`commit_thermal`）、`propulsion_of`、`Ephemeris::extend_to`（只在真的往前積分時）
- 地形：`PlanetLod::select`、`build_tile_mesh`（背景執行緒）、`Tiles::draw`
- 地圖：`map coast path`、`map plan path`、`map body paths`、`map labels`
- 導航搜尋：`NavigationJob::solve`（`navigation search` 執行緒）

Bevy 的 system 不用自己加，`trace_tracy` 會自動記。
