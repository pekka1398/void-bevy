# 主遊戲天體擴充：第一批

分支 `work/expanded-bodies`，獨立工作區 `../void-bevy-bodies`。
實作 `062193e`，小半徑光學查表修正 `97c4394`；尚未合入 master／push，待人類驗收。

主遊戲預設 Sol analogue 從 15 個天體擴充到 58 個（新增 43 個），涵蓋各行星主要衛星、
冥王星系統、知名小行星、外海王星天體及四顆彗星。原 Galilean／Titan 類比 ID 保留，
顯示名稱補上真實對應。完整清單與逐體資料契約見 [規格](specs/expanded-bodies.md)。

所有天體有明確環境／場景配置，可用地圖、焦點與 O 軌道 fixture；新增固體使用
程序球形地形，繪圖與碰撞共用取樣器。小天體 fixture 高度依半徑設定，既有大天體維持
400 km。模型版本為 32，舊版本存檔／錄影明確拒絕。固定 golden 星系保持 15 個天體。

## 驗證

2026-10-09，`97c4394` 工作區；沒有跑全 workspace，也沒有長期穩定性實驗。

- `cargo test -p void-orbit -j 2`：24 項通過，含原 golden、58 天體資料契約及一小時有限值取樣。
- Fleet 指定 `expanded_bodies`、`solar_scenery`、`ui_presentation`：11 項通過，含所有天體 focus／環境、代表性 fixture、checkpoint 與 replay。
- Scenery：19 項通過，含既有逐位元光學 golden 與小天體查表邊界回歸。
- Root `cargo test -p void-app --lib main_ -j 2`：7 項通過，含實際場景、軌道預測及錄放／續跑。
- Orbit／Fleet／Scenery scoped Clippy `-D warnings`、app check、fmt、diff check 通過。
- Root `cargo build -p void-app --bin void-app -j 2` 通過；共用 target 的本地 crate 來源核對為此 worktree。
- 離線 `tools/regenerate-expanded-catalog.py` 重產與提交資料逐位元一致。

Root TigerVNC／Vulkan 初步檢查：Bennu 焦點畫面、O fixture、F6 存檔與 F7 載入；
實際 journal 用分支執行檔 `--verify` 核對通過（T+4 s、2 艘船、selected v2）。
Triton 焦點畫面、O fixture 與短時間運行已檢查（HUD 參考 Triton，Orbit owner，約 399 km AGL）；
未見 panic／shader validation 錯誤。截圖、journal、checkpoint、GUI logs 與 binary 來源摘要位於
忽略目錄 `lab-log/bodies-review/`。Agent GUI 不代替人類驗收。

## 驗收入口

在 `void-bevy-bodies` 執行：

```sh
./tools/bodies-acceptance.sh phobos
./tools/bodies-acceptance.sh bennu
./tools/bodies-acceptance.sh enceladus
./tools/bodies-acceptance.sh triton
./tools/bodies-acceptance.sh pluto
./tools/bodies-acceptance.sh halley
```

分支專用 binary 為 `target/acceptance/void-app-bodies`，script 核對
`target/acceptance/bodies-SHA256SUMS`。可切焦點、觀察近／軌道視角、O 生成 fixture，
暫停後 F6／F7 核對存讀。這輪不以長時間 warp 作穩定性驗收。

新增軌道是來源參數組成的 authored Jacobi 初值，不是真實共同日期星曆；資料缺測的
估算逐體標記。形狀、顏色為程序近似，Titan 大氣、彗尾／噴氣尚未建模。
Phobos 的既有導航 SOI 小於半徑，HUD 預設導航仍可能選 Ares；明確焦點可選 Phobos。
這批尚未收錄所有已知小衛星。N 體效能與木星／土星系長期穩定性留待後續。

## 標籤小修正

Sol／恆星 22 px、八大行星 18 px、衛星與小天體 11 px；遮擋及圓點對齊依文字高度更新。
主遊戲 `draw_map` 使用 `draw` 當幀更新的 root camera Transform，避免 Update 中讀到
尚未在 PostUpdate 傳播的上一幀 GlobalTransform，造成拖曳時標籤與場景錯開。
當幀焦點投影與既有拖曳／縮放錄放兩項針對性測試、app scoped Clippy 通過；
沒有修改模擬模型版本或添加標籤延遲平滑。執行檔依原驗收入口更新。
