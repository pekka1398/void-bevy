# 主遊戲圖形介面補全

以 `95c0581` 的原生飛行 HUD 為起點。使用者授權先完成所有介面，不以外觀確認或人類驗收作為實作中的停止點。開發在 `work/ui-completion`；未授權本輪合併或 push。

## 邊界與接口

- `fleet_game/ui.rs` 保留飛行儀表、導航球、機動節點數值輸入與共用排版／字型。
- `completion.rs` 提供主遊戲工具列、明確船隻／天體選取、依載具能力顯示的操作按鈕、連續按住的駕駛輸入及對接流程。
- `workshop.rs` 編輯 authored `Craft`，每次修改以現有 `void-assembly::compile` 驗證，使用既有零件目錄、接點、旋轉、表面放置、資源及穩定模組分級。預覽直接投影已編譯的零件姿態和接點；發射使用現有 Fleet Action。
- `menus.rs` 提供暫停／繼續、重新開始確認、存檔／讀檔路徑、錄製結束、光學開關、曝光、UI 縮放、視窗／垂直同步及主要快捷鍵重設。
- `review.rs` 是顯式桌面驗證入口，開啟真實面板並由 Bevy GPU 截圖；不另建遊戲或更換模擬模型。

物理、資料和錄放使用現有核心。存檔不相容、缺檔、無法操作等錯誤顯示明確原因；不產生預設世界替代讀檔。

## 輸入與流程

工具列可進入組裝和導航，Esc 進入選單。飛行面板提供按住操作；放開、失焦、開啟選單／組裝、文字輸入及切換 pilot 都清除上一艘船的連續轉向、RCS、車輪驅動／轉向、EVA 步行。油門、RCS 啟用與停車煞車保留明確既有語義。文字提交／取消以及 modal 關閉的當幀不傳遞 Enter／Esc 等按鍵給底層遊戲。

組裝可在同一世界建立新船，使用者選零件及空接點，調整可用表面連接的局部位置和 twist，配置模組級段及資源，保存／載入 craft，發射至當前天體的合法地形發射點並選取。已存在的船仍保留。存 craft 使用新路徑避免無提示覆寫。

飛行火箭的 nose 為局部 +Y、up 為 +Z，W/S 控制 pitch、A/D 控制 yaw、Q/E 控制 roll；飛機的 nose 為 +Z、up 為 +Y，A/D 控制 roll、Q/E 控制 yaw。介面按實際 profile 顯示名稱，不改變原駕駛命令。EVA RCS 的 X 方向沿用原核心的翻轉，按鈕的 ±X 標示對應實際局部 force。

地圖沿用既有軌道資料和相機；導航清單明確區分 pilot 選擇與天體相機 focus。原機動節點面板仍提供建立、移除、參考系、數值、Ap/Pe 放置、執行、取消及 warp 流程。

## 驗證

針對 app lib、Fleet 外部 checkpoint 邊界測試與受影響 lint；不全 workspace 重跑。合併前檢查所有新系統一起初始化、modal 的輸入 ownership、船隻切換和連續控制釋放，組裝編譯／發射及存檔拒絕等。

桌面驗證：

```sh
cargo run -p void-app -j 2 -- --ui-review workshop --ui-review-size 1280x720 --ui-review-screenshot lab-log/ui-completion/workshop-1280.png
```

`--ui-review` 接受 `hud|menu|workshop|navigation`，截圖約四秒後、七秒退出。不同載具使用既有 `--rover`、`--aircraft` 等 fixture 啟動參數；以 app 現行 CLI 為準。

## 交付限制

這輪提供現有核心能力的可操作入口；沒有新增任務／經濟／科技樹玩法。組裝是已編譯姿態的可選取投影圖與接點操作，尚非任意三維滑鼠拖曳工具。主要快捷鍵與視窗／縮放設定屬本次執行的設定；改動不改存檔格式。人類最終檢視依使用者指示延後，agent 圖像检查與 headless 通過不宣稱取代人類驗收。

## 本輪核對與可執行交付

來源為 `work/ui-completion` 的本輪交付 commit，基線 `95c0581`；對應 commit 與程式 SHA 記錄於 ignored `target/acceptance/ui-completion-SOURCE.json`。model31／world5 未變，沒有跑全 workspace。

- 最終 app lib 55 個測試通過；包含真實組裝刷新／所有零件形狀、模組目標穩定 ID、同世界發射及錄放、導航與載具能力顯示、按住／失焦／modal 釋放、三種 fixture 的選單／快捷鍵重啟一致，以及讀檔後移除過期啟動標記。
- Fleet checkpoint 9 個與 external_checkpoint 3 個測試通過；外部讀檔拒絕保留世界／journal、成功讀取錄放一致，以及 atomic write 不移除其他 writer 的暫存檔。
- app／Fleet lib/tests 的 Clippy `-D warnings`、fmt、diff check、主程式建置及驗收 script 語法檢查通過。
- root 實際桌面 GPU 核對：最終程式 `8b065d3d83cd14a131727220cad46dbbe544ac8e6d084cba459706e9144ef17d` 的飛行 HUD、選單 100%／150% 均為 1280×720，飛機組裝為 1440×900。圖片與 log 在 `lab-log/ui-completion/*-final.*`。先前 `316df3ec609c1d562282022933791a59fdec5f321f1aecaa09386ee43d156b23` 的火箭組裝、導航、漫遊車 1280×720 及飛機 1440×900 也已核對；對應介面結構保留，後續修正為選單遮蔽、推力零值、實際座標文字及操作接縫。舊 `*-first.*` 包含已修正問題的診斷紀錄，不是最終通過證據。

直接操作：`tools/ui-completion-acceptance.sh flight`，可用 `rover|aircraft|orbit|venus` 模式。工具列進入組裝／導航／選單；長面板可用滑鼠滾輪捲動。此 script 核對獨立交付程式的 SHA，不依賴共用 target 的當下 debug 程式。主線原有未提交檔案保持原樣，本輪未合併或 push；人類實際操作檢視仍依使用者指示延後。

## 組裝驗收操作

1. 工具列開啟 Workshop，現有世界暫停。先選 Rocket blueprint／Aircraft／Crew rover／Crew rocket，或 New command pod。上方返回／存檔／發射列固定，下面內容可捲動。
2. Add part 展開目錄，選零件後選擇其 child socket，再按投影圖的綠色空接點或零件列的 Attach。尺寸不合、接點被占用或未選目錄零件時顯示拒絕原因，原 craft 不變。
3. 按投影中的零件或零件列選取。Rotate 以接點法線旋轉 15°；有效旋轉／表面姿態編輯明確把編輯中的 blueprint 升為 craft version 3。Surface offsets 以父零件局部座標移動 0.1 m，僅 authored cuboid surface socket 可用，越出合法表面明確拒絕。此圖是 X/Y 正投影的已旋轉零件外接界，不是完整形狀／碰撞的 3D 視窗。
4. 每個可分級模組有穩定 ID 的獨立 stage 操作；All module stages 是明確整零件重設，清除該零件的模組覆寫。資源按容量調整 10%。质量由 PartGraph 計算；推力顯示所有安裝火箭引擎的標稱真空最大值，不代表當前 stage 推力，不計 jet，也不顯示假定 TWR。
5. Name／File path 點入文字編輯，Ctrl+A 清除、Enter 提交、Esc 取消。Save new file 先完整寫入並 sync 暫存檔，再以不覆寫的 hard link 發布；既有檔案拒絕保存。Load 使用嚴格 craft 格式驗證，錯誤保持原 blueprint。
6. Launch on viewed body 使用當前觀察天體的合法 daylight terrain site，無合法地形的天體拒絕。成功經 journalled LaunchGroundAt 進入原 Fleet，保留其他船、選取新船並更新 pilot／對接埠與顯示資料；主遊戲保持暫停，Resume 後按 Stage 開始。返回 Workshop 可繼續編輯 blueprint，不能藉此直接修改已飛行船的 PartGraph。

## 選單驗收操作

1. Esc 或 MENU 開啟選單並暫停；Close 保留暫停狀態，Pause / Resume 繼續並關閉選單。重新開始需在面板內明確選 Restart current world，再確認；使用當前 session 的 initial world；與鍵盤 R 共用 restart_session，恢復 initial craft 並保留啟動時的 rendezvous／reentry／water fixture。Workshop 的獨立草稿仍保留。成功載入完整 checkpoint 後清除 CLI 啟動 fixture 標記，之後 restart 使用載入檔案的 initial world，不重新套用舊啟動場景。
2. Edit save/load path 支援 Ctrl+A、文字輸入、Backspace、Enter 提交及 Esc 取消。Save / Load 與 F6 / F7 共用處理器。缺檔、格式／模型不相容及非法 checkpoint 顯示拒絕原因，不修改 live world；合法載入後暫停並清理 pilot／prediction／對接埠。
3. 錄製顯示實際 journal 路徑；Finish recording 完成既有錄製，不是假裝開始錄製。尚未錄製時顯示明確狀態與啟動方法。
4. UI 80–150%、Window / Fullscreen、VSync 立即改變實際介面或桌面視窗；Air / Clouds / Ocean / Stars / Terrain 及 Exposure 經原 ViewCommand 寫入錄放。主要鍵盤操作點入後按新鍵；已有其他功能的鍵明確拒絕，避免悄悄奪走 W/S 等駕駛控制。主要快捷鍵及視窗／缩放只保存於本次執行，重開恢復預設；Reset keyboard bindings 恢復預設。
5. Replay 中可以暫停、查看選單及調整本地顯示尺寸／視窗；Load、Restart 及 view mutation 明確拒絕。暫停或關閉選單不清除 replay 的 recorded controls。

外部 checkpoint 的 candidate restore 在核心的明確邊界內將既有 invariant validation panic 轉成拒絕；先完整驗證，再以一個 LoadWorld intent／commit 安裝已還原的 candidate。內部 capture／simulation／journal IO 錯誤不被此邊界吞掉。存檔使用 checked atomic write，失敗只清理本次成功建立的暫存檔，不刪除其他 writer 的檔案。此輪未改 checkpoint schema 或 MODEL_VERSION。
