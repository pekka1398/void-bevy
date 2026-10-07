# 主遊戲繪圖框架

主遊戲 1／2／3／4 選擇質心／天體慣性／天體地表／雙體旋轉框架；G 循環。
J 循環參考天體，Shift+J 循環第二天體並進入雙體模式。HUD 顯示 FrameSpec 與索引，
天體索引對應世界 system 的順序。這些操作只改繪圖，不改物理 owner／積分框架。
模式和明確天體 ID 隨存檔與 journal 保存，模型版本 18 拒絕舊模型存檔／錄影。

縮遠進入 map 可觀察預測與機動路徑。每個樣本使用自己的時間換框架，再以框架
當下軸向投回畫面；Pe／Ap 使用相同轉換。雙體模式使用 orbit core 質心與旋轉軸。
相機在 map 中跟隨繪圖框架轉動，近地視角保持原本地表鎖定過渡。

天體軌跡使用已有 N 體星曆中的實際樣本，最多當下前後各一天；尚未算到的未來
不由 renderer 擅自延伸。這與舊 map 的封閉 osculating ellipse 不同，亦沒有把 orbit
lab 的全部歷史保留長度／預測長度設定搬入。以模擬時間每兩秒更新天體樣本，
切框架即更新；船路徑按 generation 快取，讀入較早時間時重建。

驗收：縮遠、輪流 1–4；3 的地表固定點保持固定，4 的選定雙體軸保持一致。
J／Shift+J 改參考對象，觀察路徑與 Pe／Ap 無舊框架殘留；F6／F7 與錄放後
恢復同一選擇。切換不應改位置、速度、資源、owner 或船隻控制。

本次驗證：model 18 工作區，app library 24、fleet-flight presentation 9、orbit
reference_frames 4、view 原檢查 8 與新增固定地表／cache rewind 1 通過。上述
crates lib/tests Clippy -D warnings、fmt、diff check 通過。TigerVNC 操作四種模式、
J、縮放、續跑、F6/F7 與錄製，最終 journal `/tmp/void-plot-gui.jsonl` headless
verify 通過；當時未跑全 workspace，人類驗收尚未完成。

2026-10-07：使用者確認包含本項的三項主遊戲整合已完成人類驗收，對應最終
整合 master `f06c283`（model 20）。此確認不新增測試或擴大既有驗證範圍；尚未 push。
