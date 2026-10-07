# 主遊戲 Solar scenery

2026-10-05：採入 `work/solar-scenery` 的第一輪核心配置、取樣器與外觀，原 worktree 保留。
主 agent 在 master 將 renderer 接入既有 FlightSession；不另建物理世界或另一套遊戲。

## 範圍

主遊戲預設 Aurelia 世界含十個 authored scenery：Sol、Cinder、Vesper、Aurelia、Ares、
Velvet、Halo、Azure、Abyss、Selene。Aurelia 保留現有地形、海、大氣、雲；四個其他固體
天體共用 cratered 取樣器，近景用既有 TileField／LOD，碰撞使用同一配置。氣態巨星
使用程序雲帶表面，Halo 有環，Sol 使用發光表面與透明日冕。其他衛星仍是 map 球。

這是第一輪可辨識外觀，不是十顆都已達到地球的細緻程度。氣態巨星沒有可著陸固體、
LOD 地面或碰撞地形；沒有氣態巨星深入／恆星毀損模型。星環、日冕是視覺物件。

Aurelia、Vesper、Ares 有分別配置的光學大氣；只有 Aurelia 有物理大氣。其他天體的
光學外觀不會自動產生阻力或加熱。每個光學 volume 使用自己的座標與 LUT，先做
HDR transport，最後一次曝光／tone mapping。尚無食、雲影／環影、多恆星照明、天氣，
也未加入完整 scenery 調參面板。

## 操作

```sh
./target/acceptance/void-app --body selene --view orbit
./target/acceptance/void-app --body halo --view orbit
./target/acceptance/void-app --body sol --view far
```

一般啟動就是預設主遊戲。Shift+Tab 或 map 標籤選天體；F1 循環 near／orbit／far，
Home 回到船。O 在目前觀測天體的 400 km 軌道生成並選取船：這是驗收 fixture，不是
自動完成轉移。Alt+F10／F11 調曝光，不會同時切換對接埠。機動起始時間向前調整改為
Alt+Home；End 不變。1–4／G／J 繪圖框架仍可使用。

`--world <InitialWorld JSON>` 使用明確配置，不能混入 planet／terrain／craft／vacuum
覆寫，或覆蓋 checkpoint／replay。`--body` 與 `--view` 是初始視角操作，會錄入 journal；
不能覆蓋 replay。`--exposure` 為正數且不超過 100。`--vacuum` 明確停用光學與物理大氣。

F2／F3／F4／F5 用於比較地形與實際 collider。F6 存檔、F7 載入後暫停、F8 結束錄製。
切換天體會釋放上一近景的 tile／task；相同世界的 checkpoint restore 保留世界 GPU
資產，實際世界配置改變才重建並移除舊 image／material／mesh。

## 格式及核對

整合 model 20、world schema 3、FleetCheckpoint 8、Craft 2。新增必要 surface recipe 與
presentation exposure；舊模型明確拒絕，沒有自動遷移。熱系統的 model 19 錄影仍可用
當時保留的 `target/acceptance/void-app-thermal` 核對，但不能用 model 20 載入。

針對性 core 檢查包含十天體配置 roundtrip、鏡頭操作不改物理與存讀／journal一致、
cratered collider 與渲染取樣一致、authored LUT 有限、非法 HDR 值拒絕。app 測試走
實際主 renderer，切十個天體並連續還原相同世界，檢查船仍存在、image／ground
material 數量不增加。這些 headless 檢查不代替 shader／GUI 或人類最終驗收。

## 人類驗收

1. 預設主遊戲確認火箭、地球近景、雲／海、F2–F5；F6→F7 後畫面與船仍在。
2. `--body selene --view orbit`，按 O：船在月球軌道、HUD 參考 Selene；Home 追船。
3. Halo 看環、Velvet 看雲帶、Sol 看發光；F1／曝光／鏡頭切換後無 panic。
4. `--reentry` 按 P，觀察熱讀數與材料減少；存讀後續跑。四種 plot frame 切換時
   路徑與標籤更新，不能改變船的物理狀態。

使用者已於 2026-10-07 確認最終驗收完成（master `f06c283`）；本輪未自動跑全 workspace，未 push。

## 本輪驗證紀錄

master `a625be9` 加本次 scenery 工作區（本頁隨實作提交），2026-10-05：

- `cargo test -p void-app --lib -j 2`：26 passed；包含真正主 renderer 的十天體切換、
  GPU 相機必要設定、相同世界三次還原資產穩定、熱／對接／錄放。
- `cargo test -p void-fleet-flight --tests -j 2`：77 passed；含完整氣動／RCS／對接、
  direct checkpoint、durable journal、guidance、presentation、多天體、warp。
- terrain cratered、scenery solar、Fleet solar_scenery 三個 target：9 passed。
- app／fleet-flight／scenery／terrain 的 `--lib --tests` Clippy `-D warnings`、fmt 通過。
  未跑 workspace；既有 opt-in GPU offscreen 測試本輪未另跑。
- root TigerVNC／RTX 5060 Laptop／Vulkan：Selene、Halo、Sol、Vesper、Aurelia；
  月球 O fixture、四種 plot frame、曝光、F6／F7、再入熱／耗材。五份真實 journal
  headless verify 通過，月球與再入 checkpoint verify 通過。
- 再入新 renderer 實測 T+91.383333 s：shield skin 850 K、core 約 272 K、ablator
  2.321 kg、總質量約 112.3 kg；存讀後仍顯示，journal／checkpoint 狀態核對一致。
- 初次 GUI 發現相機漏掉 HDR／depth binding／MSAA 配置，已恢復並加回歸斷言；
  修正後未見 shader validation error 或 NaN panic。durable crash 測試更新了實際
  `invalid vessel control` 錯誤文字的斷言，沒有更改拒絕條件。

可直接執行的 model 20 snapshot：`target/acceptance/void-app`。本機 ignored 證據在
`lab-log/solar-evidence/`；舊 model 19 熱證據保留 `lab-log/thermal-evidence/`。
以上是 agent 初步核對的證據；使用者另於 2026-10-07 確認人類驗收完成，沒有宣稱其他 GPU 或長期記憶體檢查通過。
