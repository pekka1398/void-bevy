# 導航效能與資料生命週期：第一輪實作

基底 `f55056a`，分支 `work/navigation-performance`。本輪選擇先解決同步等待、預測
資料的 ownership／預算，以及曲線顯示密度；没有修改天體步長、Yoshida8、Hermite、
船舶 DOPRI5 容差或力學模型。主遊戲新 durable command 使 model 由33升為34；
舊版本存檔／錄放明確拒絕，沒有自動改寫。

## 已實作的路徑

- **主遊戲背景導航**：Depart／Correct／Capture 先建立精確只讀快照，完整的既有計畫
  尾端、導航搜尋、有限推力驗證、追加計畫驗證均在 worker 執行。初版明示要求暫停；
  不會自動暫停、點火或耗真實燃料。鏡頭／介面繼續更新。
- **工作生命週期**：一個 running job＋最新 queued request。取消、恢復時間、選船、
  改控制／計畫、載入世界等使舊工作失效。接收時再次比較船、engine、world、frame、
  plan inputs 與精確 celestial continuation fingerprint；取消／過期結果不提交。
- **共享星曆快照**：單星系 Arc chunks／coupled Arc samples；複製小型積分器續算狀態，
  含 Kahan 補償，不複製整份樣本 payload。新寫入採 COW；worker 不借用 live Fleet。
- **完整結果接線**：成功計畫需要的未來星曆一同接收，讓主遊戲畫軌跡時不重新推演。
  Coupled adoption 保留既有 Rc owner 身分與各自 source view。失敗預測不擴張 live cache。
- **明示資源限制**：每 job 256 MiB 保守累計 allocation reservation、2,000,000 個天體步、
  4,000,000 次 vessel trials（含 rejected/refinement），交付軌跡最多131,072 samples。
  長區間 preflight 在配置前拒絕，資源錯誤帶 required/limit。檢查取消涵蓋推演與候選查詢。
- **錄放**：`CommitNavigation` 記錄完整 prepared plan，replay 驗證結構／輸入並重建所需
  天體 coverage，不重新搜尋導航。worker 預先編碼這個 command，主執行緒仍先把 Intent
  寫入並同步磁碟才安裝計畫；沒有改成未落盤就提交。外部錄放不走私有 prevalidated 快路徑。
- **曲線呈現**：f64 誤差導向取樣先覆蓋全航程，再按估计誤差從大到小細分；輸出／查詢量
  有界。鏡頭焦點深度的0.75 pixel估值向下量化成世界距離門檻，預設8192點；達點數／深度
  上限會在導航資訊顯示。paused 的節點／近遠心點分析快取，移動鏡頭只重放置。
  BodyPlots 也會在背景接收改變 source coverage 時刷新，避免暫停畫面沿用舊天體軌跡。

## 效能證據與界限

硬體沿前輪 Ryzen 7 H 260／RTX5060 Laptop；CPU benchmark 是 release headless，
不能換算成 GUI FPS，亦沒有與其他編譯同時執行。

原 `navigation_profile` 的58體固定高推力 fixture：同步和 bounded prediction 的
完整 NavigationSolution、終點與128,286,720 bytes保留相同。snapshot約0.039ms，
求解約3.04s，保守預算使用136,317,658 bytes；這是一次樣本，不宣稱小時間差為加速。

真正 Fleet／PartGraph 的上面級 rocket 使用更嚴的原生 tolerances，約870,333次
vessel trials，故完整月球出發約9s；不能拿它和原benchmark的3s當相同負載。
改動讓這段等待移到背景，沒有聲稱把9s計算變成幾毫秒。

初次端到端量測：快照0.54–0.74ms、idle poll最大約0.002–0.011ms；成功結果普通接收
約4.66ms，durable record時約39ms，其中35.56ms在journal。據此移走JSON編碼並保留
同步durability。最終重測結果與GUI證據在下方驗證記錄更新；2ms接收是目標，非保證。

近地折線的synthetic witness：10,000s軌跡中的短100m曲段，被原512時間間隔漏掉。
新方法的誤差／點數由view tests量測。它證明舊畫法有此類限制，**不是已重現使用者
當時的場景**。取樣僅改presentation，沒有用簡化後折線求力、耗油或事件。

## 明確限制

- 256MiB是每job的累計保守reservation，包含共享source的邏輯保留、新配置與交付複製，
  不是全process RSS上限；丟棄候選不退還此工作預算。取消在檢查邊界生效，沒有硬即時保證。
- 成功計畫使用的未來星曆會保留供live讀者；沒有新增全域LRU、active-reader leases、
  單星系自動舊歷史回收、稀疏checkpoint archive或有損壓縮。不能宣稱長期主遊戲RAM已完全有界。
- Coupled沿用8192樣本政策。prediction不准為延伸未來而淘汰可能仍被讀取的過去，
  需求超過容量會明確拒絕；長窗口多星系導航因此仍有限制，没有自動縮短窗口／改單星系模型。
- 只在暫停時提交導航；未實作飛行中任意完成時刻的安全rebase。已在執行的機動不接受新計畫。
- 曲線門檻是焦點深度近似；quarter probes＋有界knot/time seeds是誤差估計，不是對任意
  窄曲段或任意透視深度的嚴格上界。點數上限時全航程仍有分布的粗表示，UI明示限制。
- 同步 `GenerateNavigation` 保留給既有headless／測試路徑；UI按鈕使用有界背景入口。
- 多worker仍opt-in研究，不增加一般遊戲CPU競爭；GPU packing／LOD優化仍在獨立LOD分支，
  未合入本分支。未改星際Fleet外層chunk或改用獨立Traveller runtime。

## 驗證與使用

完成前的檢查記錄：core78 passed＋既有ignored1；最後變動導航8／prediction5補驗，
舊golden與門檻未修改。Fleet生命周期、checkpoint、plan、session、durable及同步物理
計畫逐位元對照由所屬tests核對；最終GUI／app結果待本輪完成後列於下方。
所有raw logs、source manifest、圖片、benchmark JSON保存在ignored
`lab-log/navigation-performance/`。不執行全workspace、不merge/push。

驗收artifact準備完成後：

```sh
tools/navigation-performance-acceptance.sh
```

使用model34 staged上面級停泊軌道場景。保持暫停，在導航面板選Selene並按Depart；
檢查計算期間可旋轉／縮放鏡頭、可Cancel，完成後橙色計畫與機動資訊可檢視。
恢復時間、換船或改計畫應取消舊結果。航程超預算須顯示拒絕原因；不應自動耗油／點火。
人類最終遊玩驗收與agent GUI/headless紀錄分開。

研究與後續gate：[Principia方法調查](principia-methods-review.md)、
[本輪實作規格](specs/navigation-performance.md)。
