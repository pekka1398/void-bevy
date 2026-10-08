# 主遊戲 Vesper：金星觀感

work/venus-scenery，基線74aec6b，未合併，候選實作與agent驗證完成。規格見[main-venus](specs/main-venus.md)。

入口：`./tools/venus-acceptance.sh orbit|near|far|plains`（擇一）。binary在本worktree的`target/acceptance/void-app-venus`。重新建置：`cargo build -p void-app -j 2`，再複製`target/debug/void-app`至上述驗收名稱。

全球以不透明連續厚雲和可見光淡色對比呈現。地表使用自訂程序火山平原、盾狀起伏和變形高地；不以橙色雷達假色冒充肉眼地表。物理大氣仍未實作，HUD明列optical air true / physical air false。地面雲下漫射光是擴散近似，並非完整標定光譜傳輸。

驗收需看全球／近景／地面、F2–F5、F6/F7與journal/checkpoint，再確認Aurelia不受影響。GUI證據與針對性驗證結果見下方。

## 候選驗證（2026-10-08）

實作完成，待root審查／人類驗收，未合併／push。候選binary SHA256：`15fd4fd89c81b439ed50438d7b57e795254819f8187f96cba927bdf33aeea196`。獨立target由第三方快取開始，全部workspace本地crate artifacts/fingerprints及incremental清除後從本worktree重建；沒有共享主target的本地產物。

model29／world5／FleetCheckpoint12／Craft3。新增CloudProfile必填形貌與RGB albedo，Volcanic terrain enum；舊model28／world4明確拒絕，不自動修補存檔。固定water-performance-world測試資料僅升schema並明寫舊EarthWeather/.99，保持該固定場景原來配置。

針對性驗證，未跑全workspace：

- terrain volcanic 2項通過：10000方向contract、cell濾波、非有限cell拒絕、配置roundtrip；最後公尺粗糙度調整後重跑通過。
- scenery lib6通過，連續雲要求full coverage且垂直光學厚度>40。
- Fleet solar_scenery6通過：所有固體renderer/collision同cell頂點1mm門檻、天體切換與checkpoint、Vesper無物理大氣、model28拒絕、LUT有限；最後terrain調整後重跑通過。
- app lib31通過，包括Vesper普通InitialWorld地面fixture、十天體renderer與restore asset ownership；後續僅光學shader與公尺terrain參數調整，以上述接縫和GUI補驗。
- 受影響四crate `--lib --tests` Clippy `-D warnings`、fmt、diff check與主遊戲build通過。

agent GUI使用TigerVNC :13＋Openbox、RTX5060 Laptop Vulkan。實際VNC操作near/orbit/far、拖曳至半月相位、地面P短跑、F4、F6/F7與錄製。已親自view_image核對ignored `lab-log/venus-evidence/`：

- final-aurelia.png：同binary保留Aurelia原地形／海洋／雲與光學；未發現shader error或panic。
- final-orbit.png、final-far.png、final-near.png、final-phase.png：自然可見光淡暖厚雲、相位與低對比細紋，全球無地表穿透。
- final-ground.png、final-ground-collider.png：普通Vesper Ground owner，P跑0.916667s後存讀，實際collider；灰色固有岩石受暖色天空照明。
- final-ground.jsonl及final-ground-save.json：最後binary verify／verify-save均通過，T+0.916667s。
- final-orbit-v3.jsonl及final-orbit-v3-save.json：最後binary verify／verify-save均通過，T+10.766667s（船仍在Aurelia，Vesper為觀測焦點）。

地面script用`--exposure 20`作相機曝光適應；直接`--vesper-site plains`仍沿用6.31，可自行Alt+F10/F11調整。厚雲下光線本就比雲頂暗，不以削薄雲層露出地形。

## 限制

光學是低成本RGB近似：連續厚雲加擴散高階散射尾部，雲下地面和氣體光源共同衰減；不是標定的金星全光譜傳輸或動力雲。單一48–70km deck沒有分層粒徑／硫酸化學；自然色吸收紋理是自訂低對比方向。程序地形是原創火山平原／寬盾／變形高地，不宣稱重建真實金星。

船的PBR仍使用既有通用燈，未隨雲下地面diffuse作光譜一致的重照明。沒有新增物理大氣，因此不宣稱金星阻力、壓力、溫度、熱或降落傘體驗已完成。近景從雲頂上方看不到大尺度地質是預期行為；地面fixture才展示雲下真實地形。人類最終驗收仍待進行。
