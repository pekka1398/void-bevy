# 清除舊 TS 遺留

## 目標

舊 TS 專案已經不是依據（AGENTS.md 第 12 條）。把為了跟它保持一致而留下的東西清掉，讓每件事只剩一種做法。

完成後主遊戲的行為不變：照樣能發射、入軌、對接、再入、降落、濺落，地形和天空的外觀也照舊。這次只清理，不加功能。

## 背景

2026-10-10 盤點時發現，多數核心 crate 都是從舊 TS 的各個 lab 搬過來的，而且附了 golden 測試，要求跟舊 TS 逐位元一致。為了讓 golden 繼續通過，每個 lab 的舊模型都原樣保留，新的統一做法（Fleet、座標樹）只能長在旁邊。結果同一件事出現好幾種做法。

## 範圍

### 1. 舊的飛行實作

主遊戲只用 Fleet（`void-vessels` + `void-fleet-flight`）飛船。下面這些是舊的另一套飛法，全部移除：

- `landing`：`Lander`、`PartJointRocket`（`rocket.rs`）、`DemoRocket`（`demo_rocket.rs`）
- `assembly`：`AssemblyFlight`（`runtime.rs`）
- `aero`：飛機和返回艙的獨立模擬（`flight.rs`、`entry.rs`，以及只為它們服務的 `vehicle.rs` 部分）
- `app`：`flight.rs` 中只為舊飛行服務的部分、`aero_field.rs` 的 `RocketAir`、舊錄放格式 `session.rs`
- `app/src/multi_body.rs`：獨立的雙天體驗收場景
- `vessels/src/scenarios.rs`：舊 TS lab 的六個場景（`create_lab_scene`）。其中主遊戲還在用的部分（例如預設船 `demo_craft`）搬到合適的地方
- 依賴以上東西的 example 和測試：`app/examples/{legacy_flight,landing,sas,multi_body}.rs`、`app/tests/{flight,air}.rs`、`landing/tests/` 相關檔案

**要注意的地方：** 主遊戲目前用 `demo_rocket(&terrain).launch_site` 決定發射場位置，`fleet_game.rs` 有 6 處、`tiles.rs` 和 `world_scenery.rs` 也有。移除 `DemoRocket` 前，要把「發射場在哪」變成主遊戲自己的設定，位置維持不變。

`fleet_game.rs` 裡用到 `flight.rs` 的 `game_planet_by_id` 這類函式時，要搬到合適的地方，不要整個刪掉。

### 2. 重複的型別和算法

- `landing::FrameState` 和 `void_frames::State` 完全一樣，合併成 `void_frames::State`。
- `multiscale::FramedState` 用字串指定框架，改成用 `FrameId`。
- `multiscale/world.rs` 自己重寫了一遍點質量重力再補 J2，改成直接用 `void_orbit::gravity`。

### 3. 模仿 V8 的東西

- `void-math`：整個 crate 都在模仿 V8 的 `hypot`、`tan`、`atan2` 等函式，換成 Rust 標準庫，然後刪掉這個 crate。目前有 13 個 crate 在用。
- golden 測試：刪掉 `crates/*/tests/golden/` 共 31 個資料檔，以及讀它們的測試。
- 註解和寫法：「as the lab's …」「bit-exact」「keep the lab's operation order」這類說明，以及為了一致而刻意保留的計算順序，一起清掉或改寫。

### 4. 保留的東西

這些看起來跟舊專案有關，但其實是遊戲本身在用的，保留：

- `orbit/systems/*.json`：太陽系的天體資料
- `landing` 的 `ContactWorld`、`ContactFrame`、`PlanetFrame`、`Coast`：Fleet 用它們做接觸物理和地面框架
- `aero` 的氣動力、熱、燒蝕：`modules` 在用
- `assembly-lab`：目前唯一的組裝編輯器，等遊戲內 Workshop 合入再說
- `ref/` 下的舊專案封存

## 測試

golden 刪掉後，軌道、座標、旋轉這些核心算法就沒有測試了。每個受影響的 crate 補幾個簡單的物理檢查，取代逐位元比對，例如：

- 二體軌道繞一圈回到原點，能量守恆
- 座標樹往上疊再往下拆，回到原值
- 旋轉框架中的物體，慣性角動量守恆

數量少、看得懂就好。測試只是讓 agent 確認沒改壞，不是交付成果（AGENTS.md 第 7 條）。

## 存檔

改用標準庫的數學函式後，最後幾位數會變。照現有規則升 model 版本，舊存檔直接拒絕，不做轉換。

## 完成條件

- 上面列出的舊實作、重複型別、`void-math`、golden 都已移除。
- 在 crate 開頭說明和註解中搜尋 `lab/`、`bit`、`V8`、`TS`，不再出現把舊專案當依據的說明。
- 我開主遊戲，用正常世界走一遍：發射、入軌、再入、降落，外觀和手感跟清理前一樣。

## 其他

- 從目前的 master 開 branch。其他還沒合併的 branch 不用管，也不用顧慮跟它們的衝突。
- 這份做完才做 [主遊戲只有一個入口](main-game-single-entry.md)。
