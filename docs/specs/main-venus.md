# Vesper：主遊戲金星式景觀

基線74aec6b，work/venus-scenery；開發流程引用根AGENTS.md。僅Vesper，本輪不改其他天體外觀。

## 範圍

自訂程序地理，不重建真實金星地圖、不貼全球照片。全球自然可見光外觀以乳白淡暖的連續厚雲為主，雲從48至70 km，不為露出地形挖洞。近地面是暖色漫射照明下的灰色火山岩；固有色與天空光分開。

VolcanicOptions/Volcanic在既有void-terrain內：大面積平原、寬盾狀火山及小頂部凹陷、交錯變形高地、逐cell濾波的公尺起伏。LOD、collision、frames、world及主camera共用；正高度datum沿用既有contract。雲只屬光學，Vesper沒有新增CO2氣動力、熱或物理大氣。

## 接口

CloudProfile必填CloudMorphology（EarthWeather或ContinuousDeck）、single_scattering_albedo RGB。ContinuousDeck要求coverage=1，保證密度場無孔洞；EarthWeather保留原天氣／噪聲路徑。Aurelia仍使用原參數。

既有air ray march聯合積分air/cloud，不另造transport。連續雲使用低對比緯向紋理與程序細節，反照率偏暖但不複製UV高對比。GroundUniforms尾端continuous_cloud包含雲下漫射傳輸RGB和垂直光學厚度；地面直射以Beer-Lambert衰減，漫射以吸收性雲的擴散近似。這不是標定金星完整輻射傳輸，未模擬硫酸化學／光譜／雲動力。

## 參考用途

- NASA Venus facts https://science.nasa.gov/venus/venus-facts/ ：岩石地表、火山地貌、厚CO2大氣與硫酸雲。
- NASA Hubble clouds https://science.nasa.gov/resource/venus-cloud-tops-viewed-by-hubble/ ：永久覆蓋火山表面的硫酸雲。
- ESA ultraviolet https://www.esa.int/ESA_Multimedia/Images/2008/12/Venus_in_the_ultraviolet ：UV暗紋不是肉眼色彩。
- NASA cloud patterns https://science.nasa.gov/resource/venus-cloud-patterns/ ：紫色濾鏡、高通增強與人為著色，不直接作自然色。

已檢視使用者ref/celestial/venus全部9圖。image.png以及copy7左側作低對比乳白雲外觀方向；copy/copy2的橙色全球地表與copy5三維地貌呈現屬雷達式資料視覺化，僅取形貌。copy3/4/8的強雲紋與色彩用作增強／非自然色方向辨識，截圖本身缺原始波段資訊，不斷言其精確儀器來源。copy6是3D藝術示意。copy7右側比左側明顯增強。

## 驗收

主遊戲--body vesper --view near|orbit|far；tools/venus-acceptance.sh同名入口。--vesper-site plains是普通InitialWorld暫停地面fixture，P短跑，F4核對碰撞、F6/F7存讀，錄放沿用現有語義。不可混用其他fixture或world/load/replay/planet/terrain。

agent必須實際GUI操作並view_image查看截圖；仍待root審查與人類驗收，不以headless或截圖代替人類驗收。

## 格式與光學近似

分支model29／world5／FleetCheckpoint12／Craft3。舊版本明確拒絕；CloudProfile沒有默認補欄位。地形f64規則改變由model版本覆蓋。

連續雲高階散射使用半無限層反射率形式及衰減尾項補足原三階cumulus近似，雲下地面與air source使用同一有效擴散傳輸形式。此係數為視覺近似，非實測金星反照率擬合。雲的極弱暖色紋理不代表UV測量；遠端不顯露地形。驗證與二進位摘要見main-venus-scenery.md。

## Root審查补修

ContinuousDeck外觀由CloudDeckAppearance明示absorber_tint、latitude_frequency、band_contrast、warp、texture_scale，不從Vesper shader常量推斷；EarthWeather使用None並維持原分支。新增shield/upland普通地面fixture與同main camera中景，沒有另建觀察runtime。根審查確認保留正常霾，不為能看完整盾山而關閉大氣或提高不合理坡度；同sampler剖面補證與新含deck錄放證據見main-venus-scenery.md末節。人類驗收尚未完成。

## 2026-10-09 穿雲連續性修正

人類連續wheel升高暴露黑層／亮度跳變，先前驗收候選重新開啟。雲density taper與上覆柱深度須為同一場，diffuse與直接sun beam不能重複滅光；必須驗證地面→雲底→雲內→雲頂整段主camera行為，以frame-tree eye高度記錄，不能只靠端點截圖或CPU連續性。物理／序列化未變，simulation model29/world5維持；光學版本與binary hash另記。

本輪已完成root主遊戲連續wheel及agent雲頂60–72km每2km、兩曝光受控GUI補驗；root確認黑層消失、沒有待修項，恢復等待人類驗收。最新SHA與有效證據見main-venus-scenery.md開頭及末節。
