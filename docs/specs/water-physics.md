# 靜止海面排水與水阻（model 24 分支）

直接接主遊戲與 Fleet；`void-app --splashdown` 在當前天體真實海洋位置放置
防熱盾指令艙：COM 海面上 8 m、向下 2 m/s。fixture 透過一般 LaunchState /
Select journal action，沒有另一套水 runtime。HUD 顯示同一 evaluator 的 water N。
不能搭配 load/replay 或沒有海的天體。按 P 暫停，F6/F7 存讀沿用現有操作。

`Environment::SeaSample` 保留海平面 datum 深度，即使位於陸地；新增 water_present
及相對旋轉海水的速度。terrain height >= sea level 或 query point 在固體內不施水力。
繪圖和物理取同一 BodyDescription sea_level_meters。海面為旋轉天體上的球面；浮力
取局部径向重力減去径向離心項，不包含波浪／潮汐／非球形自由液面。

`void_modules::water::VesselWater` 從 PartGraph 複製 accepted geometry，trial 不修改
資源／模組。閉合外殼按零件 authored box / cylinder / cone 定義排水；box clipping
精確，曲面用 32 邊凸多面體，校正橫截面面積使全浸體積等於 authored 解析體積。
海面在零件尺度視為局部平面，裁切後以有向四面體積分取體積及浮心。
沒有開口、進水、流體 CFD；焊接零件外殼各自排水，未做穿插 hull 的集合布林體積。

rho = 1000 kg/m³；F_b = rho V g_eff up。水阻在浮心依實際點相對海水速度施加：
F_d = -rho V (1.2 / m) |v| v。正定角阻力用 displaced-mass bounding cuboid inertia，
乘 1/s 阻力率，作用於相對海水角速度，因此繞浮心原地旋轉也能耗散。
此簡化阻力是穩定下沉／濺落用的模型，不聲稱實測流體阻力係數或撞水結構破壞。
力及矩用 query axes，以 COM 為參考；旋轉海水的速度／spin只扣一次。

Scene Ground/Bubble 在開始、中點 torque、步末 push reconstruction 使用同一水源；
Orbit coupled trial source 也施水力，guided ideal pointing 用同一水 source。
非零水 load 禁止 rails，包括 sleeping boat；海上睡眠不能令 buoyancy 消失。
普通密度零件浮或沉由 V 與 PartGraph mass 決定，沒有浮力上限／強制漂浮。

驗證：closed hull 解析體積、半浸與傾斜對稱、角阻耗散與旋轉不變性、scene/inertial
frame wrench invariance、trial purity、海岸／地下 mask；真實 Fleet 防熱盾艙低速及
40 m/s 傾斜濺落、有限值與停穩、world checkpoint 續跑與 journal playback。
Agent GUI 尚待主 agent 排程 TigerVNC；人類驗收未代替。

Owner 接縫：ground_for 與 band_safe_seconds 以 min(terrain clearance, sea clearance)
判定 contact band，但只有 water_present 的海洋柱使用 sea；public clearance 仍是
地形 clearance。海面／深水使用 body-corotating Ground owner，不讓 Bubble 的
獨立 freefall reference 穿過天體。water force 列入 wake load，防止 sleeping body
跳過浮力。分級濺落測試同時保留浮動 pod 与密度>水的十引擎堆，20s 引擎 COM
在海面下約 19.65 m，仍是同一 Ground owner；不放寬 freefall Impact 斷言。

2026-10-07 分支驗證：基線 cc22385 加本分支工作區；隔離 target/water-core，-j2。
headless environment/module 全 crate 測試通過，water trial frame/energy/purity 通過；
fleet-flight water/aircraft/vehicles 受影響場景通過。四 core clippy -D warnings 通過。
未跑全 workspace，尚未主 agent GUI／人類驗收。

後續 review 修正：Orbit water evaluator 使用近海 trial envelope，取零件 reach、
相對海水速率及一腿最大 surface-g／thrust 接近距離；遠方乾燥 orbit 不因世界有海
就切入每步 coupled source。ground sea contact band 仍負責一般海洋的提前 owner 交接。
ForceOnly coupled/guided air 沿用原 no-spin force evaluator、air torque 為零；water
自己的 torque 不受 air mode 影響。no-ground Fleet 測試分別核對 dry-near-sea
ForceOnly 與無海既有旋轉一致、Full 有氣動阻尼，以及 airless submerged 的兩模式
water 角速度／線速度一致。這些測試及 near/far/incoming envelope 測試通過。

Scene 後續修正：water torque 中點和 water boundary push 更新不再依赖 AirDynamics。
ForceOnly 原 air force/start推力語義保留，仅从原 now 移除 start water 並加入 end
water；airless Ground 中傾斜濺落 10s 兩模式 position／velocity／spin 一致（零差）。
