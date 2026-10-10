# 測試用的假世界和假船移出正式程式碼

branch：`cleanup/test-fixtures`

## 目標

正式程式碼只留遊戲實際用到的東西，主世界只在 `main_game` 定義一次。測試要用的假星球、假星系、假船、固定場景，都放在只有測試看得到的地方。

## 要做的事

1. **主世界只定義一次**
   - `main_game` 直接從 Sol 星系資料建 Aurelia（Layered 地形和海），不再先建 `aurelia()` 的丘陵地形再換掉。
   - `solar_scenery` 併進主世界的定義。其他星球的設定只有這一份，Selene 只留現在主遊戲用的隕石坑版本。
   - 遊戲本身不變：改動前後 `main_game` 產生的世界資料（序列化成 JSON）要完全一樣。

2. **新增只給測試用的 crate `crates/testkit`**
   - 只出現在各 crate 的 `[dev-dependencies]`，正式程式碼不依賴它。
   - 放進去的東西：
     - `void-landing` 的假星球：pebble、Luna、Terra、丘陵版 Aurelia、`planet_by_id`、`planet_environment`、`planet_ephemeris`。沒人用的 `aurelia_fast` 直接刪掉。
     - `void-multiscale/fixtures.rs` 的假星系。`default_galaxy` 如果遊戲本身要用，就留在正式程式碼。
     - `void-assembly` 只給測試用的船：`crewed_flight_rocket`、`crew_rover`、`rendezvous_pod`。
     - `void-vessels/sites.rs` 的 `flat_site`、`pod_tank`、`nearby_site`。
   - 只有單一 crate 的測試在用的東西，放在那個 crate 的 `tests/` 裡，不進 testkit：
     - fleet-flight 的 `aurelia_selene`、`stellar_neighborhood`、`daylight_terrain_site`；
     - terrain 的 `sunlit_*`；
     - assembly 的 `migrate_legacy_craft`。
   - 寫在 `src/` 裡、需要用到這些假資料的單元測試，搬到 `tests/`。

3. **只給測試用的 Action 和固定場景**
   - `Action` 裡只有測試在用的 `LaunchState`、`LaunchSplitState`、`LaunchFlightAt`、`LaunchGroundAt` 刪掉。
   - 同時刪掉固定 400 km 軌道的 `launch_orbital`，以及只有它在用的 `LaunchOrbit`、`LaunchOrbitAt`。這兩個 Action 要先確認遊戲本身沒在用。
   - 用到它們的測試，改成用遊戲本身的 `Place`／`PlaceNear` 設定船的狀態。有表達不了的情境就回報，不保留舊的 Action。
   - 錄影格式改了，所以 `FORMAT_VERSION` 要升。

## 不做的事

- `LandingPlanet` 這個型別本身留著，遊戲執行時還在用。
- 不改物理、畫面和存檔內容。

## 驗收

- 主世界 JSON 前後一致；`cargo check`、clippy 和測試都通過；`guides/workflow.md` 的未使用程式碼檢查沒有列出新的名稱。
- 使用者開遊戲：發射場、海、其他星球看起來和改動前一樣；F6／F7 存讀檔正常。
