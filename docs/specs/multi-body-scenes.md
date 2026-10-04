# 任務：多天體世界描述與場景管理

狀態：審查修正／TigerVNC 核對完成，依使用者授權合併 master；整合模型 13。任務分支 `claude/multi-body-scenes` 保留歷史。
起始程式基線：`7a4e5d2`（已審查的四項重構，MODEL_VERSION 9）。
相關：[NOTE.md](../../NOTE.md)、[environment.md](../environment.md)、[frame-tree.md](../frame-tree.md)、[scenery.md](../scenery.md)。

## 目標與完成邊界

在同一世界內，至少兩個天體有自己的地形／環境／視覺設定，船能在各自地面接觸、軌道飛行、切換觀測焦點及存讀檔。第一個場景是 Aurelia 類有大氣／海的母星，加一顆無大氣月球類天體。

此次以「第二顆可著陸天體」驗證資料與生命週期，不做整個太陽系的美術，也不只是用 CLI 重開另一顆發射星球。

## 現有接口與問題

- Environment 已有每個天體的 BodyEnvironment；Fleet 接收 Vec<GroundSpec>。先利用現有能力，不能另寫多天體物理系統。
- `fleet-flight/src/lib.rs` 的 FleetFlight 持有單一 planet／home；new 建一個 ground。
- `fleet-flight/src/session.rs` 的 InitialWorld 只有 body_id／terrain／air_density_scale；checkpoint restore 透過它重建一顆母星的環境。
- `app/src/fleet_game.rs` 的 Ground 資源只有一個 TileField；scenery 建構、材質、光照、HUD／高度等仍有 home 假設。
- 在地形、物理設定、視覺和世界初始化之間建立共同天體身份；不能靠 Vec 順序暗示它們是同一顆。

## 世界資料規則

1. 可序列化的世界描述包含星系設定、每個已配置天體的穩定 body ID、TerrainConfig、物理大氣／高度零點／海、必要視覺設定，以及明確發射天體和位置。
2. 不把 Bevy Handle、LOD live cache、GPU 資產放入核心／世界存檔。純資料配置放適合的 core crate；具體 crate 和型別由第一階段設計提案決定，避免 environment 反向依賴 app。
3. 環境和 renderer 從同一份天體描述建構；地形參考半徑、大氣零點、海平面一致。物理大氣參數與散射參數不必同型別，但要有共同 datum 並明確說明相互關係。
4. 無大氣／無海／無地形是明確合法配置，不能自動套地球預設。重複 body ID、未知 ID、半徑不合、非法數值、存檔世界不相容均拒絕。
5. home 只保留「發射母星」意義，不再代替目前附近天體、導航參考天體、相機焦點、渲染天體。各角色明確命名和決定，切船／切焦點不能偷偷改物理世界。
6. 舊單天體 preset 仍能透過顯式建構函式建立等价新世界。序列化格式不相容要版本拒絕，不能在載入失敗後改建預設母星。

## 場景與資產生命週期

- 每個天體有獨立 TileField／材質與配置，tile／renderer entity／collider cache 身份包含 body ID，避免相同 TileKey 或 origin 相撞。
- 幾何直接經既有 frame tree 到 camera；不得把跨天體局部點轉成絕對 f32 座標。root 相機追蹤與遠尺度 split 精度保留。
- 視覺 LOD 依觀測位置／焦點；物理 collider 依各 live 船的接觸需求。遠處未觀測的船仍正常模擬和接觸，不能因 renderer 卸載就刪 owner。
- 定義近景載入／遠景表示／卸載條件、滯回與資產釋放。離開近景的天體仍有一致遠景；不能在過渡時消失或形成錯誤尺寸。LOD 的合法粗化不是錯誤 fallback。
- 第一版不要求同畫面兩套體積大氣合成：先明確選定可處理的觀測天體，無大氣月球不能沿用母星散射／雲／海。不支持的多大氣重疊視角須列明，不能標為全宇宙 renderer 完成。
- 視覺資產的非同步建構須帶世界／天體／配置 generation 身份。reset／load／切換期間舊工作完成不能掛進新場景。對應測試或可控制建構順序的檢查必須證明。
- 光照方向與尺寸讀當前世界和觀測天體，不能沿用 home 的自轉軸／太陽向量。不在本輪實作多恆星光照或 eclipse。
- 記錄載入天體數、tile／collider 數、pending 工作和 mesh／資產占用；避免反覆切焦點造成無上限累積。不需承諾全部 GPU 驅動已釋放，但須能核對 app 擁有資產的上限。

## 物理與遊戲接線

- 根據世界描述建立多個 GroundSpec 和同一 Environment，沿用 Fleet owner 交接，不新增獨立船／月球 simulator。
- 自動或顯式近地參考選擇有明確規則；實際碰撞是配置天體的 terrain，與 HUD／疊圖一致。导航選擇不能決定有沒有碰撞。
- HUD 清楚區分發射母星、導航参考和目前地面／近景天體；AGL 對實際選定地形取樣，無地形不能給假 AGL。既有慣性／地表速度選項維持。
- lab 提供同世界兩船分別在两天體、第二天體上空降落、跨天體轉移起點與焦點切換 preset。生成预置可用于验收；真實跨天體軌跡驗證要由時間推進完成，不能 teleport 冒充。
- 保留機動、warp 攔截、切船、存讀檔和錄放；針對另一顆天體的交接／rails 判斷不得仍只看 home。
- 存檔包含完整世界描述、所有船及 owner；還原後重建每顆配置環境並核對。renderer 由還原世界重建，不恢復舊 handles。
- 錄放保存世界身份、生成／切焦點命令及必要觀察狀態；摘要含足以抓到錯天體／錯 owner 的資訊。

## 交付順序

1. 盤點 home 使用點並分類，提出世界描述、觀测選擇、資產管理與 checkpoint／journal 的接口變更，先供審查。
2. 多天體 core 初始化及存讀檔，headless 兩天體接觸／owner／rails 檢查。保持單天體情境可用。
3. 獨立 lab 的近景／遠景、scenery、HUD／疊圖、切焦點和資產卸載。先提供無大氣月球及母星場景。
4. 使用者 lab 驗收；依結果決定正式主遊戲啟用。共用 app 接線如不可避免，維持現有單天體主遊戲行為，不先把未驗收新模式設成預設。

## 數值／行為驗收

- 同 body 描述重建後 terrain 取樣一致；兩個不同天體的同 TileKey 不互用。地形 collider 與繪圖共用配置，代表方向／tile 邊界取樣核對，不只看設定字段相同。
- 在各自局部 frame、船 frame、camera frame 的代表位置／姿態轉換，保留依精度尺度制定的既有門檻；離船米尺度與天體軌道尺度同時覆蓋。
- 兩船分处不同地面，沉降／睡眠／rails／續跑各正確；另一船未觀測時仍有效；轉移船在第二天體進出地面 band 正確，時間和狀態連續。
- 第二天體上空存讀檔、两天體地面同時存讀檔、轉移途中存讀檔；續跑和不中斷對照一致。錯世界、錯 body、錯 terrain 明確拒絕。
- 多次焦點切換、reset、load，核對 entity／資產計數能回到可說明範圍；慢速舊建構工作不能污染新世界。
- 單天體基線的物理、相機／存檔／錄放和渲染參數保留；行為改變另列。GPU 畫面須使用者驗收，headless 不宣稱 shader 已驗收。

## 使用者視窗驗收

提供固定 lab 命令和按鍵表，能直接進兩天體場景：

1. 在母星與月球船之間切換：鏡頭固定 root，月球無母星大氣／海／雲。
2. 查看月球地形、F2／F3／F4／F5，確認實際地面碰撞與外觀、尺度一致。
3. 從月球上空预置降落、起飛、warp，另一顆星球的船繼續存在。
4. 看近景／遠景過渡，反覆切焦點及載入，沒有舊 tile、錯材質、閃掉天體或資產無限增加。
5. 多天體保存／載入、錄放，確認世界、相機與船狀態保留。

## 不在範圍

全部太陽系地形美術、植被／天氣／雲影、新 LOD 算法、星際主遊戲整合、相對論、完整多大氣體積合成、多恆星 renderer、浮力／水下物理、正式 UI、全部歷史 lab 刪除。

## 分支整合及檢查

- 另一任務是 `claude/part-state-resources`，其主責零件與資源。這邊主責 world 初始化／InitialWorld／天體資產管理。Fleet checkpoint、session、world_mark 等交集先列接口，不能覆蓋另一邊修正。
- MODEL_VERSION 基線 9；世界格式改變需顯式版本識別，最終合併依順序分配唯一模型版本，避免兩分支都自稱同一個新模型。craft／catalog 版本不由本任務任意修改。
- 使用者取消了全量測試；跑實際受影響 crate／場景的針對性測試、對應 all-targets Clippy、fmt；如需要全量另說明理由，不擅自重啟。
- 保留環境世界核對、PartGraph 驗證、root 相機、split 精度、精確軌道時鐘、F6 緩衝原子存檔及 durable journal。
- 最終 commit／push 的視窗驗收依 AGENTS.md；交付真實檢查結果、限制及未完成項，不能把「世界有兩份配置」當作本任務完成。

## 後續追加範圍

使用者追加多大氣支援與太陽系 scenery 開發入口。已補 per-body optics/cloud profiles、HDR transport、世界配置匯入／匯出及 [solar-scenery.md](solar-scenery.md) 交接規格。原「不要求多大氣」邊界不再代表最新實作；共同逐射線積分／相穿介質仍未實作。最新實作已由 agent 核對，使用者授權審查後合併 master。

最終整合狀態：MODEL_VERSION 13，world schema 2、craft 2、FleetCheckpoint 4；分支暫定版本為歷史紀錄，master 以此為準。
