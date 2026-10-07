# 四功能整合版驗收

工作區 `void-bevy-integration`，分支 `work/four-feature-integration`。四個功能分支
保留，沒有合入或 push 到 master；人類最終驗收待進行。共同範圍見
[共同規格](specs/playable-vehicles-and-multiscale.md)。

## 執行

在這個工作區執行以下入口。`target/acceptance/void-app` 是 root 協調編譯的
組合版本；`manifest.json` 記錄實際程式來源 commit、版本和 SHA256，啟動腳本
先核對二進位摘要。它不是任一功能分支自己的舊快取。

```sh
./scripts/run-next-features.sh rover
./scripts/run-next-features.sh eva-space
./scripts/run-next-features.sh aircraft
./scripts/run-next-features.sh water
./scripts/run-next-features.sh stars
```

每次只開一個場景。rover／aircraft／water 按主遊戲預設開始運行；stars fixture／
eva-space 預設暫停，先核對 HUD 再按 P 切換。可追加 `--record /absolute/new-journal.jsonl --save
/absolute/checkpoint.json`；錄影路徑必須尚不存在。P 暫停／開始，F6 存檔、暫停後
F7 讀檔，F8 結束錄製。F4 顯示實際碰撞體；滑鼠拖曳環繞、滾輪縮放。PrintScreen
保存 GPU 圖像至 `lab-log/screenshots/`；桌面攔截此鍵時用 Ctrl+Shift+PrintScreen。

重新編譯時明確使用這個工作區，不同分支不得共用未核對來源的驗收二進位：

```sh
cargo build -p void-app --bin void-app -j 2
mkdir -p target/acceptance
cp target/debug/void-app target/acceptance/void-app
(cd target/acceptance && sha256sum void-app > SHA256SUMS)
```

自行編譯後應另更新 manifest 的來源 commit／SHA256；root 原驗證記錄不自動
適用於後續修改。被保存的 executable 是 Linux native，使用目前主機的 GPU／系統庫。

## 操作與觀察

### EVA 與 rover

`rover` 使用有乘員的普通 PartGraph 車。P 開始，W/S 前後、A/D 轉向；Space
鎖住煞車，W/S 解除，X 切停車煞車。核對四輪支撐、轉向輪外觀、實際位移與煞停。
先用短促 W 低速練習，約13m/s時急打滿方向會抬輪／翻覆；沒有隱藏穩定器或自動扶正。
F 出座，W/S 步行、A/D 側移、Q/E 轉身，Space 跳躍；靠近空座位按 F 回座。
轉動視角可避免人物被車身／座椅遮住。出座後乘員質量與五公斤背包燃料跟隨人物；
回座後剩餘燃料回到座位的隔離資源容器。

`eva-space` 是明示的近軌道交會起始配置，使用
[有乘員火箭](../crates/assembly/data/crewed-rocket.json)。外側座椅是本輪的載人
fixture，不是艙內／梯子模擬。F 出座，H 背包；Alt+W/S 前後、D/A 右左、E/Q 上下，
WASD/QE 有限旋轉推進。觀察人物／船的相對移動、燃料减少、接近後 F 回座。
步行控制在失去接地時不提供空中移動。攀爬、艙內行走、游泳未做；rover 未做電力。

### 飛機

`aircraft` 是明示的近乎平坦 Terra runway 世界，地形配置保存在同一 world，
绘图与碰撞共用。Space 點火，Shift/Ctrl 油門，X 切油門；W/S 俯仰、A/D 滾轉、
Q/E 方向舵與前輪，B 按住煞車。無理想 reaction-wheel SAS。

先滑行至約 45–50 m/s，用短促 W 抬頭約 8–12°，放開控制；長按可能過度旋轉／失速。
下降時調整油門，接地前短促 W 拉平；三輪接地後 X 切油門、B 煞停。觀察 AIR
速度／動壓／翼面迎角、姿態球與 gear 支撐數；低動壓時迎角讀值明示 N/A。

機翼、控制翼面、機身、jet、起落架使用共用 assembly 定義／幾何／慣量／接合與
鏡像接口。沒有新增 VAB 前端、收放起落架、襟翼、螺旋槳或結構破壞。

### 水

`water` 使用真實日側深海柱，將艙體明示放在海面上方約八米下落。P 開始，觀察
濺落、部分浸水與晃動／減速，HUD water N 來自同一物理求值器。AGL 仍是海底地形
距離，不是海面高度，因此可以顯示約 159 m。

浮力依浸水排水量與質量，不強制漂浮；低密度／高密度 headless 場景另檢查浮起與
下沉。水阻、偏心浮力力矩沿用同一 Fleet owner；沒有流體模擬、進水或游泳。

### 三星系

`stars` 明示建立 Sol 地面船、Beryl 地面船、Cygnus 軌道船；Tab 切換，普通火箭
點火／分級、RCS 与保存／錄放共用原操作。Ctrl+Home 看 16 ly 總覽，點恆星標籤
切焦點，Home 回船。N 是 home-site 地面初始船，O 在觀察天體建立軌道初始船。

遠方船是初始 fixture，不代表已完成星際飛行。核心連續邊界跨越、遠端地面／軌道／
Bubble 精度與力框架已檢查；沒有驗證完整 Sol 起飛到 Beryl 著陸的光年航程。
Bubble 保留 source system chart 並重定位 split anchor，Orbit 才依遲滯選最近星系。
沒有特別的星際引擎、傳送、相對論或銀河重力模型。

## 版本與驗證

組合版 Flight model 26、FleetCheckpoint 12、world schema 4、Craft 3；明示保留
未使用新幾何／狀態的既有 Craft 2。舊模型、缺必要欄位或不相容 checkpoint 直接拒絕，
沒有自動修復／轉換。位置／物理公共接口使用 f64；native Rapier 目前是 f32 ABI，
局部物理座標與浮動原點避免先轉遠方絕對 f32。沒有宣稱 native solver 是 f64。

root 受影響核心檢查：assembly、rotation、landing、vessels、modules、frames、
environment、multiscale、fleet-flight 的 lib/tests，345 passed、0 failed；一個已结案的
Pebble 傾角測試維持既有 ignored。app 最終28項（含2項實際方向測試）通過。
所屬核心與 app all-targets Clippy `-D warnings` 通過，fmt 通過；沒有跑全 workspace。
方向回歸以世界向量及真實氣動力矩檢查 W 抬頭、D 向玩家右方傾斜、E 右偏航，
另用 native 共享起落架滑行證明 E 向右轉，不依赖零件的 left/right 名稱。

GUI 與錄放證據在 [status](status.md) 另記錄來源 commit／觀察範圍，不能以測試數量
代替人類驗收。證據與執行檔摘要保存於 `target/acceptance/` 與外部 review archive。
