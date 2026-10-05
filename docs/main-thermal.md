# 主遊戲熱、傳熱、燒蝕與防熱盾

model 19、FleetCheckpoint 8。舊模型存檔／journal 明確拒絕；Craft 2 格式不變，
catalog 增加 thermal 模組及 Ablator typed resource。正常預設火箭的質量與幾何不變。

- 每個 flight 零件有表皮／核心溫度及不可逆失效狀態，存於 PartGraph 的穩定 thermal
  module ID；有限燒蝕質量僅存於 typed resources，不複製一份可變材料紀錄。
- 沿用 aero 的 Sutton–Graves／熱壁修正、對流、輻射、表皮／核心傳熱與有限 latent
  reserve。相連零件核心以對稱 conduction 交換；每筆交換能量正負相抵。
- 局部氣流按每個零件的 point velocity 與共享 Environment 取樣。埋住的堆疊端面
  共用氣動 body 的暴露面積；上游防熱盾僅遮蔽落在其投影盤內的下游零件。
- 只有接受的時間更新狀態與材料。Orbit 及 Rapier scene 都更新質量／慣量與 COM；
  材料以原本局部剛體速度離開，保留其餘零件的 pose／point velocity，不施加額外推力。
- 有热氣流使用固定物理步，Orbit／Bubble 的相同五秒條件已核對。進入大氣前
  rails 保守停止，避免以一個長時間窗的末端熱氣流計算整窗；安靜冷卻仍會在 rails
  更新。安靜長步使用相同熱方程的 adaptive exchange bounds，沒有凍結溫度。
- 熱失效不自動修復。失效的引擎／RCS 不提供作用力，失效對接埠拒絕捕獲，失效
  防熱盾不再遮蔽。失去唯一健康指令艙時清除控制、SAS／guidance；普通控制請求
  明確拒絕。沒有新增碎裂／爆炸；熱失效的固體仍留在船上。

## 主遊戲驗收

```sh
cargo run -p void-app -j 2 -- --reentry
```

預設是 RCS 指令艙＋heat-shield，盾帶 30 kg Ablator，110 km、7.5 km/s surface flow，
盾面向氣流，SAS on，暫停。P 續跑，兩次 Period 到 4x；HUD 顯示最熱零件、最高
core、失效數，以及盾的 skin/core 與材料。可拖曳相機看圓盤，F4 看實際 collider。
F6/F7 續跑，F8 錄製後以 --verify 核對。--reentry 不與 load/replay/rendezvous 同用；
再入場景由普通 journal actions 建立，不另開一套模擬。

heat-shield 在共用 assembly catalog，可由編輯器組入或 Craft JSON 指定。可在
resources.ablator 設定 0–30 kg；不得省略 inventory key。預設火箭仍沒有額外掛盾，
新增熱行為適用於正常發射。ThermalDefinition 的容量、面積、極限、傳導、輻射背景
等是明確 authored 參數，可針對零件配置。

## 模型範圍

初始 flight 材料參數是遊戲估計，未宣稱經真實火箭校準。輻射背景以明確的 270 K
recipe 表示；沒有日照／行星紅外的幾何積分、燒蝕厚度或化學模型。遮蔽是投影盤與
堆疊面近似，沒有 CFD／shock interaction。沒有因過熱改變碰撞形狀或移除零件。
再入 preset 用於熱與續跑验收，不含降落傘，也不承諾完整回收著陸。

本輪證據（master 168fcf2 後 model 19 工作區）：app library 25、新 thermal 核對 6
通過；先前所屬 core lib/tests 回歸亦通過（assembly/aero/modules/vessels/fleet-flight，
非 workspace 全量）。相關 lib/tests Clippy -D warnings、fmt 通過。
TigerVNC 實際再入、材料耗盡、F6/F7、錄製，561.533333 秒 journal 及直接世界存檔
headless verify 通過；證據存 `lab-log/thermal-evidence/`。載入後 renderer 呈現仍隨
下一項 scenery 整合再核對，不宣稱已完成人類驗收。
