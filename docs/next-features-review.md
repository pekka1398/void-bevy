# Root 四功能審查與完成條件核對

程式來源 `c7c638b`，分支 `work/four-feature-integration`；主線沒有合併本輪。
功能分支最終來源：EVA continuation `4e11a13`、aircraft `31bc395`、stellar `4c541a0`、
water `38ffd9b`。各自 worktree／branch 保留，root 整合了共用接口與審查修正。
操作見 [驗收入口](next-features-acceptance.md)，實際驗證範圍見 [status](status.md)。

## 需求與權威證據

| 完成條件 | 程式／資料證據 | 已檢查的行為 |
| --- | --- | --- |
| 分支／worktree／subagent 直接修改主遊戲 | 各功能分支 commits；crates/app/src/fleet_game.rs；各 core 的 git diff | 四項接同一主遊戲，沒有用另一套 demo 代替；昂貴 root 編譯固定 -j2 |
| Assembly 是可擴充核心，未要求 VAB | assembly/model.rs、graph.rs、runtime.rs、aircraft.rs、crew.rs、wheel.rs；data/catalog.json | surface attachment／mirror、合法 nodes／幾何、碰撞／慣量／外觀、資源／穩定 module IDs；assembly geometry/graph/wheel tests |
| EVA 出入座、步行、跳躍、背包、控制權 | vessels/fleet.rs、fleet/eva.rs；assembly/crew.rs；fleet-flight/session.rs；app profile 操控／avatar | 地面實際移動／跳躍／回座，Orbit 背包實際運動與耗用；eva/eva_stellar tests 檢查 crew identity、隔離燃料、COM/P/L、checkpoint/journal、跨星系拒絕 |
| rover 輪胎、懸吊、轉向、驅動、煞車、真實地形接觸 | assembly/wheel.rs；vessels 的 wheel/contact 邏輯；landing/contact_world.rs；app accepted wheel graphics | raycast 真實 native collider、reciprocal point impulses、friction circle、incline park/release、sleep/wake、finite steering；Ground/Orbit rotor momentum／rails refusal；main GUI 駕駛／轉向／煞停 |
| 飛機可滑行、起飛、控制、降落 | assembly/aircraft.rs；modules/air.rs、jet；vessels aero/owner；app profile 控制／AIR 資訊 | headless taxi/takeoff/approach/touchdown/brake；最終 GUI 全流程、存讀及真實 journal verify；世界向量的 W/D/E 及實際 taxi 方向 regression |
| 水依排水與質量，含部分浸水／力矩／阻力 | environment SeaSample；modules/water.rs；fleet-flight/water.rs；vessels water owner/substeps | cuboid/cylinder/cone clipped volume、部分浸水、float/sink、偏心力矩、100m/s entrance、受控 substeps、air-mode equality、checkpoint；GUI 真實濺落及35s漂浮 |
| 多星系沿用普通火箭，具局部精度／框架／多船／保存 | frames split/tree；multiscale world/ephemeris；vessels owner anchors；fleet-flight/world.rs、checkpoint.rs；app/map.rs | 三系真實光年間隔、远方 Ground/Orbit/Bubble、连续边界跨越、source query/wrench frames、COM/P/L、对接/分級/crew/存讀/錄放；GUI 三系地圖／切船、普通引擎／有限 RCS |
| 共用 runtime／環境／框架，trial 不提交，accepted 才改狀態 | 同一 PartGraph 與 Fleet owners；query scopes／Wrench frames；water accepted substeps；rotor rotation legs | owner mass/inertia、dry contact history、resource purity、相對小量、rotor angular momentum、water air modes；targeted tests 未放寬原門檻 |
| 可直接執行、有觀察資訊與操作文件 | scripts/run-next-features.sh；target/acceptance/manifest.json／SHA256SUMS；各 worktree target/acceptance/run-reviewed-combination.sh | compiled source SHA256 核對、主遊戲 HUD/profile/實際 collider；入口明示跑 root 組合版 |
| 版本、相容性、證據與人類驗收區分 | model26、checkpoint12、world4、Craft3；status／specs／review archive | 缺必要 wheel state／非法 frame／非有限量／舊版本拒絕；headless、agent GUI、未進行人類驗收及未合併分開記錄 |

## Root 審查後的修正

共用 owner 的遠端 query frame／escaped aerodynamic Wrench、海水求值與推進步長、
EVA stellar transfer、共用 wheel 原點／睡眠／solver delta、accepted steering 和 airborne
rotor angular momentum 都已整合並測試。沒有以放寬門檻、開理想 reaction wheel、
更換海面／地形或傳送船來掩蓋接縫。真正非法狀態／格式明確拒絕；fixture 明示起始配置。

Main rendering 補上有乘員指令艙的 authored render recipe；profile navball、低動壓
AIR、三星 root label／click hitbox 修正。最後飛機方向測試獨立使用 nose/top 的世界
向量；歷史 wing-side 名稱不能作物理左右的證據。VNC 間歇畫面矩形僅有重繪後消失
及正常 GPU 配對圖的證據，未宣稱根因已修好，保留作 human GUI 驗收注意點。

## 最終檢查與保留限制

受影響九 core 的 lib/tests：345 passed、0 failed、既有 Pebble slope ignored 1；
app lib：28 passed。core/app all-targets Clippy -D warnings、fmt、Bevy main binary build
通過，沒有全 workspace 重跑。真實操作錄影按其來源 model26、保存 catalog 和最終
binary verifier 核對，來源時間點和涵蓋範圍在 status 清楚記錄。

最終二進位 SHA256：
`fab9667f72cdfecfb01f91c77cf123d49a1eaad13c8dcc084ebaedd08dd52009`。
實際程式來源 `c7c638b`；後續 docs/launcher 及 water unit-test 排序提交不改 production 內容；
非 test token 一致證據保存於 model26-test-layout-proof.json。

本輪不包含攀爬／艙內／游泳、rover 電力、VAB 前端、複雜流體、結構破壞、特殊
星際推進／相對論／完整銀河重力。主遊戲完整 Sol 到 Beryl 航程未驗證；小段連續
邊界測試與初始 fixture 不冒充完整航程。Native Rapier 仍為 f32／局部坐標，公開
世界／船接口使用 f64。人類最終驗收和合入 master 仍待使用者另行處理。
