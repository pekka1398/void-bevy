# 量測：時間花在哪裡

量效能只用這裡的三樣東西，不要自己另外做一套計時。

| 工具 | 看什麼 | 怎麼開 |
|---|---|---|
| DEV 面板的 PERFORMANCE | 最近 2 秒的幀時間（平均、最差）、main world 時間、模擬占多少、實際和設定倍率、GPU pass | 一般編譯就有；GPU 那行要 `--features profiling` 或 `--bench` |
| `--bench <報告>` | 固定的一組情境，每個情境同樣秒數，輸出文字報告 | 一般編譯就能跑 |
| Tracy | 每個 Bevy system、render 階段、GPU pass、我們自己的 zone，逐幀 | 用 `--features profiling` 編譯（指令在下面 Tracy 一節） |

下面的指令分兩種：標「agent」的是 agent 跑的，照 `guides/build.md`：編譯經過 `tools/slice`、用 `-j 8`；遊戲和量測直接用 `systemd-run` 放進 `void-agent.slice`，量測整段拿著編譯鎖 `/tmp/void-build.lock`，免得別的編譯插進來；畫面在 `:7`（guides/computeruse.md）。標「使用者桌面」的是使用者自己在桌面上跑的。開發編譯的 Bevy 是共用函式庫，執行檔靠 rpath 找到它，直接執行 `target/.../void-app` 就好，不用設 `LD_LIBRARY_PATH`。

## 固定情境（`--bench`）

每次合併進 master 前跑一次，和上一次的報告比較。情境定義在 `crates/app/src/fleet_game/perf.rs` 的 `SCENARIOS`：主遊戲世界，每個情境先 R 重設，只改船的起始狀態（O、DEV「放置船」用的同一個 `Action::Place`）和倍率；每個情境先用 1× 跑 4 秒（tile 載入、R 和 O 留在發射台上的船停穩；它還在動時 on-rails 加速會被拒絕），再要求情境的倍率跑 4 秒（被拒絕就再要求），然後量 10 秒。`landing` 例外：放置後整個 settle 都保持暫停（tile 照樣載入），量測開始時才放開，所以量到的 10 秒包含下降、著地（約 2 秒）和停在地上；報告裡每個情境的說明寫了量的是什麼。視窗失焦也照跑。

```bash
# agent：在 :7 上跑（guides/computeruse.md），開了以後馬上把視窗拉到 1440×900
tools/slice cargo build -j 8 -p void-app
DISPLAY=:7 flock /tmp/void-build.lock \
  systemd-run --user --scope --quiet --slice=void-agent.slice -- \
  target/debug/void-app --bench <報告.txt>

# 使用者桌面
cargo build -j 8 -p void-app && target/debug/void-app --bench <報告.txt>
```

跑的時候不要同時編譯或跑別的重東西，數字才能比較。在 Xvnc（`:7`）上，畫面每幀要從顯示卡複製到 Xvnc，present 每幀就要 25–50 ms，所以 `:7` 上的幀時間有很大一塊是 Xvnc 的成本，不是遊戲的；比較改動前後的 main、sim、gpu、倍率最可靠。報告開頭的表格每個情境一行：

- `avg/p95/worst ms`：幀時間：這一幀的 `First` 開始到下一幀的 `First` 開始，含等 render 和螢幕。
- `main ms`：main world 從 `First` 到 `Last` 的時間（遊戲邏輯、UI、畫面準備）。render 在另一條執行緒和它同時跑，所以幀時間大約是 main 和 render（含等螢幕 present）兩者較大的那個。幀時間遠大於 main 時，瓶頸在 render 或 present，要用 Tracy 看。
- `sim ms`、`sim%`：`simulate` system（推進飛行、加速、滑行預測）每幀的時間和占幀時間的比例。
- `gpu ms`：每個有 GPU 時間的幀裡，最上層 render pass 的 GPU 時間加總，再對這些幀平均；下面各情境的段落列出有幾幀、每個 pass 的平均（分母一樣是全部這些幀，某幀沒跑的 pass 算 0）和它跑了幾幀。Bevy 要 `TIMESTAMP_QUERY`、`TIMESTAMP_QUERY_INSIDE_ENCODERS` 才寫得了任何時間，大部分 pass 的時間還要 `TIMESTAMP_QUERY_INSIDE_PASSES`；三個缺任何一個，報告和 DEV 面板寫「not measured」和缺了哪幾個，不給少了一部分 pass 的總和。三個都有卻一幀都沒收到時 `--bench` 直接報錯結束。大氣和雲在同一個 shader 裡：`void_air_body` 是每個有大氣的星球（包括相機所在的）的大氣和雲，每個一次、加總；`void_air_resolve` 是最後合到畫面上的一次；地面和海是 `main_opaque_pass_3d` 裡的 `GroundMaterial`；地圖的線是 gizmo，畫在 `main_transparent_pass_3d`。GPU 時間會隨顯示卡當下的時脈變：負載低時顯示卡降頻，同一個 pass 可能量到好幾倍，比較時要看同樣條件下跑的報告。
- `requested / in effect / actual ×`：要求的倍率、警告限制後實際生效的倍率、實際每秒推進的模擬秒數。生效的比要求的低時，段落裡的 notice 寫原因。
- `RSS MiB`：情境結束時的常駐記憶體；段落裡另有整個程式的峰值。

## Tracy

Tracy 的版本要和 Bevy 用的 `tracy-client` 一致：Bevy 0.19.1 → `tracing-tracy 0.11.4` → `tracy-client 0.18.3` → `tracy-client-sys 0.27.0` → **Tracy v0.13.0**（protocol 76）。原始碼、建置腳本和編好的程式在主目錄的 `ref/tracy/`（`~/Desktop/void-bevy/ref/tracy`；`ref/` 不進 git，worktree 裡沒有，用這個路徑；`bin/` 下有 `tracy-profiler`、`tracy-capture`、`tracy-csvexport`）。升級 Bevy 後，看 `tracy-client-sys` 的 `tracy/common/TracyVersion.hpp` 換對應的 Tracy，用 `ref/tracy/build.sh` 重編。

Tracy 版一定要 `--no-default-features`，Bevy 靜態連結：開著 `dev`（Bevy 是共用函式庫）時，Tracy 的 C 程式庫被連進 `libbevy_dylib.so` 裡、沒有匯出，我們自己的 `zone!` 連結不到（`undefined symbol: ___tracy_emit_zone_begin`），所以 `dev` 和 `profiling` 一起開會直接編譯失敗並說要加什麼。因此 Tracy 版每次改動都要重新連結整個 Bevy。它放在自己的 `--target-dir target/profiling`：Bevy 的 feature 和一般編譯不同，和 `target/debug` 共用的話兩邊每次都要重編整個 Bevy。第一次從零編約 6 分鐘。要量發行版就再加 `--release`。

```bash
# 使用者桌面：開 GUI 看。先開 Tracy，再開遊戲，Tracy 會列出它，點 Connect
cargo build -j 8 -p void-app --no-default-features --features profiling --target-dir target/profiling
~/Desktop/void-bevy/ref/tracy/bin/tracy-profiler &
TRACY_NO_SAMPLING=1 target/profiling/debug/void-app

# agent：不開 GUI，錄下來再匯出（遊戲結束時 tracy-capture 自己存檔結束）
tools/slice cargo build -j 8 -p void-app --no-default-features --features profiling --target-dir target/profiling
nohup systemd-run --user --scope --quiet --slice=void-agent.slice -- \
  ~/Desktop/void-bevy/ref/tracy/bin/tracy-capture -o run.tracy -f > capture.log 2>&1 < /dev/null &
CAPTURE=$!   # 結束時只用這個數字 PID
DISPLAY=:7 TRACY_NO_SAMPLING=1 flock /tmp/void-build.lock \
  systemd-run --user --scope --quiet --slice=void-agent.slice -- \
  target/profiling/debug/void-app --bench report.txt
T=$'\t'   # zone 名稱裡有逗號，一律用 tab 分隔
~/Desktop/void-bevy/ref/tracy/bin/tracy-csvexport -s "$T" run.tracy > zones.tsv           # 每個 zone 的次數、總時間、平均
~/Desktop/void-bevy/ref/tracy/bin/tracy-csvexport -s "$T" -e run.tracy > self.tsv         # 扣掉子 zone 的 self time
~/Desktop/void-bevy/ref/tracy/bin/tracy-csvexport -s "$T" -u run.tracy > events.tsv       # 每一次 zone，含開始時間、執行緒
~/Desktop/void-bevy/ref/tracy/bin/tracy-csvexport -s "$T" -u -p -f zzz run.tracy > plots.tsv  # 只要 plot 點（-f 濾掉所有 zone）
~/Desktop/void-bevy/ref/tracy/bin/tracy-csvexport -s "$T" -g run.tracy > gpu.tsv          # 每一次 GPU pass
```

- 取樣（sampling）：不設 `TRACY_NO_SAMPLING=1` 時 Tracy 會定時取樣各執行緒的呼叫堆疊。現在可以開：Tracy 版執行檔約 440 MB（Bevy 靜態連結，工作區只有行號表），開著取樣玩 3 分鐘，遊戲記憶體停在約 1.7 GB（以前執行檔 800 MB 時會漲到 6 GB，現在沒有）。但取樣得到的不多：我們和 Bevy 的函式在 Statistics → Sampling 裡名稱是 `[unknown]`，只有檔名和行號（例如 `crates/terrain/src/noise.rs:63`），呼叫堆疊只有一層（Rust 沒有 frame pointer），只能看哪幾行最熱，看不到誰呼叫的。只要 zone 時設 `TRACY_NO_SAMPLING=1`，少掉 Tracy 自己解析符號和壓縮資料的負擔。
- 想知道哪個函式最熱、又不想編 Tracy 版時，`perf` 可以直接量一般的開發編譯（這台 `perf_event_paranoid` 是 2，量自己的程序不用 root）：`perf record -F 199 --call-graph dwarf,16384 -p <PID> -- timeout 5 tail -f /dev/null`，再 `perf report --no-children --stdio`。函式名稱完整（例如 `void_terrain::noise::noise_with_gradient` 在 `Async Compute T` 執行緒），但試過一次呼叫堆疊是空的，只有每個函式自己的時間。
- 錄一次完整的 `--bench` 約 2.5 分鐘、70 MB 檔案，`tracy-capture` 自己用到約 300 MB。

- `--bench` 在 Tracy 裡畫一條 plot `bench scenario`：量測開始時是情境編號（1 開始），量完歸 0。把 `events.tsv` 的每個 zone 依開始時間（`ns_since_start`）落在哪兩個點之間分到各情境，除以該段 `frame ms` 的點數，就是每幀的時間。`exec_time_ns` 含子 zone；要 self time 用 `-u -e`。GPU zone 的時間軸是顯示卡的時鐘換算來的，和 CPU 的不一定對齊，分情境時只當參考。
- 其他 plot：`frame ms`、`main world ms`、`simulate ms`、`warp set`、`warp actual`。
- 主執行緒上 `sub app{name=RenderExtractApp}` 的 self time 是在等 render 執行緒做完上一幀（通常是在等 `present_frames`）。
- 開 Tracy 會讓每個 zone 多花一點時間，zone 很多的情境（10k× 軌道每幀三萬多個 zone）會慢約兩成；比較快慢用一般編譯的 `--bench`，找原因才用 Tracy。

## 自己的 zone

要量一段自己的程式碼，用 `void_diagnostics::zone!("名稱");`，計到所在的 block 結束；要並排的 zone 各包一個 block。沒開 `profiling` 時它什麼都不編進去，所以熱點也可以放。目前的 zone：

- 模擬：`FlightSession::execute`、`FleetFlight::advance`、`FleetFlight::update_plans`、`FleetFlight::predict`、`Fleet::advance`（`reconcile`、`step_all`、`step_scene`、`advance_orbit`）、`Fleet::advance_on_rails`（每個 `rails chunk` 裡的 `rails_blocker`、`thermal_rails_blocker`、`ground_for`、`rails_coast_chunk_seconds`、`distant_coast_clear`、`band_safe_seconds`、`rails pair gate`、`rails propagate vessel`、`air_source`、`rails idle scene`、`commit_parachutes`、`commit_thermal`）、`propulsion_of`、`Ephemeris::extend_to`（只在真的往前積分時）
- 地形：`PlanetLod::select`、`build_tile_mesh`（背景執行緒）、`Tiles::draw`
- 地圖：`map coast path`、`map plan path`、`map body paths`、`map labels`
- 導航搜尋：`NavigationJob::solve`（`navigation search` 執行緒）

Bevy 的 system 不用自己加，`trace_tracy` 會自動記。
