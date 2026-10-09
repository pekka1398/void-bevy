# 主遊戲 Ares：火星觀感候選

> 2026-10-09 整合更新：使用者已授權合入 master 並移除 Mars／Venus worktree。以下候選版本與驗證紀錄保留為歷史；現行整合版為 model30／world5，整合結果見 [status](status.md)。驗收程式須由主線重新編譯，不沿用分支 binary。


2026-10-09，`work/mars-scenery`，基線 `74aec6b`。分支候選已建立與驗證；root已親審diff／接口／GUI及獨立核對Ares四項、solar六項與同binary journal/save，沒有待修項。未合併、未 push、尚待人類驗收。範圍、研究及接口見 [spec](specs/mars-scenery.md)。

## 可執行入口

```sh
./tools/mars-acceptance.sh orbit
./tools/mars-acceptance.sh near
./tools/mars-acceptance.sh far
./tools/mars-acceptance.sh plains
./tools/mars-acceptance.sh canyon
./tools/mars-acceptance.sh volcano
./tools/mars-acceptance.sh canyon-overview
./tools/mars-acceptance.sh volcano-overview
```

前三項使用主遊戲 `--body ares --view near|orbit|far`。三個地面位置使用普通 InitialWorld，`--ares-site plains|canyon|volcano`，暫停開始；overview 額外使用 `--ares-overview`，以同一主相機執行可錄放的 Focus／Drag／Zoom，分別在650km／700km看地貌。Home回地面船，P放行，左鍵拖曳／滾輪、F1、O均沿用主遊戲。F2線框、F3 tile、F4真collider、F5地形，F6/F7存讀、F8結束錄製。腳本可附加`--record <journal>`、`--save <checkpoint>`及`--exposure <值>`。

驗收binary：本worktree `target/acceptance/void-app-mars`。

SHA256：`04bb68967c5c0a34a7ae94131aea5880e75483df36a5139945dc6795ce3a0ba4`。

```sh
cargo build -p void-app -j 2
mkdir -p target/acceptance
cp target/debug/void-app target/acceptance/void-app-mars
```

target獨立於主線；僅複製第三方cache，全部本地crate清理並由本worktree重建。沒有共用其他分支本地crate產物。

## 地表與主遊戲

程序自訂地理，沒有真實Mars地圖或全球照片貼圖。北方低地、南方古老高地、兩個大盆地、偏置火山隆起、三座盾狀火山、有限長分支峽谷與固定極冠形成不同省區。淡赭色塵、暗褐基底及白極冠區分材質，不使用地球海／雪線，不把增強色參考的藍色當地表自然色。

`AresTerrain` 在現有`void-terrain`內組合`ImpactTerrain`，直接構造獨立的完整ImpactOptions，不繼承Cinder美術preset。大地貌與公尺粗糙度都是f64真實高度，渲染和碰撞使用同一cell-limited sampler。8km正值基礎datum維持既有高度契約；Ares半徑3389500m、質量6.4171e23kg、自轉88642.44s及軌道不變。LOD密度未增加。

峽谷沉積會削弱實際舊坑起伏；shader用同一谷地mask削弱未被mesh解析的坑法線，保留原cell/pixel頻帶濾波。峽谷主壁和火山口壁有較清楚的過渡，但沒有縮小宏觀寬深、增加假macro法線或另一套物理地面。主遊戲樣本橫剖面谷底約5.8km、兩側高地約11.7km。`rise_direction`／`rise_width`／`canyon_direction`是配置，GPU資料從同一配置上傳；極點方向用穩定幾何basis。

三個fixture的日照經完整frames轉換驗證，初始sun·site為0.398／0.843／0.736。MartianRegolith加入現有光學大氣天空照明，Custom有效RGB係數近似薄塵霧；沒有新增大氣框架。全球高光下較淡赭色，低相位／迎光時大起伏對比仍較柔和；可使用overview及斜視核對真形狀。

## 驗證證據

分支model29，world4／FleetCheckpoint12／Craft3；model28明確拒絕，不自動遷移。root整合需統一版本並再次核對組合行為。

針對性檢查，沒有全workspace測試：

- terrain／scenery所屬測試在初版全部通過，Cinder及既有golden未改；最終地形修改再跑Ares四項，包括10萬方向／三格距範圍、連續性／序列化、盆地／火山／峽谷、±Z合法canyon。
- 最終Fleet `solar_scenery`六項通過：10天體view／checkpoint、同cell renderer/collider頂點1mm門檻、Ares recipe及舊model拒絕。
- 最終app日照／三地面InitialWorld測試與10天體renderer切換／restore資產穩定測試通過。
- 四個受影響crates的`--lib --tests` Clippy `-D warnings`、診斷example Clippy、fmt、主遊戲build通過。
- TigerVNC :14／RTX5060 Laptop／Vulkan，agent實看最後binary的全球、near/far、峽谷overview與地面、火山口與地面、極冠、三fixture、Aurelia/Cinder；最後GUI logs無panic或shader validation error。
- 同binary在canyon P短跑至T+1.166667s、F4、F6/F7、F8，journal與checkpoint皆核對通過；owner維持Ground。連續從近地向上zoom至全球，檢視跨越光學大氣高度的取樣畫面，未見黑介面／白球跳變。這不是完整相位／全部地點／所有顯卡的保證。

證據在ignored `lab-log/mars-evidence/`：`final-canyon.jsonl`、`final-canyon-save.json`、`final-canyon-overview.png`、`final-canyon-ground-{collider,restored}.png`、`final-zoom-{24,40,48,54,60,66,72,78}.png`、`final-volcano-{overview,caldera,ground}.png`、`final-polar.png`、`final-ares-{near,orbit,far}.png`、`final-plains-ground.png`、`final-aurelia.png`、`final-cinder.png`。

`ares_inspect`是讀取真checkpoint、以完整frames核對相機和地形橫剖面的headless診斷。其`--polar-checkpoint <path>`使用普通ViewCommand::BodyPreset輸出南極視角checkpoint，`final-polar.png`是主遊戲載入此檔的實際畫面，不是probe圖。普通主遊戲亦可拖曳觀察極區。

```sh
target/acceptance/void-app-mars --verify lab-log/mars-evidence/final-canyon.jsonl
target/acceptance/void-app-mars --verify-save lab-log/mars-evidence/final-canyon-save.json
cargo run -p void-fleet-flight --example ares_inspect -j 2 -- lab-log/mars-evidence/final-canyon-save.json
```

## 限制與人類驗收

尚無Mars物理大氣、動態塵暴／氣候、冰熱力學或岩塊實體。光學為RGB有效散射近似，非CO2 Rayleigh／礦物光譜校準；表面BRDF與微粒法線也未標定，沒有地形逐像素自投影陰影。極冠是固定季節的材質與低幅真地形。分支未包含另一分支的Vesper雲層修正。

人類驗收：先看orbit／far全球組織，再看canyon-overview及volcano-overview並縮放到地面，F2–F5對照；plains或canyon P短跑、存讀／起飛；同binary看Aurelia/Cinder。agent截圖與headless結果不代替人類對實際可玩行為和美術的驗收。
