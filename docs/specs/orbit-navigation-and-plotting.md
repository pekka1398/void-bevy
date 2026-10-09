# Orbit 自動導航、繪圖參照系與 AN／DN

討論定案：2026-10-09。狀態：已確認需求，尚未實作／驗收。
共用開發與架構規則依 repository 根目錄 `AGENTS.md`。

## 已確認的結論

1. 自動導航採分階段生成機動節點：選目標天體後生成出發節點；中途修正與抵達捕獲各自提供操作，依操作當時的狀態重新計算。不要一次預先固定整趟旅程的所有節點。
2. 自動填入節點時間、參考天體與 prograde／normal／radial Δv；沿用現有節點手動編輯及執行流程。生成節點本身不自動點火。
3. 解不必最優或極精確，不要求全面處理 N 體混沌；仍須用現有物理預測呈現所得結果，不把簡化求解的目標宣稱成實際已達成的接近。
4. 繪圖參照系先提供四類：系統質心慣性、指定天體中心慣性、指定天體自轉、指定雙天體共轉。
5. AN／DN 跟隨所選繪圖參照系的參考平面，沿數值預測軌跡找穿越點，不另設獨立平面選擇器。
6. 實作次序：參照系 → AN／DN → 自動導航。新功能使用 branch／worktree；主 agent 可依範圍自行實作或委派 subagent。小修正不另開 worktree 或 agent。

## 自動導航範圍與接口

- 目標為目前系統中的天體；初版不包含船艦交會、對接、起飛導航或降落導航。
- 三個操作：前往目標（出發）、修正接近、捕獲。每次操作生成其所需的一個節點；不自動串成全程 autopilot。
- 出發搜尋允許等待合適窗口。搜尋窗口、到達時間範圍及目標近心高度的預設值由實作者制定；應提供可理解的設定／結果，而非要求使用者理解求解器參數。
- 生成後呈現可用的預估：節點時間、總 Δv、接近／抵達時間、最近距離或近心高度、相對速度。結果依適用情況標示，不混淆距離與高度。
- 捕獲須先有適用的接近狀態／預測；不得在缺乏條件時偷偷改成別種操作。說明拒絕原因。
- 保留既有節點，不默默覆寫；新增位置、既有後續節點的相容性與選取流程由實作者設計並明確呈現。
- 可採簡化二體／Lambert／轉移窗口估算產生候選，再用既有 N 體、有限燃燒預測檢查及有限次修正。具體演算法不在本 spec 鎖定。
- 使用現有 ManeuverSpec、FlightPlan、引擎與燃料語義。不可新增平行船 runtime，不能用瞬間 Δv 執行取代既有有限時間燃燒。
- 合法但無解、預測撞擊、燃料不足、搜尋超出範圍等狀況提供明確結果／拒絕原因；非法內部狀態依 AGENTS.md 明確報錯。
- 求解不改真實船狀態、不耗油；耗油只在既有接受步／執行流程提交。錄放與存檔依既有權威命令、穩定 ID 及版本規則處理。
- 不以本工作承擔積分器效能優化、最佳轉移、重力助推序列或長期衛星系穩定性研究。

## 繪圖參照系

| 類型 | 原點／軸向與參考平面 |
| --- | --- |
| 系統質心慣性 | 所選系統質心；固定慣性軸，參考平面為系統黃道基準面 |
| 天體中心慣性 | 所選天體中心；不隨自轉旋轉，軸向以該天體赤道基準定義 |
| 天體自轉 | 所選天體中心；隨天體自轉，參考平面為赤道面 |
| 雙天體共轉 | 兩天體質心；連心線與相對軌道法向定義軸，參考平面為兩天體軌道面 |

- 核對既有 FrameSpec／void-frames 定義，尤其天體中心慣性系是否已對齊赤道；不得只改 UI 名稱而留下不符的軸。
- 每個歷史／未來軌跡樣本依自己的時間轉換；不可把整條軌跡僅用目前時刻的旋轉搬移。
- 同時間的天體、船、計畫軌跡、Pe／Ap、AN／DN、機動標記使用一致的繪圖轉換。
- 切換影響呈現，不改積分框架、船物理狀態或燃燒指令語義。
- 明示目前模式、所選天體／天體對、參考平面；相機焦點與導航目標不應被切換操作隱性改寫。
- 天體對須不同且位於可支援的同一系統。軌道法向退化等無法定義的狀況明確處理，不任意替換天體或軸。

## AN／DN

- 將預測軌跡在各自時間轉入繪圖參照系，找 z=0 的穿越。朝正法向穿越為 AN，朝負法向穿越為 DN。
- 使用參照系中的位置與速度／時間導數，旋轉與平移參照系的速度變換必須一致。
- 求根沿數值軌跡進行，可參考 Principia 的位置／速度 Hermite 插值與括區求根；不假定整段預測為單一 Kepler 橢圓。
- 預測多圈時允許多個穿越點，按可讀性限制顯示量；不要硬限制為一對。
- 標記提供 AN／DN、到達時間、穿越平面的速度；存在有意義的中心時提供表觀傾角並標明其語義。
- 共面或切觸不得生成不穩定、反覆跳動的偽節點；未預測到穿越就不畫。數值容差由實作者訂定並測試。
- 赤道交點離天體太遠時，可採有物理依據的相關性限制，參考 Principia；具體門檻尚未定案，不能假裝使用者已指定。
- 接到現有地圖標記／選取流程；可將機動節點定位到交點，時間語義須符合有限燃燒的既有規則。

## 驗收與驗證

- 參照系：同一軌跡在四模式下呈現合理，天體與標記一致；旋轉系使用逐時樣本轉換，切換不改物理狀態。
- AN／DN：已知傾斜軌道的交點、方向、時間正確；覆蓋共面、切觸、無穿越、多圈與動態參照平面。
- 自動導航：至少覆蓋行星到衛星、同一恆星系內行星轉移，以及有效接近下的修正與捕獲；展示實際預測結果和明確失敗原因，不宣稱所有天體／所有出發狀態都可解。
- 核對燃料不足、已有計畫、選取／切換船、存檔與錄放的受影響接縫；生成不得提交船狀態或耗油。
- 核心邏輯留在合適 core crate，app 負責控制與呈現。先做受影響 crate／場景的測試與 lint，編譯 -j 2；不自動跑全 workspace。
- 提供可玩的主遊戲與驗收操作。agent GUI 檢查使用 TigerVNC，人類最終驗收另行記錄；不以截圖宣稱已人類驗收。

## 本地參考資料（絕對路徑）

以下外部 repo 已由使用者授權 clone 至 vendor，供閱讀參考；未加入 Cargo 依賴。方法應適配 VOID 的 N 體／有限燃燒模型，直接搬用程式碼前須核對各 repo 授權。

### MechJeb2

- Repo：`/home/pekka/Desktop/void-bevy/vendor/MechJeb2`
- 閱讀版本：`cadbe3d12d2f8fb6801f86d2bc1025ce4c198eec`
- `/home/pekka/Desktop/void-bevy/vendor/MechJeb2/MechJeb2/MechJebModuleManeuverPlanner.cs`
- `/home/pekka/Desktop/void-bevy/vendor/MechJeb2/MechJeb2/Maneuver/OperationTransfer.cs`
- `/home/pekka/Desktop/void-bevy/vendor/MechJeb2/MechJeb2/Maneuver/OperationAdvancedTransfer.cs`
- `/home/pekka/Desktop/void-bevy/vendor/MechJeb2/MechJeb2/Maneuver/OperationCourseCorrection.cs`
- `/home/pekka/Desktop/void-bevy/vendor/MechJeb2/MechJeb2/Maneuver/TransferCalculator.cs`
- `/home/pekka/Desktop/void-bevy/vendor/MechJeb2/MechJeb2/OrbitalManeuverCalculator.cs`
- `/home/pekka/Desktop/void-bevy/vendor/MechJeb2/MechJebLib/Maneuvers/InterplanetaryTransfer.cs`
- `/home/pekka/Desktop/void-bevy/vendor/MechJeb2/MechJebLibTest/ManeuversTests/InterplanetaryTransferTests.cs`

閱讀結論：一般轉移可依條件生成一或兩個節點；直接天體轉移並非一般就生成捕獲。此版本 AdvancedTransfer 最後 OptimizeEjectionToTarget 回傳單一出發節點，include capture burn 不等於建立捕獲節點。修正是獨立操作。
上游：https://github.com/MuMech/MechJeb2

### Principia

- Repo：`/home/pekka/Desktop/void-bevy/vendor/Principia`
- 閱讀版本：`0feb271b24a2a0c9ab200e34766711265d220039`
- `/home/pekka/Desktop/void-bevy/vendor/Principia/ksp_plugin_adapter/reference_frame_selector.cs`：模式與參考平面描述。
- `/home/pekka/Desktop/void-bevy/vendor/Principia/ksp_plugin/plugin.cpp`：ComputeAndRenderNodes，逐時轉換後求交點、赤道相關性限制。
- `/home/pekka/Desktop/void-bevy/vendor/Principia/physics/apsides_body.hpp`：ComputeNodes，z 變號、Hermite 插值與求根。
- `/home/pekka/Desktop/void-bevy/vendor/Principia/physics/apsides_test.cpp`：交點測試。
- `/home/pekka/Desktop/void-bevy/vendor/Principia/ksp_plugin_adapter/map_node_pool.cs`：AN／DN 標記、法向速度與表觀傾角資訊。

上游：https://github.com/mockingbirdnest/Principia

### VOID 現有接線

- `/home/pekka/Desktop/void-bevy/crates/orbit/src/reference_frames.rs`
- `/home/pekka/Desktop/void-bevy/crates/orbit/src/flight_plan.rs`
- `/home/pekka/Desktop/void-bevy/crates/orbit/src/simulation.rs`
- `/home/pekka/Desktop/void-bevy/crates/frames/src/`
- `/home/pekka/Desktop/void-bevy/crates/fleet-flight/src/plans.rs`
- `/home/pekka/Desktop/void-bevy/crates/fleet-flight/src/presentation.rs`
- `/home/pekka/Desktop/void-bevy/crates/app/src/map.rs`
- `/home/pekka/Desktop/void-bevy/crates/app/src/fleet_game.rs`
- `/home/pekka/Desktop/void-bevy/crates/app/src/fleet_game/ui.rs`

擴充天體及近期標籤修正在尚未合併的 `/home/pekka/Desktop/void-bevy-bodies`，branch `work/expanded-bodies`。開始新功能前核對當時 Git 狀態與整合基底；不可將本 spec 當成合併／push 授權，也不要覆蓋其他未完成工作。
