# 接縫數值 lab 與案例 corpus

`void-seam-check` 是 engine-free core＋headless CLI，直接使用 Fleet、assembly、contact、orbit、multiscale 的公開介面；沒有 Bevy、OS 視窗、Node 或測試專用物理後門。它補的是跨系統接縫，不取代各 crate 的 golden／TS 行為門檻、landing 的 ground handoff／floating-origin 掃描或主遊戲存讀／錄放檢查。

```sh
# 每類 200 個，共 1200 個；固定 seed 可重現
cargo run -p void-seam-check -- --count 200 --seed 24301
# 執行前將完整輸入保存，程序被終止仍可重跑
cargo run -p void-seam-check -- --count 20 --write-cases lab-log/seam-inputs
# 單一案例：接受 input case 或 failure artifact
cargo run -p void-seam-check -- --case lab-log/seam-failures/<case>.json
# 修復後重跑已保存的整個 corpus，任何失敗都 exit 1
cargo run -p void-seam-check -- --corpus lab-log/seam-failures
```

案例包含 schema／fixture VERSION、seed、index 和**實際數值輸入**。重跑不重新從 seed 產生狀態，故 generator 改變不會換掉案例。巨大座標的 i128 cells 用十進位字串，f64 JSON 使用 workspace float_roundtrip。版本或未知欄位明確拒絕；fixture／解讀改變時須提高 VERSION，沒有隱性 migration。`--case` 與 `--corpus` 不能同時使用，corpus JSON 必須非空且逐檔解析；未知 CLI 選項直接失敗。

每個失敗將輸入與 panic 原因寫入預設 `lab-log/seam-failures/`（可用 `--failure-dir` 指定），sync 至磁碟。採 create_new 並另加序號，重跑不覆寫原案例。捕捉 panic 只是為了保存證據；CLI 繼續掃剩餘案例，最後仍以失敗 exit code 結束，不把錯誤變成成功。預設只在失敗後寫出，若要保留 abort／kill 前的输入，使用 `--write-cases`，它會在執行任何案例之前先寫完全部輸入。寫入失敗直接報錯。

| 家族 | 參數與接縫 | 檢查 |
| --- | --- | --- |
| Fleet 分離 | 200–1000 km 軌道、任意三維姿態、速度增量、三軸旋轉；demo craft 的 decoupler | 瞬間線動量 <0.1 kg·m/s、角動量 <0.01 kg·m²/s，全部 part pose／fuel／mass 保留、兩船產生 |
| 本地 join | 三維姿態、相對速度、雙船 spin、0.01–0.2 m node gap | 共享 contact bubble；join 瞬間線動量 <0.1、角動量 <0.005，part position <5e-5 m、rotation <1e-6 rad，graph／free-node／fuel／mass |
| 遠方 join | 同上，使用 Beryl 的 FrameEphemeris＋CoupledWorld；0 或極大 i128 cells | 同一門檻；另測巨大平移不改局部數值結果 |
| orbit→bubble→orbit | 3750–3950 m 起始距離、8.75–9.25 m/s 接近速度、25–35 m miss | 確認真有進／出 owner；每 10 s 比對獨立 VesselPropagator，700 s 全程 position <0.03 m、velocity <3e-5 m/s |
| split／frame | 任意公尺級位置、100 m/s 內三軸速度、不同時間與 i128 cells | Aster↔Beryl 100 次來回；每次絕對位置 <2e-6 m、velocity <1e-9 m/s |
| 星系交接 | 三個小型系統、不同離開位置／速度與巨大平移 | Traveller 真正 A→B；每 1000 s 比對**未分組**直接 N-body 至9000 s，position <0.02 m；每次 event jump <2e-6 m／1e-8 m/s |

動量只在瞬間拓撲操作前後比較，沒有把重力、推進、耗油或碰撞後耗散誤當成守恆義務。遠方共用接觸場景是數值／debug join，不宣稱已新增真正 docking capture／RCS。上述門檻沿用 `vessels/tests/fleet.rs` 與 `multiscale/tests/checks.rs`，沒有為掃描放寬。

本批 seed 24301、每類 200 個，共 1200 passed／0 failed：Fleet 分離最大線動量誤差 2.93e-5、角動量 1.28e-4；join 最大角動量 1.01e-5，本地 part position 3.08e-5 m；遠方 join 最大 part position 1.72e-5 m；交會最大 position 5.12e-3 m／velocity 1.73e-5 m/s；frame roundtrip 5.97e-8 m；Traveller／直接 N-body 2.65e-3 m。這是已掃參數範圍的證據，不宣稱任意船型／速度／時間尺度都無 bug。

預設測試另有 30 個經序列化再解析的物理案例、巨大平移一致、輸入 export→完整 corpus CLI、以及真實拒絕的非法初始姿態保存→跨程序重跑。保存／重跑測試會要求非零 exit 與原錯誤保留，防止工具把失敗吞掉。一般 workspace 不必執行 1200 案例；重型掃描由上列 CLI 明確執行。

本批最終驗證：`cargo test --workspace --all-targets -j 2` 為 **280 passed、0 failed、4 ignored**；`cargo clippy --workspace --all-targets -j 2 -- -D warnings`、fmt 與 diff whitespace 檢查通過。seam-check 自身 7 項測試全數通過。1200 案例的重型 sweep 已另外執行，不灌入 workspace 測試數。
