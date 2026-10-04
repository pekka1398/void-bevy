# 多天體世界與獨立場景

狀態：實作與針對性驗證中，未經使用者視窗驗收，未啟用為主遊戲預設。

## 世界模型

`void-fleet-flight::world::WorldDescription` 保存 SystemSpec 和穩定 body ID 對應的天體配置。配置包含可選地形、物理大氣密度與高度 datum、可選海面及視覺大氣／雲／海和配色高度。無配置天體只有重力／遠景，不自動新增地形或空氣。world schema 2；master 整合版 simulation model 13（分支暫用 12）。

`InitialWorld` 只保存 world、launch_body、craft、launch_site，沒有第二份單天體配置。`WorldDescription::build` 是初始化與 checkpoint restore 共用入口；同時建立 Environment 和每顆 terrain 的 GroundSpec。所有天體仍在同一 Fleet／N-body ephemeris，沒有另一套月球物理。

`home` 保留發射身份。`nearby_body` 以船 root 在各配置地表框架中的高度選最近地形；`observation_body` 可以由天體焦點明確指定。預測／plain camera 不再把 home 當現在附近天體。导航及觀測不影響物理 ground owners。

舊 `InitialWorld::new` 明確轉成單天體 world；沒有載入舊格式失敗後改建預設世界的路徑。checkpoint 世界描述與 launch body 核對，world marks 帶 world descriptor、launch body 與 ephemeris stable IDs。新增命令 LaunchGroundAt／LaunchOrbitAt／LaunchFlightAt 皆記錄天體身份和局部起點。

## lab

```sh
cargo run -p void-app --example multi_body
cargo run -p void-app --example multi_body -- --record lab-log/multi-body.jsonl
cargo run -p void-app --example multi_body -- --replay lab-log/multi-body.jsonl
cargo run -p void-app --example multi_body -- --verify lab-log/multi-body.jsonl
cargo run -p void-app --example multi_body -- --load lab-log/multi-body-save.json
cargo run -p void-app --example multi_body -- --verify-save lab-log/multi-body-save.json
```

Aurelia 使用正式主遊戲 layered 地形與原 scenery shader；Selene 使用月球半徑 hills 地形，視覺指定裸岩、無海／大氣／雲。兩艘正式 assembly/Fleet 火箭同時在兩地面，開始暫停。不是 CLI 重開另一顆星球。

| 按鍵 | 操作 |
| --- | --- |
| Tab | 切船，鏡頭回船 root |
| 1／2、Home | 焦點 Aurelia／Selene、回選中船 |
| 滑鼠左拖／滾輪 | 相機方向／距離 |
| P、逗號／句號 | 暫停／九級 warp |
| T、Space、Shift／Ctrl、WASDQE | SAS、分級、油門、姿態 |
| O | 在觀測天體生成 400km 軌道船 |
| L／I | Selene 2km 降落／20km 接近 fixture，20m/s 向下，生成後需 P 續跑 |
| M、B、Z | 在当前導航天體新增 +20m/s 機動、執行、機動前快轉；未點火引擎會明確拒絕 |
| F2／F3／F4／F5 | 地形線框、tile 邊界、讀回實際 Rapier 地形／船 collider、隱藏外觀地形 |
| F6／F7／F8 | 完整世界快存／快讀／停止錄製 |
| R | 重設同世界兩地船 fixture |

L／I 是可重現的初始接近狀態，不宣稱是從地球完整飛來月球；跨天體真實時間推進在小型雙體 headless 場景驗證，沒有中途 teleport。

## renderer 生命週期

每顆配置天體持有獨立 TileField、材質和兩張地面查表；每天體有自己的 volume lookup／光學與雲參數；immutable cloud noise 共用。只有一顆觀測天體的近景 LOD 啟用，其餘保持依 frame tree 放置的遠景球。每次切船／切焦點清掉近景 entities 和 app 所持 mesh，丟棄前一 TileField 的 pending Task，重新建立其 job map。只有當近景 LOD 已有可繪集合才隱藏該球；LOD 自身的拆合滯回維持。

reset／load 重建 terrain Arc identity，觸發新 generation；舊 Task 僅屬於舊 field instance，不能被新 field 接受，即使 TileKey 相同。不同 world descriptor 載入時明確清理舊 ground materials／images／far meshes/materials，從新 descriptor 重建；不還原舊 handles。HUD 顯示 generation、近景／配置天體数、terrain meshes、pending、cache bytes、app mesh/material/image asset數，以及 live ground owner/collider數。

所有幾何（零件／球／collider／相機）走 frame tree；近景 tile origin 先在該天體局部 f64 減 eye 再轉 f32。物理 ground cache 永遠由 live ships 管理，renderer 卸載不影響未觀測的船。

## 限制

- 支援多顆分離的大氣，包含無 terrain 的大氣遠景。whole-volume passes 只接受順序明確的視角；相穿的大氣球、投影與徑向範圍同時重疊的歧義视角明確拒絕，尚非共同逐射線積分 renderer。
- 沒有多恆星／eclipse。所有光照由當前世界根天體和觀測天體的框架求方向。
- 新模式在獨立 example；正式主遊戲仍使用原單天體renderer與按鍵，尚未決定啟用新模式。
- 美術／GPU shader 顯示與轉場仍待使用者視窗驗收。

## 視窗核對操作

1. Tab 在兩船切換；月球沒有母星雲／大氣／海，root相機不追質心。
2. 月球F2-F5線框與實際地形、腿／零件碰撞可觀察。L降落並接觸，母星船仍存在。
3. O軌道、SAS／分級／油門與warp；1/2放大天體觀察近遠景。
4. 反覆Tab、R、F6/F7，觀察asset數維持有界、沒有舊tile/錯材質。
5. 錄影F8結束，verify/replay含焦點、完整世界、所有船狀態。

## 數值與生命週期檢查

- 多天體測試涵蓋兩顆可著陸天體同時存在、兩地船睡眠後一天 rails、完整 checkpoint 還原與 replay、月球接近／contact 交接，以及小雙體中靠時間積分真正從主星抵達伴星。
- 分別讀回兩顆天體的實際 Rapier terrain vertices，核對各自的 terrain sampler；拒絕半徑 mismatch 和重複 stable body ID。
- TileField unload 檢查 entity／mesh 移除、pending jobs 清空，新 field 不接收舊工作。Bevy draw system 初始化檢查 query 存取互斥。
- 接觸步進將主動推力與被動空阻分開：被動阻力不反覆喚醒已靜止的船。場景空阻直接在 scene frame 查询 Environment，避免先 flatten 到 1 AU 再轉回地面座標。舊 ContactWorld::step 的主動外力語義保留。

這些檢查不代表 GPU／視覺驗收完成，仍須執行上面的視窗核對。

agent 初輪檢查結果（審查前）：`void-fleet-flight --all-targets` 55 passed；`void-landing`／`void-modules`／`void-vessels --lib --tests` 76 passed、1 既有 ignored；補上的 framed-air 檢查使 modules 單獨 6 passed。app 的 TileField 資產釋放與 ECS query 檢查各 1 passed；`multi_body` example build 通過。上述五個受影響 crate 的 all-targets Clippy（`-D warnings`）通過，fmt／diff check 乾淨。沒有跑 workspace 全量測試。

審查補強：airless preset 不會因 air 開關產生大氣；自訂 sea／air datum 在 legacy projection 與存讀保留；主遊戲明確拒絕不支援的場景描述。載入同 part ID 的不同 definition 時清除舊模型，讀檔恢復 pause/rate。

本輪定向驗證：多天體核心 9、renderer ECS／模型生命週期 2、durable 6、landing environment 2、modules 6，共 25 passed；app／FleetFlight targeted Clippy 通過。沒有跑 workspace 全量測試。

啟動回歸修正：零消光時散射步進積分使用精確極限 dt，避免 0/0；scenery 5 項（含既有 golden）通過，production build_scenes 在混合大氣／全無大氣兩種世界通過。TigerVNC 已核對啟動、Tab 切換及 F6/F7 存讀，未再出現此 NaN；不代表所有 GPU／視覺情境已驗收。

最終核對：受影響 FleetFlight／modules／landing lib/tests 與 multi_body renderer 測試共 101 passed、0 failed（本輪新增 datum 球半徑驗證计入）。scenery 的 5 項含既有 golden 通過。對應五個 package all-targets Clippy 通過；最後改動再跑 app／FleetFlight Clippy，兩個 lab 重建完成。

TigerVNC 核對母星／月球切船、F2–F4 疊圖、兩次存讀後月球近景 asset 計數回到相同 1150 meshes／48 materials／19 textures、月球上空至 Ground 真正降落、兩天體遠近景焦點。月球 fixture 改為向陽面；回到船焦點依當地地平線重置相機，避免沿用母星方向。F6 在保存前提交 pause/rate。此段為擴充前的驗收紀錄；下方新增多大氣與無地形大氣遠景支援。

程式核對完成；多天體最新視窗改動仍待使用者最終驗收。未 commit／push／merge，兩分支最終合併仍須協調模型版本與共享 Fleet／air 改動。

## 多大氣與 scenery 開發入口（後續擴充）

每日天體有可序列化 `AtmosphereProfile`（EarthScaled 或 Custom）與 `CloudProfile`。大氣可無 terrain，optics 和物理壓力模型分開；不自動把自訂散射參數当作外星壓力。`AirLayers` 依 camera-relative f64 體積順序合成，逐層保留 HDR，最後 resolve 曝光與 tone mapping 一次。legacy 單大氣 camera 不需新增 component。所有貼圖就緒前不開始 ping-pong，避免半套合成。

`--two-atmospheres` 是虛構紅色月球大氣 fixture；預設 Selene 仍無大氣。`--export-world <path>` 匯出完整 InitialWorld；`--world <path>` 讀自訂配置。world schema 2 / master MODEL_VERSION 13 明確拒絕舊格式。

TigerVNC 已操作雙大氣母星／月球近景、月球軌道邊緣、F6/F7。該實際視窗記錄與 checkpoint 的 headless verify 通過。scenery＋fleet-flight 針對性數值／錄放／golden 測試、app 多天體測試與所屬 all-targets Clippy 通過；沒有全 workspace 測試。使用者已授權審查確認後合併；實際 TigerVNC 核對保存在本文件。

交接規格：[solar-scenery.md](specs/solar-scenery.md)。九個主要天體實際外觀、太陽發光、氣態包層、環、各天體新地形算法未實作，清楚留作後續任務。

整合版合併保留 typed resources/module states 和 per-chute framed air sampling；新增第二天體降落傘 checkpoint／replay 整合回歸。主遊戲仍維持單天體預設，獨立 multi_body lab 提供新世界驗收。
