# assembly：零件組裝與本地試飛

`crates/assembly`（`void-assembly`）移植 TS `lab/assembly` 的資料模型與試飛物理，不依賴 Bevy。獨立的 `crates/assembly-lab`（`void-assembly-lab`）提供 Bevy 編輯器與試飛畫面，不依賴 `void-app`。主遊戲 `void-app` 依賴 assembly：船是 assembly craft（預設 `data/flight-rocket.json`，可用 `--craft` 指定匯出的 JSON），零件外觀共用 `void-assembly-lab` 的 `RenderAssets`，見 [fleet-flight.md](fleet-flight.md)。

從新 workspace 根目錄執行：

```sh
cargo run -p void-assembly-lab
cargo run -p void-assembly-lab -- --craft /path/to/void-craft.json
cargo test -p void-assembly -p void-assembly-lab
```

## 編輯器

- 預設載入兩級範例。左側 New 建立只有指令艙的船，Demo 回到範例。
- 在左側選零件，點模型上的綠色空接點接合。Switch new part's node 切換新零件使用的 top／bottom，方向與位置由成對接點決定。Cancel 或 Escape 取消。
- 點船體選取零件。右側調整燃料（每次容量的 10%）、執行級數，或移除該零件及其子樹。根指令艙不可刪除；New 可以清空。
- 級數由小到大執行，允許不連續的數字。同級先分離，再點火；Stage -1 可把 0 改為 unset。沒有級數的引擎／分離器會阻止試飛。
- 名稱與 JSON FILE PATH 可直接編輯。Export 寫入指定的新檔案；已有檔案會顯示錯誤，改用另一個路徑即可。Load 先驗證完整資料，再替換現有組裝。Craft JSON 沿用 TS 的 version 1、camelCase 格式，可互相匯入。
- 左鍵拖曳環繞、右鍵拖曳平移、滾輪縮放。F 置中；C 切換重心標示。拖曳超過 5 px 不當作選取／接合。

## 試飛

Launch 使用當下的自訂組裝建立 Rapier compound bodies，不是另一份固定火箭。Space／Next stage 點火或分離，Shift／Ctrl 調整節流，X 關閉；W/S、A/D、Q/E 提供測試轉向力矩。P 暫停，Return 回到試飛前的原始組裝。失去視窗焦點時不推進物理；文字輸入時不處理飛行快捷鍵。

兩級範例：Space 點燃下級，升空後 X 關油門，再 Space 分離並點燃上級，Shift 恢復節流。分離後兩個群都繼續模擬，質量、燃料和重心隨耗油更新。沿用原 lab 的行為：已點火的拋棄級仍共用節流，暫停時也可以分級。

畫面使用 TS `PartVisual.ts` 的模型資料：引擎的短上殼與擴口噴嘴、燃料箱的兩道環帶、指令艙的艙窗、各部件材質，以及原本的選取輪廓線和火焰。地面格線、發射台圈、霧、暖光與冷色邊光也用原場景的配置；Bevy 與 Three 的光照實作不同，畫面仍由使用者對照驗收。

這個獨立試飛場是平地、固定重力；没有軌道、行星 scenery、氣動或 SAS。物理沿用原 assembly lab 的 1/60 s Rapier 步進及 0.7 角阻尼；此處的轉向和阻尼是測試範圍的行為，未引入 landing 的旋轉座標接觸積分。

## 外觀與碰撞

兩者分開。`assembly-lab/src/parts.rs` 根據 `data/visuals.json` 建立繪圖 mesh；資料由 `golden/assembly_visuals.ts` 呼叫原 TS `createPartVisual` 後匯出，包含實際尺寸、局部位置、旋轉、材質及 `EdgesGeometry(25)` 的線段。

組裝編輯時以接點推導位置，尚未做零件重疊／碰撞檢查。進入試飛後，`void-assembly` 的 `runtime.rs` 才建立 Rapier compound body，每個零件各有一個簡化 collider，與原 TS lab 相同：

| 零件 | 繪圖 | Collider |
| --- | --- | --- |
| 指令艙 | 圓錐與艙窗 | 單一圓錐 |
| 燃料箱 | 圓柱與兩道環帶 | 單一圓柱 |
| 引擎 | 短上殼與擴口噴嘴 | 原零件完整高度的單一圓柱 |
| 分離器 | 圓柱 | 單一圓柱 |

不是 triangle-mesh collider。外觀細節不會增加質量、慣量或碰撞形狀；分離之後每個物理群保留其零件 colliders。

## 核心與驗證

| TS | Rust |
| --- | --- |
| `model.ts` | `model.rs`：零件／模組／資源、接合樹、編譯姿態、空接點、子樹、連通群、crossfeed、分離接點、摘要、JSON |
| `runtime.ts` | `runtime.rs`：Rapier 本地世界、各群剛體與各零件 collider、燃料／慣量更新、Isp、分離與分級 |
| `main.ts` | 獨立 `void-assembly-lab` 的 Bevy 操作與繪圖 |
| （無） | `graph.rs`：飛行中的零件圖 `PartGraph`，每個零件帶自己的燃料、分級、點火與 pose；分離、對接是它的 `disconnect`／`connect`。Fleet 用它，assembly lab 的 `AssemblyFlight` 不用（[part-graph.md](part-graph.md)） |

`crossfeed_tanks(&[CrossfeedPart { id, definition }], &connections, engine_id)` 是獨立供油查詢 API，不需要 `Craft` 或 `CompiledCraft`，可處理任意連接圖（包含環路）。只穿過兩端都允許 crossfeed 的連接，回傳燃料箱 ID，順序沿用輸入零件順序；外部零件的連接會略過，呼叫端在分離後傳入仍有效的連接。重複 ID 或無效引擎會 panic。`CompiledCraft::fuel_sources()` 過濾已切斷連接後共用此 API。獨立圖測試涵蓋環路、阻斷、外部端點、分離與無效輸入。

原本的六種零件（pod、tank-small／large、engine-small／large、decoupler）的 authored data 直接從 TS catalog 匯出到 `data/catalog.json`。對照資料由 `golden/assembly.ts` 產生，沒有在 Rust 手抄一份不同的火箭參數。

主遊戲整合時在同一 catalog 新增 11 個 `flight-*` 零件（指令艙、上級箱／引擎、分離器、助推箱／引擎、斜撐、四個腳墊），組成主遊戲預設船 `data/flight-rocket.json`（7620 kg、理想 Δv 9.6 km/s，共 14 個零件）。原六種零件的數值與 golden fixture 不變；catalog 改變後，舊 catalog 的存檔／錄影會被明確拒絕。詳見 [fleet-flight.md](fleet-flight.md)。

引擎模組的 `nozzleExitAreaM2`（噴嘴出口面積，主遊戲的背壓用）每個引擎都必須寫：engine-large、flight-booster-engine 0.12 m²，engine-small、flight-upper-engine 0.15 m²。TS catalog 沒有這個欄位，是零件圖 branch 從 fleet-flight 的對照表搬來的；assembly lab 的試飛不讀它。

```sh
# repo 根目錄
python3 tools/regenerate-golden.py --reference-root ../void assembly
python3 tools/regenerate-golden.py --reference-root ../void assembly_visuals
```

11 項核心測試涵蓋 TS 姿態／質量／空接點／供油對照、反向接合與不同 parts 順序、錯誤資料拒絕、子樹、真正連通群、切斷分離器的子邊、pad 接觸、起飛與 Isp 耗油、旋轉分離的位置／線動量／角動量、燃料耗盡、自訂 JSON 船、無效試飛與不同級數。另有 4 項編輯器檢查：從空船組裝至試飛再返回、無效操作不替換現有船、實際檔案匯出／載入與錯誤檔案拒絕，以及 Bevy 系統初始化沒有存取衝突。

原 assembly lab 的範圍維持不變：堆疊接點與單一推進劑，質心在零件原點，慣量用外接圓柱近似；尚無表面接合、對稱、自由位移、結構彎曲／破壞或完整供油優先序。非預期狀態 panic；使用者的資料與操作錯誤顯示原因。

程式測試與建置通過，Bevy 畫面與操作由使用者在本機驗收。驗收時建議先看範例的分級，再從 New 自己組一艘船、匯出／匯入並試飛；另外比較反向接合、空燃料與 unset 級數。
