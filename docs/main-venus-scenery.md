# 主遊戲 Vesper：金星觀感

work/venus-scenery，基線74aec6b，未合併，候選實作與agent驗證完成。規格見[main-venus](specs/main-venus.md)。

入口：`./tools/venus-acceptance.sh orbit|near|far|plains|shield|upland`（擇一）。binary在本worktree的`target/acceptance/void-app-venus`。重新建置：`cargo build -p void-app -j 2`，再複製`target/debug/void-app`至上述驗收名稱。

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

## Root審查補修候選（2026-10-08）

此節取代上方初輪binary／final-*的「最後候選」地位。root已看初輪phase、ground與Aurelia，並獨立重驗初輪ground journal/save通過；root要求共用ContinuousDeck不暗藏Vesper外觀，以及補實際地貌觀察。補修已完成，仍待root最後審查與人類驗收，沒有merge/push。

最新驗收binary SHA256：`6c9994f78fcd0dc3fd4d3b0d116c7435a3b6f78bbc156e7cc4fd1fa3a28e6475`。

- `CloudDeckAppearance`顯式配置吸收tint、緯向頻率、帶紋對比、warp與三軸texture scale；Vesper專屬值放在profile，shader只讀配置。ContinuousDeck必須有deck配置，EarthWeather不得帶deck；Earth保留原路徑。
- datum核對：cloud_height相對光學bottom_radius，雲高度相對cloud datum。density／segment／subcloud均先減一次sea_level再比較cloud_bottom。原`height < bottom+sea`數學等價，補修改成同一形式避免誤讀。
- 新增`./tools/venus-acceptance.sh shield`及`upland`，是普通InitialWorld固定真地貌位置，暫停啟動，同主camera以journalled Zoom/Drag拉到約6km距離。plains入口保留。shader、光學濃度與地形形狀均不因fixture改變。

補修驗證：新增terrain landmark測試連同volcanic contract共3項、scenery lib7項、app精確篩選`vesper_ground_fixture`一項（包含plains/shield/upland）通過；terrain/scenery/app lib/tests及新增profile example Clippy `-D warnings`、fmt、diff check、主遊戲build通過。沒有重跑全workspace或初輪31項app全集。

最新GUI證據在`lab-log/venus-evidence/`：review-shield.png、review-shield-high.png、review-upland.png，皆使用正常濃霾，已view_image檢查。高相機俯視的shield-high幾乎全被霾遮蔽；實際中景只讀得到局部缓坡／細起伏，不能靠此圖宣稱完整盾山輪廓肉眼清晰。沒有為截圖關霾或造陡崖。

最新`review-shield-v2.jsonl`及`review-shield-save.json`含deck配置，Vesper真Ground owner短跑1.016667s、F6/F7後由上述新binary獨立verify／verify-save通過。舊final-ground／final-orbit-v3檔案只屬2236111初輪證據，缺deck，不當作補修版本可載入的資料；未替舊檔自動補欄位。分支仍model29/world5，尚未交付合入；兩個候選之間的必填ContinuousDeck配置變化在本節明記。

真幾何補證見[盾山剖面](images/venus-shield-profile.png)：沿shield fixture東西向±100km，取同一Volcanic f64 sampler，顯示寬緩盾山及中央淺凹陷。全細節與100m cell濾波線對照，並非雷達影像或可見光渲染。縱橫軸不同尺度；圖上的高度差不代表真實坡角。重產數值：

```sh
cargo run -p void-terrain --example volcanic_profile -j 2 -- 0.44514744410845786 -0.8809818721283112 0.16035801815002318
```

輸出三欄為向東距離m、全細節高度m、100m-cell高度m（皆above reference sphere）。剖面高度約3.18–6.10km、fixture約5.93km；雲層在48–70km，上方正常稠密氣體與雲仍會遮住遠距地貌。剖面僅補充真幾何證據，不代替主遊戲或人類驗收。

root對補修工作區另行獨立重驗volcanic3及scenery lib7通過；此證據只覆蓋該兩個針對性範圍，不代表全量驗證或人類驗收。

root其後已看補修review-upland／shield-high，接受文件如實描述濃霾可見限制；並以同SHA `6c9994f78fcd0dc3fd4d3b0d116c7435a3b6f78bbc156e7cc4fd1fa3a28e6475`獨立verify最新review-shield-v2 journal及review-shield-save，兩者皆通過T+1.016667s。這是root驗證，不是人類驗收。
