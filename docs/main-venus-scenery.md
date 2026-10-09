# 主遊戲 Vesper：金星觀感

> 2026-10-09 整合更新：使用者已授權合入 master 並移除 Mars／Venus worktree。以下候選版本與驗證紀錄保留為歷史；現行整合版為 model30／world5，整合結果見 [status](status.md)。驗收程式須由主線重新編譯，不沿用分支 binary。


**穿雲黑層修正與全段核對已完成；root審查完成，等待人類驗收。先前4a1d927的ready曾因人類發現問題撤回，下方保留該歷史與本輪新結論。**

目前有效驗收binary SHA256：`5d9d84b6902ea2ffa19a09ea36313ff0a0b2ba0ab7a465dc5cb37f4e29547cbe`。

work/venus-scenery，基線74aec6b，未合併，候選實作與agent驗證完成；root審查完成，等待人類驗收。規格見[main-venus](specs/main-venus.md)。

入口：`./tools/venus-acceptance.sh orbit|near|far|plains|shield|upland`（擇一）。binary在本worktree的`target/acceptance/void-app-venus`。重新建置：`cargo build -p void-app -j 2`，再複製`target/debug/void-app`至上述驗收名稱。

全球以不透明連續厚雲和可見光淡色對比呈現。地表使用自訂程序火山平原、盾狀起伏和變形高地；不以橙色雷達假色冒充肉眼地表。物理大氣仍未實作，HUD明列optical air true / physical air false。地面雲下漫射光是擴散近似，並非完整標定光譜傳輸。

驗收需看全球／近景／地面、F2–F5、F6/F7與journal/checkpoint，再確認Aurelia不受影響。GUI證據與針對性驗證結果見下方。

## 初輪候選驗證（歷史：2026-10-08）

實作完成，root審查完成，等待人類驗收，未合併／push。候選binary SHA256：`15fd4fd89c81b439ed50438d7b57e795254819f8187f96cba927bdf33aeea196`。獨立target由第三方快取開始，全部workspace本地crate artifacts/fingerprints及incremental清除後從本worktree重建；沒有共享主target的本地產物。

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

## Root審查補修候選（歷史：2026-10-08）

此節取代上方初輪binary／final-*的「最後候選」地位。root已看初輪phase、ground與Aurelia，並獨立重驗初輪ground journal/save通過；root要求共用ContinuousDeck不暗藏Vesper外觀，以及補實際地貌觀察。補修已完成，root審查完成，沒有待修項，等待人類驗收，沒有merge/push。

該輪驗收binary SHA256：`6c9994f78fcd0dc3fd4d3b0d116c7435a3b6f78bbc156e7cc4fd1fa3a28e6475`。

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

最終狀態：root已核對補修接口／diff、正常濃霾GUI與sampler剖面，獨立核心測試及新journal/save通過，沒有待修項。root審查完成，等待人類驗收。

## 穿雲黑層修正與重新審查（2026-10-09）

人類從plains地面以滾輪連續拉遠，發現雲內黑層與出雲後突然變白；先前只核對地面／全球端點不足，4a1d927的待驗收狀態已撤回。本輪已補完整連續GUI與雲頂密集核對，root重新審查通過；下列實機證據不以CPU曲線或兩端畫面替代。

原因：原厚雲高階diffuse以直接向陽路徑光學深度做`exp(-tau*0.12)`，厚層內源項幾乎熄滅；雲下氣體卻使用另一個擴散傳輸，並在cloud_bottom硬切回未衰減光源。新實作用同一上覆垂直柱深度的diffusion場，讓雲下、雲內及雲頂氣體／雲源項連續。diffuse不再乘直接光透射而重複滅光；直接束仍沿原sun-ray optical depth衰減。

密度0–0.08雲厚的smoothstep底部漸入、0.80–1.0的顶部漸出不變。解析`deck_column_integral`精確配對此profile；水平密度噪聲錨定cloud datum底面，使垂直積分與實際密度同源。仍為48–70km、extinction=0.003/m，沒有為消黑層削薄厚雲。

CPU ground使用代表性平均柱深度（約42.14），GPU air/cloud使用當地水平紋理的柱深度；這是地面uniform未逐像素取雲噪聲的近似，非逐像素完整輻射傳輸。兩者使用同一diffuse transmission形式。地面與近地結果仍須用GUI驗證，不能只因解析積分連續就推論畫面無接縫。

本次沒有新增serialized字段，也沒有改物理／地形模擬規則，因此保持model29/world5；有deck的既有journal/checkpoint保持可讀，影像則取決於執行binary的光學版本。前節SHA6c9994...的影像屬舊光學結果；這輪候選debug／acceptance SHA為`5d9d84b6902ea2ffa19a09ea36313ff0a0b2ba0ab7a465dc5cb37f4e29547cbe`。

針對性CPU驗證scenery lib10通過（含密度積分／primitive導數配對、厚雲內diffuse連續、非有限高度拒絕及原配置驗證）；root亦獨立跑過此範圍。scenery/app lib/tests Clippy `-D warnings`、fmt及diff check通過。没有重新跑全workspace。

`cloud_camera_heights` example從journal每次Zoom後的真正frame-tree camera位置計算Vesper eye高度與focus distance，輸出兩者分欄；不得用focus distance當眼睛高度。VNC新連線應先將游標移入遊戲，再送wheel，並以journal確認事件；檔名wheel序號本身不能證明高度。早期baseline末段圖取點過高，以及游標落在titlebar的固定圖，不作穿雲修正證據。

技術參考曾檢視本機RSSVE的Venus atmosphere/cloud配置；其MinScatter和irradiance倍率只作處理體積內照明的方向參考，未複製參數、未換成2D球殼，未假稱取得不公開的新Scatterer shader。

同一新SHA對前次有deck的review-shield-v2 journal／review-shield-save再執行verify及verify-save，均通過T+1.016667s，確認本次純光學修正未破壞其模擬重播。新增cloud_camera_heights example亦通過專項Clippy `-D warnings`。

### 本輪有效連續GUI與雲頂密集補驗

root在TigerVNC :15從真plains／Home／曝光20固定方向連續滾輪升高；第二段`root-cloud-fixed2.jsonl`及save已穿過實際eye高度44.188、51.717、60.665、71.317、84.027km（高度由frame tree回算，非focus distance）。root親看雲底／雲內／雲頂後確認原黑層消失；两段新journal/save均verify通過。第一段`root-cloud-fixed`僅到18.118km，單獨不代表穿雲。

為分辨最後11km升高的亮度斜率，另以同一root第二段checkpoint生成60、62、64、66、68、70、72km，曝光6.31與20各一組受控camera checkpoints。`cloud_camera_fixture`只執行主遊戲既有Zoom/Exposure命令，固定方向、主camera不變，並核對eye目標誤差<1e-5m及排除presentation後整份world_mark相等，沒有推進或改動物理。此組是主GUI以F7載入的受控checkpoint證據，**不是手動滾輪錄影**；它補充root連續操作而非替代。

agent在:13載入同世界逐點觀察，已view_image核對兩曝光的60/66/72km與6.31的68km等代表畫面。`cloud-grid-6.31-{60..72}.png`與`cloud-grid-20.00-{60..72}.png`保留正常厚雲，從暖灰逐步變乳白，雲頂沒有全黑界面或從零突然亮起。6.31的變化比20易分辨；20在雲頂接近白色是固定高曝光結果。局部luma斜率在66–68km較大，70–72km趨於平臺，未聲稱為標定的金星輻射曲線。

[實際GUI顯示亮度曲線](images/venus-cloud-transition.png)以各張圖同一ROI（x850–999、y420–499）RGB中位數加權而來，單位是顯示RGB luma 0–255，不是HDR物理輻射量。6.31依序約121/138/156/178/212/228/228；20約199/210/220/231/243/248/248。原始`cloud-transition-display.dat`與`cloud-fixtures.dat`位於ignored evidence目錄。VNC初次capture偶有未更新的黑矩形；最終圖在同連線先capture再等0.5s取第二次capture，不把截圖傳輸缺塊當成雲物理。

受控點重產命令：

```sh
cargo run -p void-fleet-flight --example cloud_camera_fixture -j 2 -- lab-log/venus-evidence/root-cloud-fixed2-save.json lab-log/venus-evidence/cloud-fixtures
cargo run -p void-fleet-flight --example cloud_camera_heights -j 2 -- lab-log/venus-evidence/root-cloud-fixed2.jsonl
```

兩個新增example皆通過專項Clippy；沒有改GUI控制架構或另開物理runtime。root已親看曝光6.31的64/66/68/70km密集圖，並對照親自操作的44.2/51.7/60.7/71.3/84km真eye高度，確認黑層消失、雲頂漸亮；曝光20的近白是固定高曝光結果。root亦獨立驗證同連線第二次capture後黑矩形消失，確認該截圖缺塊不是shader。source／接口、scenery lib10及兩段新journal/save皆經root獨立核對通過，沒有待修項。**本輪root審查完成，等待人類重新驗收連續滾輪。**這是修正後的新結論，不是沿用舊ready。
