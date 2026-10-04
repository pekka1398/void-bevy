# 零件狀態與資源：實作設計

本文件是 part-state-resources 分支的具體設計，尚未視窗驗收。

## 表示法

- `ResourceId` 為封閉 enum：`LiquidPropellant`、`Monopropellant`。數量全為 kg；非質量資源未實作，將來以獨立 quantity dimension 定義，不能混進質量加總。
- catalog 每個 `Module` 都有 authored `id`，同零件內唯一。Tank 宣告資源與容量；Engine 宣告單一資源需求。多同類模組以 id 尋址。
- `Part` 只有一份 `resources: BTreeMap<ResourceId,f64>` 和 `modules: BTreeMap<String, ModuleState>`。定義仍不可變；PartGraph 驗證完整 state/definition 對應和資源 domain。不存在的資源與狀態直接拒絕。
- `stage` 是 craft 的預設 action stage；`module_stages` 以 module id 保存各動作的 stage，module state 保存 activated/phase，零件的 lit/staged 只作觀察的導出值。模組身份不依賴 Vec 位置。

## 資源供應

既有 part crossfeed 為兩端都開啟才可通過，且對每種資源一樣；connection 不另存重複 crossfeed flag。可達 tank 只保留相同資源的 tank，且限 vessel members。第一版同資源多 tank 模組的容量相加，存量以 resource 在 part 保存。

池 key 為 `(resource, ordered tank IDs)`。相同池合併 demand，成員順序沿 vessel members/module catalog order。不同但部分重疊的相同資源池拒絕；目前雙向連通圖產生等价池，因此正常配置不會有該情形。扣除以池原存量比例，所有消耗者共享 `available/sum(flow)` 的耗盡時間，不依引擎先後搶油。

引擎求值純函數；accepted orbit leg 或 contact step 才提交扣量。耗盡屬正常狀態，0供應=>0輸出；非法數字不是遊戲狀態。

## 降落傘

`Stowed -> Armed -> SemiDeploying -> Semi -> FullDeploying -> Full -> Cut`。
Deploy 對 Stowed 生效；對 Armed/已部署重複 deploy 是明確冪等命令；Cut 對已armed/展開生效，Stowed cut拒絕，Cut重複cut冪等。Armed 在最低氣壓、最高動壓條件符合時開始半開；半開完成且 相對大氣基準面的高度低於 full altitude 才全開。展開面積線性按已接受 simulation seconds變化，暫停不變。

傘力 `-0.5 rho Cd A |v_air| v_air`，取該零件 position，但第一版如既有 body drag只加總力、不提交氣動力矩。居中/對稱案例作驗收；偏置力矩不在本輪，HUD與文檔明示，沒有假稱完整耦合。

狀態改變在 Fleet 接受 leg/step 前後，active 模組使用固定 accepted physics ticks；部署完成在 tick 邊界提交，保證狀態提交不出现在 Dopri trial stage。air source 複製純求值描述與初始phase時間，以 trial t取面積但不修改graph。世界包含大氣時，active 傘一律阻止 rails，即使船目前在真空中；全真空世界允許。完整軌跡的大氣穿越事件偵測留待後續，不能僅檢查終點。

## 格式

craft 2保存 resource map，craft1由明確 `migrate-craft`離線工具轉換；正常import不猜測。保存fixture的 TS golden不修改，以顯式legacy conversion讀取測試輸入。catalog schema以模块id/resource声明严格验证，身份进入fingerprint。

FleetCheckpoint version4保存资源及完整模块state；FleetFlight临时模型版本10（与multi-body协调，由root最终分配）。journal Action含part/module id与Deploy/Cut命令；world mark含全部resource/phase/elapsed，存读和重播不另存一份GUI状态。旧模型明确拒绝。

## RCS 核对

目前可达 Git 历史与当前远端只有已合入重构，无未合入 RCS/docking branch。NOTE.md提到历史WIP translate/单推进剂专用路径，不能据此视为存在。RCS未来声明Monopropellant consumer，直接使用相同供给池与accepted-step提交接口；喷嘴分配器只提供flow/force，不另开扣油。

## 影响

assembly：catalog/craft/graph验证、mass/inertia、旧独立runtime的显式legacy输入。
modules：多engine按id求值、parachute纯状态机/力、VesselAir冻结描述。
vessels：propulsion、owner accepted时间、module命令、checkpoint及snapshot。
fleet-flight：动作录放与mark、版本。world/InitialWorld属于另一个分支，不改其描述。
新增独立lab使用正式Fleet和FleetFlight接口；旧主游戏默认船不新增伞。
