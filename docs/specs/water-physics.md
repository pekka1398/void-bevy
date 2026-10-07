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

J2 與 stiff drag review：浮力重力改呼叫 void_orbit::gravity::pull／oblateness，以
query pole 投影 inward radial 再扣離心項；仍是球面海，沒有外部天體潮汐液面。

二次阻力剛性不能靠固定 60 Hz 顯式 kick 承受所有有效速度。Fleet 一個設定步
可細分為同一 runtime 的 accepted substeps；所有 Scene／Orbit 與耗用／thermal
一起推進到相同時間。以可能入水的 full hull 排水上界、實際相對點速度、正定
角阻及逆慣量 operator norm 算 relaxation rate，h <= 0.2/rate；沒有 force clamp。

ContactWorld::set_step_seconds 先讀舊 boundary velocity，消耗已入 boundary 的
solver_delta 一次，再用新 h 的 I+hC+h²C²（C 為 Coriolis operator）反解新的 native
half velocity；Rapier dt 和 options dt 一致。恢復配置步長也做同一 rebase，checkpoint
保存配置步及已轉換 cache，不把上次 substep 的 half velocity 當成 60 Hz。

有效 fully submerged fixture 同時以 100 m/s 橫向、100 m/s 向下（合速141 m/s）
入水：逐 60 Hz boundary kinetic speed² 遞減、有限，0.25s 小於100 (m/s)²；沒有
反向能量爆增。checkpoint 恢復後跨不同 substep 數量精確續跑一致。ContactWorld
重設 h 測試允許 native f32 一次重發布 rounding 3e-5 m/s，沒有物理 impulse。
J2 共用 gravity 數值測試、既有水5／飛機3／車輛2及 core clippy 通過。

濺落視窗 fixture 選點追加 daylight 條件：與 renderer 同樣的 home-system root star
→home body-fixed f64 frame tree geometry，候選點 sun cosine >0.2 且地形低於海面50m。
單天體配置沿用既有 renderer 明寫的遠方 +X 光源，不新加光源或亮度 fallback。
找不到 daylight 深海點明確 panic。核對真正 Aurelia／Sol 光源和單天體方向兩種
選點測試；使用相同 terrain sampler，未改海面或地形。
