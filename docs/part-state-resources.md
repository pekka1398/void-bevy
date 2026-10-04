# 零件資源與降落傘 lab

本輪新增 typed 質量資源、按穩定 module ID 尋址的動作狀態與 staging、降落傘狀態機。核心已接 Fleet，但主遊戲預設火箭沒有新增降落傘；此 lab 尚待使用者視窗驗收。具體格式與算法見 [設計](specs/part-state-design.md)。

## 驗收

```sh
cargo run -p void-part-state-lab -j 2
```

按 1–4 換場景，P 暫停／继续，W 切換 1x／4x physics；HUD 顯示每個 part/module ID、phase、elapsed、面積、資源、質量及 rails 拒絕原因。

| 場景 | 操作與要看什麼 |
| --- | --- |
| 1：既有火箭 | T 開推力與 SAS、Space 分級；保持舊火箭行為與觀察值。 |
| 2：雙資源 | 兩引擎使用 liquidPropellant／monopropellant，各自消耗；一種先耗盡不應熄掉另一種。 |
| 3：返回 pod | D 展傘，觀察 Armed→SemiDeploying→Semi→FullDeploying→Full、面積連續增長、下降減速；P 停住時 elapsed 不變，C 切傘後面積為 0。 |
| 4：雙傘分離 | D 展開两傘，Space 分離、Tab 切船、J debug join；各 part/module 狀態和資源應跟隨自身身份而保留。 |

F6 存入 `lab-log/part-state-save.json`，F7 載入。半開和全開時各存一次，繼續飛、再載回，確認 phase/elapsed/資源恢復並能繼續。F8 開始／停止錄製到 `lab-log/part-state.jsonl`。

```sh
cargo run -p void-part-state-lab -j 2 -- --replay lab-log/part-state.jsonl
cargo run -p void-part-state-lab -j 2 -- --verify lab-log/part-state.jsonl
```

verify 不開視窗，重跑 durable journal 並核對完整 world mark。這是同 build 的回歸核對；舊 model 9、舊 Fleet checkpoint 3 明確拒絕。Craft 1 的轉換需顯式執行：

```sh
cargo run -p void-assembly --example migrate-craft -j 2 -- OLD.json NEW.json
```

## 實作邊界

- 目前兩種資源皆以 kg 計入 COM／慣量。電力等非質量量綱未實作。
- 傘開啟高度相對大氣模型海拔 datum，並非 AGL；最低氣壓／最高動壓控制開始半開。
- 傘力在零件位置採樣、純求值，狀態與消耗只在 accepted 時間提交。與既有 aero 相同，第一版只提交總力，沒有偏心氣動力矩及 angular airspeed；HUD 有明示。
- 只要世界包含大氣，active 傘一律禁止 rails，包括目前仍在真空中的船；沒有大氣的世界才允許。這是明確的暫時限制，尚未實作沿整段軌跡的大氣穿越事件偵測。展開用固定 physics ticks，渲染幀長不决定 phase；presentation/pending clock 可能存在浮點加總尾差。
- 旧 assembly editor/local flight 是單液體、單引擎 runtime，會明确拒绝不支援的新配置。新的多資源與傘請在本 lab 操作。
- 当前历史没有可核对的未合入 RCS 实作；未来 consumer 应复用 typed supply pool，不能另建专用扣油路径。

## headless 覆蓋

新增檢查涵蓋非法資源／module ID／transition、獨立引擎 stage、共享池同步耗盡、crossfeed 與 vessel members 邊界、真空中的 active 傘在有大氣世界拒絕 rails、半開／全開存讀與 journal 全狀態核對、雙傘 split/join 身份保留、不同 render delta 的 accepted 狀態一致。

原六個 Fleet 場景另在 baseline af31f1e 與重構後各輸出 4056 行既有觀察值，比對逐位一致；TS golden fixture 與門檻未修改。

agent 初輪驗證（審查前）：assembly 22、modules 6、vessels 43、FleetFlight 53，共 **124 passed、0 failed、1 existing ignored**（分批執行、按唯一測試計數）。六個 package 的 targeted all-targets Clippy `-D warnings`、新增返回行為測試 Clippy、fmt、主遊戲與受影響 labs 的 all-targets check 通過。`void-part-state-lab` native build 通過，未操作視窗／未 commit 或 push。

審查補強：拒絕重複 JSON 資源／模組／stage key；各傘獨立取樣空氣，COM 在大氣外不會漏掉大氣內的傘；contact 使用原生場景框架並區分主動推力與被動空氣力；存讀恢復 pause/rate。審查後驗證記錄另以實際執行結果為準。

本輪審查後定向驗證：resources、part_states、parachute、modules、fleet、contact、durable 共 56 passed。五個受影響 package 的 all-targets Clippy 通過；其後 modules 與 lab 再次 Clippy 通過。没有跑 workspace 全量測試。

TigerVNC 實際操作：預設 paused 下 D 只進 Armed，P 之後展開；已補明確暫停提示、無傘提示、傘繩及傘面厚度，畫面確認 Full 80 m²。這是 agent 的操作核對，不替代使用者最終驗收。

最終核對：受影響 assembly／modules／vessels／FleetFlight 的 lib/tests 共 128 passed、0 failed、1 既有 ignored；對應五個 package all-targets Clippy 通過。修正模組級 stage 不再依賴舊 part stage，傘可直接用 part stage。TigerVNC 核對半開存讀、切斷、恢復續開、雙傘分離／切船／debug join、雙資源消耗，實際視窗錄影 --verify 通過。使用者已接受目前傘外觀；未 commit／push／merge。
