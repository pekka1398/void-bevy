# 移植盤點（2026-10-02，歷史紀錄）

本頁保留搬遷階段的系統／缺口與當時 package 數。搬遷後 A／B 已完成 assembly／Fleet 主遊戲整合、直接存檔、durable replay、碰撞疊圖、渲染 profiling 與接縫 corpus；當前完成度以 [ab-progress.md](ab-progress.md) 及 [fleet-flight.md](fleet-flight.md) 為準。multiscale 仍為獨立核心／lab，已與 Fleet 在接縫檢查中共用。

範圍：以目前 TS `src/`、各 `lab/*/src` 和檢查腳本為參考，盤點 Rust 核心、可操作驗收場景、主遊戲及獨立專案的依賴。這是原始碼／API／場景接線對照加 headless 驗證，不是所有演算法逐行正確性的證明，也沒有代替使用者操作視窗驗收。

## 結論

Bevy/Rust/native Rapier 的可行性實驗已有足夠成果，可以成為主要開發專案。主要物理與渲染系統有 Rust 實作、數值回歸資料及獨立場景，TS 主遊戲的主要流程也已移植。但是不能宣稱所有 TS lab 的功能與驗收工具都完整搬完：此次補齊 orbit 雙體旋轉框架與獨立操作入口；其他驗收工具漏項仍列於下方。

TS 主遊戲沒有整合 assembly、aerodynamics、vessels、multiscale。Rust 同樣保持這些 lab 獨立，符合參考版，不是漏移植；不需要為了獨立專案而先整合它們。

（搬遷後的新增：aero 已接入主遊戲，行星有大氣會對火箭施力。這是刻意超出 TS 的範圍，不屬於移植盤點，見 [game.md](game.md)。assembly、vessels、multiscale 仍獨立。）

## 已有的系統

workspace 共 21 個 package：15 個功能／基礎 crate、`void-app` 及 5 個獨立 lab 程式。

| TS 範圍 | Rust 現況 | 驗收入口與界線 |
| --- | --- | --- |
| orbit | Kepler、系統資料、自轉／潮汐鎖定、Yoshida N 體星曆、Hermite、Dopri5、J2、船推進／撞擊、dominance、apsides、有限燃燒計畫、Simulation | 核心有 golden tests；另有 `void-orbit-lab` 的完整操作入口，見 orbit-lab.md |
| frames | 樹狀框架、位置／速度／姿態轉換、共同祖先、body inertial／surface、固定與自由框架 | `system` example；雙體旋轉繪圖框架由 void-orbit 提供 |
| rotation | 旋轉框架中的姿態與慣量積分 | crate 測試，landing／sas 使用 |
| lod | tile 幾何、四分樹選擇、接縫、鄰居平衡、剔除／快取、三種原 lab 地形 preset 資料 | `lod` example；Bevy compute pool 非同步建 tile，沒有原瀏覽器 benchmark 等價工具 |
| terrain | hills、layered、surface contract、依 cell size 過濾地形細節 | 數值／tile 對照，供繪圖與碰撞共用 |
| landing | PlanetFrame、ContactWorld、Lander、兩級 PartJointRocket、接觸／軌道交接、分離、毀損判定、rails、coast forecast | `landing` example；驗收框架／疊圖少於 TS 頁面 |
| scenery | 大氣 LUT、多重散射、地表／海洋、體積雲、星空、OrbitView、三種 terrain、ACES／AgX／Neutral | `scenery` example；GPU shader 的外觀仍需使用者驗收，CPU golden 不涵蓋完整 GPU 畫面 |
| sas | pilot override、停止旋轉／鎖定姿態、調參 | `sas` example 和數值／行為檢查 |
| navball | 幾何、sky／ground、grid、markers、RGBA painter | `navball` example；保留 TS 的 KSP 鏡像方向，已註解，非此次移植遺漏 |
| view | single／split、map fade、camera spin、inertial／surface path frame、軌道線、apsides／labels | `view` example；其除錯疊圖／session log 未搬到該 example |
| assembly | catalog、compile、接點／接合樹、供油圖、分級、JSON、動態質量／慣量、平地 Rapier 試飛、原模型資料 | `void-assembly-lab`；維持 TS 原有的有限組裝範圍 |
| aerodynamics | 大氣、body／wing forces、stall、controls、shield shadow、熱／燒蝕、loads、aircraft、entry | `void-aero-lab` 的風洞／飛機／再入三場景；未接主遊戲符合 TS |
| vessels | Fleet、orbit／bubble／ground 所有權、各船控制／SAS、供油、分離／合併、交會、rails | `void-vessels-lab` 六场景；部分觀察工具缺少，一項原始門檻未通過 |
| multiscale | split positions、多系統耦合 N 體、Traveller 自動／手動換框架、FrameEphemeris、遠方兩船接觸／join | `multiscale` example 加 `void-multiscale-lab`；新 encounter 的視角功能少於原共用 WorldView |
| flight／TS 主遊戲 | 兩級火箭、Sol／單行星、terrain/scenery、navball/SAS、warp、flight→map、滑行／機動預測與自動燃燒、除錯疊圖與 session log | `cargo run -p void-app`；仍使用 PartJointRocket，和 TS 一樣 |

## 確認的移植缺口（前兩項本次已補齊）

1. **已補齊：orbit 的雙體旋轉框架。** `void-orbit::FrameEvaluator` 已實作四種框架，並通過 TS golden 與原 60 天離軸檢查。以下保留盤點時的原因： TS `lab/orbit/src/orbit/ReferenceFrames.ts` 有 `two-body-rotating`：雙體質心原點、沿連線的 X 軸、軌道角動量的 Z 軸及瞬時旋轉週期。`lab/orbit/orbit-check.ts` 也實際驗證兩天體留在 X 軸上。Rust `frames/src/tree.rs` 沒有對應 Kind／建構 API，`view/src/path_frame.rs` 只有 inertial／surface。該項現已補入 orbit crate，frames 文件也已更新。
2. **已補齊：完整 orbit lab 操作入口。** 新的 `void-orbit-lab` 提供下列操作；SceneView 路徑／漸暗顏色直接對照原 TS 輸出，視窗仍待使用者驗收。以下保留原盤點： Simulation／FlightPlan 的相關 Rust API 存在，但沒有原 `lab/orbit/src/app/main.ts`／`SceneView.ts` 的等價程式，供使用者選 Sol／binary、四種 plotting frame、初始軌道平面／導航參考、history／prediction／plan coast 範圍、計畫目標，並查看目標路徑／計畫末端標記。主遊戲有機動編輯按鍵，不代表此獨立 lab 已完整移植。
3. **已補齊實作，待視窗驗收：landing 的驗收能力。** `app/examples/landing.rs` 已提供 C 切換 surface／inertial camera frame、F2（或 B）地形線框、F3 tile boundaries、F4 實際碰撞地形與火箭 collider 線、F5 地形顯示。HUD 顯示傾角、睡眠、框架、碰撞 tile 與近期交接事件。碰撞線讀 Rapier 已載入網格；外觀與操作仍由使用者驗收。

4. **LOD 的效能驗收工具。** 原 `BrowserBench.ts` 有 scripted rendered scenarios、settle/run/drain、p50/p95、CPU/GPU timings、draw calls／triangles 及 worker build 統計。Rust golden 重播 10,400 幀證明選擇結果一致，不等於重做這套真實渲染 benchmark。`lod` example 固定 landing preset，沒有原頁面的 preset 選擇／完整參數調整；DemoTerrain 資料已搬。
5. **vessels 的觀察工具。** TS `FleetView.ts` 的歷史路徑與 `main.ts` 的 planet-view 捷徑在 Rust lab 中未提供；TS 的 collision-terrain wireframe 開關也不是 Rust 現有的零件 collider 疊圖。原始碼沒有對應的 history trail 管理。物理 API 與六場景不因此缺失。
6. **multiscale encounter 的 WorldView 能力。** TS 兩場景共用可點天體／船焦點和軌道線的 WorldView；Rust interstellar example 有標籤與軌道，獨立 encounter 程式只有四個固定焦點／縮放預設，沒有等價的任意天體／第二船選取和軌道／可點標籤。遠方碰撞數值測試通過不代表這些觀察功能也移植了。
7. **主遊戲的 scenery 操作。** TS `src/main.ts` 的 atmosphere/cloud/ocean/star switches 與 exposure 在 Rust 主遊戲尚無操作入口；scenery example 中可操作。planet／terrain 可用 CLI，其他多數表單已改為按鍵，這些有替代操作的部分不列為功能遺漏。

`view` example 的 wireframe／tile boundaries／session log 也尚未搬入該場景；主遊戲已有相關實作。UI 排版、CSS、slider 換按鍵不作為完成度問題，只有操作能力實際不存在時才列缺口。

## 測試狀態與已知問題

- 補缺後 `cargo test --workspace --all-targets`：**196 passed、0 failed、1 ignored**；workspace clippy（`-D warnings`）及新 lab build 通過。通過代表既有測試涵蓋範圍通過，不能推出沒有漏功能。
- ignored 為 `vessels/tests/checks.rs` 的 Pebble 細長兩級火箭靜止傾角，已結案為非缺陷：發射點地形有坡，細長火箭順坡傾倒是預期行為，原 3° 門檻量到的是場地坡度。詳見 vessels.md。
- 軌道／scenery 等多項有 TS golden，aero 和 vessels 也有原 lab 行為檢查；**orbit 的整個 `orbit-check.ts` 並沒有一對一完整覆蓋**，雙體框架原檢查已在本次補上。golden 的輸入選擇不能代替功能清單。
- GPU shader、字型、選取與拖曳、不同 GPU／平台、長時間資源使用仍需相應驗收；本次未操作 GUI。
- native 與 WASM 接觸軌跡有差異，不能以「原生不逐位元相同」一概豁免原來的行為門檻。

## 原本就沒做，不能算移植遺漏

參考 TS 主遊戲尚無 assembly／aero／Fleet／multiscale 整合；也沒有真正 docking capture／RCS、完整存讀飛行世界、EVA、車輛、浮力、所有天體可著陸地形、完整資源／電力系統、銀河質量模型或相對論。原 assembly lab 沒有表面接合／對稱／自由移位／結構彎曲。scenery 的雲仍是靜態且沒有地表雲影。Rust 未新增這些能力不表示移植失敗。

## 能否獨立

此處保留搬遷前的檢查紀錄。獨立 repository 已建立，開發規則／ignore／參考重產入口已補齊，實際搬遷與驗證見 [migration.md](migration.md)。

**可獨立編譯與繼續開發。** 本次把 `git ls-files lab/void-bevy` 列出的檔案複製到 `/tmp/void-bevy-independence-*`，不帶 TS、vendor 或原根目錄設定：

- 先前 `cargo metadata --offline --no-deps`：20 packages（新增 orbit-lab 後為 21），全部 local path dependency 在新 workspace 內。
- `cargo check --offline --workspace --all-targets` 通過（共用原 Cargo target 快取；不是全新下載／從零建置測試）。
- catalog、visuals、system presets、golden JSON／bin、WGSL 都在已追蹤檔案內。Rust 不需要執行 TS；Bevy／Rapier 來自 crates.io，vendor checkout 是讀原始碼的參考。
- `golden/*.ts` 仍 import 舊 TS labs：既有 Rust tests 可直接讀已存資料，但要**重新產生 TS 對照**仍需要舊專案與其 Node 依賴。

獨立前應補齊：本專案的開發規則（目前在外層 NOTE.md）、來源版本／commit 和缺口清單、root ignore 規則（新的 workspace 要自行忽略 lab-log）、golden 重新產生的參考路徑與說明。純 Rust 新功能仍維持 core crate + lab app/example + headless tests + 使用者驗收的流程。

建議把新專案設為開發主線，舊專案改為唯讀參考封存。封存包含 Git 歷史、當前未提交 NOTE.md、TS sources／checks／設定，以及需要保留的參考資料；排除可重建的 target／node_modules。保留 zip 的同時保留 Git bundle 或等價歷史備份，搬遷不需要等待四個獨立 lab 整合。

雙體框架與 orbit lab 入口已處理；仍須整理其他驗收漏項，才能宣告「TS 功能移植收尾完成」。獨立專案可以先進行，但封存不應讓原參考實作與對照工具變得不可取得。
