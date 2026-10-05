# 主遊戲 RCS／對接與完整氣動整合

2026-10-05。開發 worktree：`/home/pekka/Desktop/void-bevy-flight-integration`，分支 `work/main-flight-integration`，基線 `0c60aad`，功能 commit `1fb8ac9`，merge `134bb37` 已合入本地 master。核心 model 17／FleetCheckpoint 7。使用者操作對接場景後認可並授權合併；其他操作的 root smoke 與人類驗收範圍分開記錄。尚未 push。

## 已接入

- 主遊戲預設使用 ForceAndTorque 氣動；既有 Fleet flight example 維持 ForceOnly。
- 預設火箭保留原本零件、著陸腿、幾何、引擎及燃料配置，指令艙增加 20 kg Monopropellant、有限 RCS 噴嘴及鼻端對接埠，起飛質量 7640 kg。
- H 開關 RCS；開啟後 WASD／QE 要求有限噴嘴力矩，Alt＋W/S、D/A、E/Q 要求局部 Z、X、Y 平移。Alt 不同時改油門。非零手動 RCS 力矩以 journal action 關閉 SAS reaction wheel，HUD 明確提示；單純平移不關閉 SAS。
- F10／F11 選本船／目標埠，F12 武裝兩埠，Enter 捕獲，Backspace 解除。拒絕原因、埠距／速度／法向／相對旋轉、供應和分配 residual 可讀。
- 暫停、失焦及切船清除手動轉向／RCS 要求，保留油門及逐船 RCS enable。對接與解除保持相機方向／距離，焦點仍為 craft root。
- 補齊新 catalog 零件的明確 visual 定義，避免核心成功但主遊戲建立畫面時失敗。

## 不用重編的驗收入口

```sh
cd /home/pekka/Desktop/void-bevy-flight-integration
./target/acceptance/void-app --rendezvous
```

`--rendezvous` 以錄放 actions 建立已對準、埠距約 0.15 m 的兩艘完整火箭，從側面看、暫停。另保留初始地面火箭，所以初始共三船。此 preset 不能與 `--load`／`--replay` 同用。

1. Enter：應拒絕未武裝；F12→Enter：三船變兩船，兩艘軌道船合為一艘，相機不跳。
2. Backspace：兩船變三船，兩端 disarm；再次 Enter 應明確拒絕，需要 F12 重新武裝。
3. P 續跑、H 開 RCS，再按轉向／Alt 平移，觀察有限油量與藍色噴嘴線；放鍵後手動要求歸零。Tab 切船、P 暫停可核對舊船控制不殘留。
4. F6 存、F7 讀（暫停）；F8 開始／結束錄製。結束後使用 `./target/acceptance/void-app --verify <錄影路徑>`，不要在錄製尚未結束時核對。

正常發射使用 `./target/acceptance/void-app`，Shift 提升油門、T 開 SAS、Space 點火。F4 可核對實際 collider 與著陸腿。CLI `--save`／`--record` 可指定檔案；錄影不覆蓋已有 journal。

## 驗證與限制

- 合併後：master `134bb37` 與 feature 分支的 core／app 內容一致；main app library 24 passed、combined core 4 passed，兩份真實 GUI journal 在 master 再次 verify 通過，fmt／diff check 通過。
- Core 組合／既有 RCS、氣動、checkpoint、durable、guidance、warp：50 個不同測試通過。新增組合涵蓋 Orbit／Bubble／Ground 的 accepted 耗油、觀察純度、owner 交接、air／RCS rails gate、對接與 pending timestep 存讀／journal 一致續跑。
- Main app library 24 passed；assembly graph 7 passed。相關 core all-target 與 app／assembly／assembly-lab lib／tests Clippy `-D warnings` 通過，fmt／diff check 通過；未跑全 workspace。
- Root 親自讀整合 diff、接口與測試，使用 TigerVNC 操作捕獲／解除、SAS 與有限 RCS、相機、F6／F7、錄製。真實操作 journal headless verify 通過；最終相機版 rendezvous journal／save，以及預設 Full 氣動發射到 T+39.65 s 的 journal／save 均核對通過。這是 agent smoke；使用者已認可對接場景並授權合併，未逐項記錄所有操作的人類驗收。沒有驗證完整入軌任務或其他 GPU／平台。
- Port highlights 是衍生 UI，循環選取不保存於 checkpoint／journal；實際 addressed capture／undock／arm actions 會保存、核對並重播。
- 熱／燒蝕、有限 RCS 驅動 SAS、Scenery 十天體美術不在本輪範圍。原 reaction-wheel SAS 與理想機動導引仍保留。
- Binary SHA-256 與基線見 `target/acceptance/build.json`。詳細測試 log 在 `/tmp/void-flight-evidence/`、`/tmp/void-combined-flight-*.log`；root GUI 圖像／journal／核對 log 以 `/tmp/void-main-*` 命名。
