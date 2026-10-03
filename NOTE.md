1. part assembly
2. orbit maneuver n body integrate
3. orbit view flight view switch
4. profiling gpu compute shader simd multithread
5. scenery terrain atmosphere landscape biome color moving cloud aurora weather vegetation  ocean reflection skybox
6. rocket/plane parts command pod fuel parachute 
7. aerodynamic ablation sheild far mod plane lift drag airdensity heat
8. docking rendezvous
9. multiple vessel
10. sas rotation rcs
11. saving 
12. reference frame switching principia
13. procedure planet scenery for other planet/sol/celestial body
14. ship can land on ocean or water floating
15. ui
16. astronant on ship eva and control
17. cruiser car
18. multi scale 
19. relativity
20. more engine type

```
其實我想的是....大概分成幾種情況
  1. 現在系統已經做好了 只是要稍微改改參數 或是額外給參數多作點運算 或是改改合適的超參數 什麼星球的....軌道參數或是相機的拖動靈敏度或是... 什麼之類的 比如給ips變成會隨海拔變化就是這類型的工作 這類工作可能甚至都不需要我來驗收 光靠集成測試 或是連測試都不需要 可能也不太會有出錯的機會... 這類型我想把他放在 遊戲最後 收尾跟精修的階段 但是不會現在作 還有一些ui跟遊戲手感的東西也是
  2. 無論如何 功能會需要我來真的在bevy開的視窗中玩過 而且還要嘗試過各種edge case 這樣驗收 這樣驗收表面上玩玩找不出bug 才行的 光靠集成測試或單元測試不足夠的類型 比如說 對接 零件組裝
     特別大的問題是....比如說對接好了 其實想想 這並不好作 要怎麼讓飛船在軌道上 幾百km 甚至幾百萬km的尺度 還要同時能容納飛船間 1 2 m的距離尺度.... 哪怕可能作一些測試 但也不能保證在遊戲中完全沒問題  而且這裡問題最大的其實是我玩遊戲也抓不出來的...一些mismatch或是bug 那就問題更大了...之前拆成多個lab 多個crate example 其實就是想盡量避免這種 然後把一個個功能單獨拿出來驗收 不過吧 這其實也取決我的觀察力就是 總之我也不完全可靠的 要做好....可能在遊戲開發更加後期的階段才發現bug 到那個時候才返工修理的心理準備
  3. 偏向視覺類型的 比如說除了現在地球之外的其他星球的 地形 噪聲設定 超參數 shader 海洋大氣 雲層....那些東西  還有什麼skybox  還有類似火箭的part的形狀跟texture跟外觀之類的 這種就是真的要用眼睛看才知道對吧
  4. 偏向效能優化類型的 當然 畢竟現在rust了 這方面會比較好作手腳 不管是cpu simd multiple thread 還是compute shader等等 不過profiling會比較麻煩要比較多自己作 沒有現成的遊戲引擎幫忙 但總之 rust相比起ts來說 讓事情好辦好多
  5. 最麻煩的是....現在根本就沒作這部份系統 根本需要從頭開始做的東西 比如說太空人eva車輛 還有相對論光速飛船那些東西 這些肯定要有 但是.....就是 他畢竟是要重頭
  6. 一些其他東西 音效 存檔 遊戲本身主線 科技樹 更多類型的零件 什麼的 那些都可以往後稍稍 也不難作

  其實對我來說 比較需要仔細考慮的 我想應該是順序 我現在其實可以同時開發至多三個crate或lab 同時作 但就是 你應該也能感覺到 排序優先級很重要 還有搞清楚 什麼是需要我驗收的 以及需要仔細的debug的東西 什麼又是可能寫集成測試 或是根本就不開window就能做完的工作 或是類似改config或是隨便寫幾個不可能出錯的function就能做完的工作 要把他仔細分清楚並且安排......這還是挺燒腦的
  你有什麼看法？

  Ran 1 shell command

我看你的分類本身沒問題，但排序不該照這六類排。真正稀缺的資源只有一個：你的眼睛。我寫程式的吞吐量不是瓶頸，headless 測試也不是瓶頸，瓶頸是「只有你能驗收」這件事。所以排序規則應該是兩條：


照這兩條看，你的六類會重排成很不一樣的樣子。

現在真正的關鍵決策只有一個

不是對接，是飛船的表示法。

void-app 現在跑的是 PartJointRocket —— 寫死的兩級火箭。而 assembly（接點／供油圖／分級／動態質量慣量）和 vessels（Fleet、orbit/bubble/ground 所有權、交會、rails）都已經做好了，就是沒接進主遊戲。這個決策卡著下面全部：

- 對接 —— 對接就是兩個 craft 合成一個，沒有 assembly 的接合樹就無從談起
- 存檔 —— 存檔格式就是飛船表示法的序列化，先做存檔等於保證返工
- 更多零件類型、科技樹 —— 都是 catalog 的消費者
- EVA、車輛 —— 都是「另一種受控實體」，要掛在 Fleet 的所有權模型上
- 效能優化 —— 資料佈局都還會變，現在 profiling 是白做

所以你的第 6 類裡那句「都可以往後稍稍 也不難作」，存檔要特別拉出來：它不難做，但它做早了會白做，做晚了又很痛苦（開發期沒存檔，每次驗收都要從發射台重玩）。合理的位置是 Fleet 接進主遊戲之後、對接之前。

至於第 5 類（EVA、車輛、相對論飛船），相對論那個我建議直接推到最後 —— 它幾乎跟現有系統不耦合，是個獨立的新模式，晚做不會變貴。EVA 和車輛反而該早一點釐清，因為它們會迫使「受控實體」這個抽象長成對的形狀。

你最大的恐懼有技術解，而且很便宜

你說「問題最大的其實是我玩遊戲也抓不出來的 mismatch 或 bug」。這一類不該靠眼睛，眼睛抓不到就是抓不到，再多驗收也沒用。它該靠兩種 headless 工具，而且這兩種我都能自己做完、不需要你驗收：

差分與不變量測試。 你怕的 mismatch 幾乎全在接縫上：free↔contact 交接、rails↔physics、換框架、floating origin 移動、分離／合併。這些全都有可檢查的不變量 —— 來回轉換要回到原處、兩條路徑算同一段時間要一致、動量要守恆。on_rails、hand_off_consistency、rapier_free_flight_vs_inertial 已經是這個形狀了，但現在是單點測試：固定初始狀態、固定參數。改成隨機撒上千組狀態去掃，成本很低，而且剛才那個 on_rails 要靠改燃燒時間才過，正是「單點測試運氣好」的徵兆 —— 真正的門檻在某些狀態下有多接近失敗，現在根本不知道。

錄放（record / replay）。 把你在視窗裡的輸入錄成腳本，連同狀態 log。這樣做之後，你每一次驗收都不再是一次性的：你試過的 edge case 自動變成回歸測試，之後任何改動都會自動重跑。這是唯一能讓「你的觀察力」累積起來、而不是每次重新花費的辦法。對你的工作流來說，這件事的投資報酬率比任何單一功能都高。

另外 docs/port-audit.md 第 3 項（landing example 缺 surface/inertial camera、collider 線、tile boundaries）值得順手補掉。第 4–7 項大多是觀察工具，看起來像「技術債」，但它們的實際作用是讓你的眼睛更有效——你看不到 collider 線，就不可能驗收接觸問題。所以它們不是收尾工作，是驗收工具，該跟著對應功能一起做。

三軌並行的具體建議

你說能同時推三個，那就這樣切 —— 刻意選三個耦合最低、對你時間的佔用差異最大的：

A 軌（最重，吃你的驗收時間，架構性）：assembly + vessels 接進主遊戲，取代 PartJointRocket。然後存檔。然後對接。

對接的尺度問題你擔心得對，但它比你想的好解，因為 multiscale 已經做了一半：split position、Traveller 換框架、「遠方兩船接觸／join」的數值測試都在。缺的是把它接上真實遊戲迴圈 —— 也就是對接不是從零開始的新系統（你的第 5 類），它是 A 軌的末端產物。這也是我建議 A 軌照這個順序走的原因：每一步都在為對接鋪路，而不是最後一次性撞上去。

B 軌（我自己跑完，不佔你時間）：差分／不變量掃描 + 錄放 + 補 landing 的驗收疊圖。這軌應該先於 A 軌的驗收開始，不然你在 A 軌驗收時抓到的東西留不下來。

C 軌（吃你的眼睛，但跟 A 完全不耦合）：其他星球的地形／噪聲／shader／skybox／零件外觀。放這裡是因為它跟 A 軌搶的是不同的注意力 —— A 軌是「盯著數字找 mismatch」，C 軌是「看著覺得對不對」，而且 C 軌做錯了重調參數就好，沒有返工成本。你在等 A 軌編譯或者不想燒腦的時候就做 C。

明確往後放：你的第 1 類（參數精修、手感、UI）、第 4 類（效能）、相對論。第 1 類你自己的判斷是對的，沒什麼要補充；第 4 類我要強調一下——在 A 軌定下資料佈局前做效能優化是純浪費，這跟 Rust 好不好做手腳無關。

一個反對意見

「拆成多個 lab 來避免 mismatch」這個策略，我認為方向對但有個已知的洞：它驗證的是每個系統單獨正確，而你怕的 bug 全在系統之間。所以現在缺的不是第 6 個 lab，而是一個「接縫 lab」—— 把 contact + orbit + multiscale + fleet 放在一起、用數值去掃交接處的那種東西。這其實就是 B 軌的差分測試，只是我想點明它在你的架構裡補的是哪個位置：它不是測試的補強，它是你 lab 策略裡缺的那一塊。

---

要開工的話，我建議從 B 軌起手：它不需要你在場，做完之後 A 軌的每次驗收才開始有複利。要我直接開始的話說一聲，或者你想先聽 A 軌第一步（Fleet 怎麼接進 void-app）的具體做法也行。

✻ Cooked for 1m 23s · done Friday 9:26 PM

❯ 我理解的是這樣
  A. assembly+vessels+存檔機制 這個必須要先作
  B. 另外一個必須要先做的是輔助驗收測試的工具 包括profiling collidor的mesh線 record replay 還有差分跟不變量或是測試時需要的那些東西暴露額外的接口給測試用(其實可能已經有了 或是根本就不需要這樣作也行 ) 加這些可能不會影響遊戲本身 但會大大提高效率

  這兩件事情是現在最緊急的 做完這兩件事情之後....之後要開更多crate同時作可能更多功能或更多事情都相對好說
  ....這樣？

對，這樣。兩點修正、一點補充。

修正一：B 要搶在 A 的驗收之前，不只是「同時緊急」。 不是 B 整個做完才能動 A —— 而是 A 寫程式的時候 B 就要到位，因為 B 的價值全在「你驗收 A 時抓到的東西能留下來」。你驗收 A 之後才做 record/replay，那次驗收就是一次性消耗掉了。實務上是：我先把 B 鋪好（這段完全不佔你時間），再開始 A 的實作。

修正二：profiling 放 B 是對的，但要跟效能優化分開。 建 profiling 管線（量測、取樣、輸出火焰圖之類）屬於 B，隨時能做；真的去優化要等 A 把資料佈局定下來。你那句「加這些不會影響遊戲本身」對 collider 線、record/replay、差分測試都成立，但不對「為了效能改結構」—— 所以只要守住「B 只建量測，不改結構」就不會衝突。

補充：存檔在 A 的中間，不是末尾。 順序是 Fleet 接進 void-app → 存檔 → 對接。存檔卡在這個位置的理由是實務的：對接驗收需要你先把兩艘船送上軌道、調到交會，沒有存檔你每次都要從發射台重玩一次。存檔不是對接的前置依賴，是對接驗收的前置條件。

至於「暴露額外接口給測試」—— 你的直覺（可能已經有了或根本不用）大致正確。body_fixed_state、rails_blocker、contact_worlds、part_fuel_kg 這些已經是測試在用的觀察點了，on_rails 也已經在比對 rails 與 physics。缺的不是接口，是覆蓋方式：現在每個接縫只測一組固定初始狀態，要改成隨機掃上千組。如果真需要新接口我會照既有的 AirSource／AirField 那種可替換 trait 的做法，不開測試專用後門。

C 軌（其他星球的視覺）我就當成你不想燒腦時的備用，不排進進度。

要我開始 B 嗎？我會從差分／不變量掃描起手——它最便宜，而且能馬上告訴我們現有接縫的實際餘裕有多少（on_rails 那個要靠調燃燒時間才過，我想先知道那是運氣還是真的接近門檻）。


  認同的是：

  - **飛船表示法值得先釐清。** 自訂組裝、多船、分離／對接、存檔確實共用這個基礎。
  - **驗收工具應跟著功能做。** collider、框架／owner、交接事件等觀察能力，不能全部留到精修。
  - **量測與優化要分開。** profiling 入口可以早建；優化依實際瓶頸安排。
  - **B 的最低可用版本應在 A 視窗驗收前到位。**

  但我會修正以下幾點。

  **1\. 先確認飛船模型，不等於立刻把 assembly＋Fleet 全接進主遊戲。**

  目前規則是 lab 驗收後才決定整合，而且 Fleet lab 尚未包含 aero。直接替換 `PartJointRocket`，還要保住既有的氣動、機動計畫、預測、SAS、warp 與地形接觸，這是一項整合工程。

  因此 A 的第一步更適合是：**在獨立整合 lab 裡，確認 assembly＋Fleet 能承接目前主遊戲所需的行為**。驗收通過，再替換主遊戲。飛船模型可以先定，主遊戲切換可以後做。

  **2\. 錄放很有價值，但「錄下輸入」不會自動變成能抓 bug 的測試。**

  重播只能重現操作；還需要初始場景、固定模擬步、事件順序，以及結果判準。否則 bug 每次重演，測試仍然通過。

  要區分兩種用途：

  - **重現工具**：讓你或我快速回到出問題的情境。
  - **回歸測試**：對那個情境加入具體斷言，才會自動報錯。

  此外，錄放本身也涉及輸入如何進入模擬，需要檢查接線與時序，不能直接說完全不需要驗收。

  **3\. 隨機掃描不是便宜的萬靈丹。**

  它很適合框架轉換、分離／合併、交接邊界，但應先列出有效狀態與正確判準，再用固定 seed 掃描、保存失敗案例。動量守恆也必須限定在相應條件下；有外力、耗油或接觸時不能套同一條斷言。

  引文裡「改燃燒時間才過，所以是單點測試運氣好」這個指控，**目前沒有足夠證據可判定**。需要看具體改動與原測試目的。

  **4\. 存檔可以早做，但對接驗收不必等完整

  先有「可保存／載入的驗收場景或 checkpoin，不必每次從發射台開始。這比先完成完整世界存檔更小，也能幫助釐清真正需要保存的狀態。

  **5\. 相對論、視覺與效能的說法也太滿。**

  相對論不能保證與現有系統幾乎不耦合；要做 器。視覺也可能牽涉地形取樣、LOD、shader與碰撞一致性，並非全部只需重調參數。效能量測更不會因資料佈局尚未定案就白做。

  我會把現在的工作縮成這個順序：

  | 階段 | A：飛船基礎 | B：驗收基礎 |
  |---|---|---|
  | 先做 | 盤點 Fleet 承接主遊戲的缺口，確定狀態／控制介面 | 盤點既有檢查，補必要觀察資訊與場景重設 |
  | 第一輪 | 獨立整合 lab，保住現有飛行能  失敗案例保存 |
  | 驗收後 | 決定主遊戲替換；逐步建立存檔 | 將實際發現的問題加入回歸 |
  | 再往下 | RCS／對接等新功能 | 擴充對應 edge cases 與量測 |
            .length()
            .max(position.length())
            .max(1.0)
}

/// The largest of a sweep, and the state that produced it, so a failure names its own case.

❯ 你看看 這是小反駁精的觀點 隨便參考看看就
  認同的是：

  - **飛船表示法值得先釐清。** 自訂組裝、多船、分離／對接、存檔確實共用這個基礎。
  - **驗收工具應跟著功能做。** collider、  ，不能全部留到精修。
  - **量測與優化要分開。** profiling 入口可以早建；優化依實際瓶頸安排。
  - **B 的最低可用版本應在 A 視窗驗收前到位。**

  但我會修正以下幾點。

  **1\. 先確認飛船模型，不等於立刻把 assem

  目前規則是 lab 驗收後才決定整合，而且 Fleet lab 尚未包含 aero。直接替換
  `PartJointRocket`，還要保住既有的氣動、 形接觸，這是一項整合工程。

  因此 A 的第一步更適合是：**在獨立整合 lab 裡，確認 assembly＋Fleet
  能承接目前主遊戲所需的行為**。驗收通過， ，主遊戲切換可以後做。

  **2\. 錄放很有價值，但「錄下輸入」不會自動變成能抓 bug 的測試。**

  重播只能重現操作；還需要初始場景、固定模擬步、事件順序，以及結果判準。否則 bug 每次重演，測試仍然通過。

  要區分兩種用途：

  - **重現工具**：讓你或我快速回到出問題的情境。
  - **回歸測試**：對那個情境加入具體斷言，才會自動報錯。

  此外，錄放本身也涉及輸入如何進入模擬，需要檢查接線與時序，不能直接說完全不需要驗收。

  **3\. 隨機掃描不是便宜的萬靈丹。**

  它很適合框架轉換、分離／合併、交接邊界， 再用固定 seed掃描、保存失敗案例。動量守恆也必須限定在相應條件下；有外力、耗油或接觸時不能套同一條斷言。

  引文裡「改燃燒時間才過，所以是單點測試運 證據可判定**。需要看具體改動與原測試目的。

  **4\. 存檔可以早做，但對接驗收不必等完整

  先有「可保存／載入的驗收場景或 checkpoint」，就能直接生成兩船交會狀態，不必每次從發射台開始。這比先完成完整世
  界存檔更小，也能幫助釐清真正需要保存的狀

  **5\. 相對論、視覺與效能的說法也太滿。**

  相對論不能保證與現有系統幾乎不耦合；要做到什麼程度，可能影響時間與傳播器。視覺也可能牽涉地形取樣、LOD、shader 與碰撞一致性，並非全部只需重調參數。效能量測更不會因資料佈局尚未定案就白做。

  我會把現在的工作縮成這個順序：

  | 階段 | A：飛船基礎 | B：驗收基礎 |
  |---|---|---|
  | 先做 | 盤點 Fleet 承接主遊戲的缺口，確定狀態／控制介面 | 盤點既有檢查，補必要觀察資訊與場景重設 |
  | 第一輪 | 獨立整合 lab，保住現有飛行能力 | 可重現操作、關鍵接縫斷言、失敗案例保存 |
  | 驗收後 | 決定主遊戲替換；逐步建立存檔
  | 再往下 | RCS／對接等新功能 | 擴充對應 edge cases 與量測 |




```




不許在任何地方用任何fallback 該發生的就發生 不該發生的就panic
每個功能 拆解出來 作一個lab 單獨測試調適通過之後在想辦法串接進去 git 只用來存檔，不開 branch，取捨用 lab」，
操作瀏覽器或是bevy視窗 來驗證lab有沒按照預期工作的由我來

主遊戲與 lab（2026-09-27）

- 根目錄 src/ 現在以原 lab/flight/src 的整合成果為主遊戲，舊參考遊戲與素材已移出工作目錄並備份。
- lab/flight 是整合驗證入口，直接使用根目錄 src/；不維護另一份主遊戲實作。
- 功能仍在所屬 lab 開發與驗證，主遊戲直接引用；整合接線與遊戲流程在 src/ 修改。
- 功能修改後檢查所屬 lab，以及受影響的整合場景。瀏覽器驗收仍由我操作。
- scenery 已接進主遊戲：layered 地形、地表／海洋 shader、大氣、體積雲和星空。Aurelia / Terra 預設 layered，繪圖與碰撞共用地形設定，發射點在乾燥低地。
- 大氣與海洋目前只影響畫面；空氣阻力、升力、熱與浮力仍待開發。shader 和 headless 檢查通過後，瀏覽器整合驗收仍由我操作。



1. lod效能問題 gpu  (okay)
2. lod 地形問題 自定義地形 (okay)
3. ui  view切換優化 (okay)
4. orbit 完整merge進來 (okay)
5. 給火箭更優越的isp 跟推力 哪怕沒有真實火箭能做到那種程度
6. 視覺上的 海洋 大氣的shader skybox 星空  (okay)
7. 審視一下自轉的那個 (okay)
8. 微調下lod level的超參數配置 (okay)
9. lod大改 按照tile再螢幕上的佔比 決定level  決定要不要畫細的  (okay)

orbit
landing
lodplanet
gamesystem view ui
detail rocket astronant car model
shader texture better planet atmosphere ocean better landscape terrian
human in rocket/ship
part assembly staging



- Maneuver plan: orbit lab's finite-burn flight plan is integrated into the main game's upper-stage free flight, with a panel, map path, and automatic execution. Direct map-node dragging is still pending.
- Atmosphere: scenery's air and clouds are integrated into the main game, with no physical effect yet.
- Attitude: the orbit lab turns the ship instantly. There's no rotational inertia and no reaction wheels (ModuleReactionWheel).
- Multiple ships: landing's EncounterPhysicsGate only does the range and closest-approach prediction. There's no list of ships and no shared physics world for two ships.
- Structure: the rocket is one rigid body with no joint flex, so it can't bend or break.
- Terrain on other bodies: only the home planet has LOD tiles. The other bodies are spheres.
- Clouds: they don't move, cast no shadows, and don't darken the ground's sky light.
Flight physics
- Aerodynamic drag: FlightIntegrator plus DragCube, the six-sided drag table.
- Lift and aircraft: ModuleLiftingSurface, control surfaces, ModuleResourceIntake (jet engines need air).
- Heat: reentry heating, heat conduction, ModuleAblator (heat shields), Radiators/, ModuleCoreHeat, parts overheating and exploding.
- Parachutes (ModuleParachute), landing legs with suspension (ModuleWheels/), wheels, ground vehicles, water buoyancy and splashdown.
- Engines: gimbal (ModuleGimbal), Isp that changes with air pressure, engines with more than one mode (MultiModeEngine), fuel flow order and crossfeed (ModuleToggleCrossfeed), fuel lines and struts (CompoundParts/).
- Parts can break off: impact tolerance, structural limits, explosions.

Controls
- SAS and its hold modes (ModuleSAS): prograde, normal, radial, target, maneuver.
- RCS (ModuleRCS) and translation controls.
- Action groups (AG1–10, gear, lights, brakes), trim, precision control mode.
- Switching between ships, targets, rendezvous display (relative velocity, closest approach).
- Docking ports (ModuleDockingNode) and the grabbing claw (ModuleGrappleNode): merging two ships into one and splitting them again.

Parts and building (the VAB)
- The Part/PartModule architecture itself. Your notes already list this as a next step.
- Attach nodes and surface attach, symmetry (radial and mirror), moving the root part, center of mass/lift/thrust markers.
- Staging editor, part list with categories and search, subassemblies, saving ships (.craft files).
- Checks before launch (PreFlightTests/): missing parachutes, blocked engines, and so on.
- Procedural fairings (ProceduralFairings/), part variants (ModulePartVariants), cargo bays that shield parts (ModuleCargoBay).
- Parts that deploy with animations: solar panels, antennas, radiators, ladders, lights.

Resources and power
- Electricity: solar panels, batteries, generators (ModuleGenerator, ModuleAlternator), what happens when power runs out.
- Other resources: monopropellant, xenon, ore. Mining (ISRU): BaseDrill, ModuleResourceConverter, surface/orbital scanners.

Crew and EVA
- KerbalEVA: going outside, jetpack, climbing ladders, planting flags (FlagSite), carrying items (ModuleInventoryPart, ModuleCargoPart).
- Seats (KerbalSeat), crew in capsules, Experience/ (pilot, engineer and scientist traits and levels).
- IVA (inside-the-cockpit view). Your NOTE has "human in rocket/ship", which fits here.

Game progression
- Science: experiments, biomes (ModuleScienceExperiment), data transmission, science lab.
- Tech tree, funds, reputation, facility upgrades.
- Contracts (Contracts/, FinePrint/: satellites, stations, surveys, rescues), achievements (KSPAchievements).
- CommNet: antennas, relays, signal delay and control (CommNet/), KerbNet.
- Space objects: asteroids and comets (ModuleAsteroid, ModuleComet, SentinelMission).

Game systems
- Saves (quicksave, reverting a flight to launch or to the VAB) and the scene flow: space center → VAB → launchpad → flight → tracking station.
- Converting saves from older versions (SaveUpgradePipeline), settings screen, key rebinding, localization, KSPedia (in-game encyclopedia).
- Audio: engine sound that changes with air density, wind noise, explosions, music. The decompile has an audio/ folder you can check.
- Effects: engine plumes that change with air pressure, reentry flames, dust kicked up on the ground (ModuleSurfaceFX), explosions, camera shake.
- Debug toolbar (DebugToolbar/: cheats, infinite fuel, teleport). This would help a lot with testing labs.

Expansions (optional)
- Robotics (Expansions.Serenity: hinges, rotors, pistons, controllers), deployed surface science.
- Mission editor (Expansions.Missions, from Making History).


blackhole
warp drive light speed relativity special skybox universe


decompile ksp2
decompile ksp
decompile ksa

rainworld in space
