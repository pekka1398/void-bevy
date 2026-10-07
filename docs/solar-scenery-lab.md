# Solar scenery lab（第一輪，待使用者驗收）

本輪只新增獨立 solar lab，主遊戲預設仍 Aurelia。世界 schema **3**、分支 MODEL_VERSION **14**；舊格式明確拒絕，沒有遷移或默認 recipe。`InitialWorld` 保存天體配置，checkpoint／journal 保存固定視角命令。

```sh
cargo run -p void-app --example solar_scenery -j 2 -- --body halo --view orbit
cargo run -p void-app --example solar_scenery -j 2 -- --body selene --view near
cargo run -p void-app --example solar_scenery -j 2 -- --body sol --view orbit --exposure 0.1
cargo run -p void-app --example solar_scenery -j 2 -- --export-world /tmp/solar-world.json
cargo run -p void-app --example solar_scenery -j 2 -- --world /tmp/solar-world.json --body vesper --view orbit
```

可選 ID：sol、cinder、vesper、aurelia、ares、velvet、halo、azure、abyss、selene。未知 ID／未配置 ID 直接拒絕；其他衛星仍是既有遠景 map sphere，未宣稱已完成。`--view near|orbit|far` 是固定相機；`--exposure` 為正有限 HDR 曝光倍數，預設沿用既有 lab 的 6.309573，只有最後 resolve tone-map 一次。近景為半徑 1.025 倍的區域俯視，並非地面行走／雲內飛行相機。Halo 軌道視角使用 6 倍半徑以容納環，其他天體 3.5 倍，遠景 12 倍。

`[`／`]` 循環十顆配置天體；`7`／`8`／`9` 切近景／軌道／遠景。拖曳／滾輪自由觀測。`Home` 回船，`Tab` 切船，原 multi_body lab 的其餘控制保留。`F6`／`F7` 存讀（`--save /tmp/solar-save.json` 可指定路徑）；`R` 重設到 CLI 配置。`--record /tmp/solar.jsonl`、`F8` 停錄，`--verify /tmp/solar.jsonl` headless 重放。切觀測天體只送 ViewCommand，不移動船、不新增碰撞球。

## 每顆第一輪外觀

| 天體 | 已接入 recipe | 明確限制 |
|---|---|---|
| Aurelia | 保留 layered 地形、海、Earth optical air／volume clouds | 未變更原有美術 |
| Selene | 隨 seed 分布的撞擊盆地／噴出環、灰色地表、無大氣 | 無真實月海分區或多年代侵蝕 |
| Cinder | 更密小型坑、低起伏與暖灰岩石 | 無真實水星地貌資料 |
| Ares | 稀疏大型盆地、較高粗糙地形、紅岩與薄塵光學 | 無峽谷／盾狀火山／極冠；不宣稱完整火星地貌 |
| Vesper | 稀疏盆地粗糙岩石、自訂黃色散射、45–70 km 濃雲 | 共用既有 volume cloud noise；未做超旋轉天氣 |
| Velvet | 緯向多雲帶、噪聲扭曲、局部橢圓風暴色場 | 靜態球殼頂層，不是可進入的氣體體積 |
| Halo | 較細平緩雲帶、獨立雙面環幾何與環帶／縫隙透明度 | 無環陰影、粒子或體積光學 |
| Azure | 低對比青色寬帶、弱擾動 | 靜態球殼頂層 |
| Abyss | 深藍雲帶、較強擾動與局部風暴 | 靜態球殼頂層 |
| Sol | 高於 1 的 HDR 發光、顆粒噪聲、透明發光日冕殼 | 風格化日冕；高曝光表面飽和，可用 0.1 檢查顆粒。太陽照明仍沿用既有 directional light |

這是第一輪可辨識外觀與可擴充入口，不是最終行星美術驗收。gas/star 使用 `SurfaceRecipe`，不建立 terrain／sea／GroundSpec。所有岩石使用 `TerrainConfig::Cratered`，地表 tiles、遠景 mesh 和 Rapier collider 都用同一取樣器；無 shader-only 地形位移。地形沒有負高度：盆地是在共同基底高度中挖低，保持 terrain core 的既有高度合約。

## 配置／接口

`world::solar_scenery` 是唯一預設配置入口；`VisualSettings.surface` 與 `rings` 明確序列化。`void-scenery::solar` 驗證且取樣氣態／恆星色場；`void-terrain::CrateredOptions` 控制 seed、坑數、角半徑、粗糙度、顏色與高度。gas/star 不接受固體 terrain。rings 半徑以 body radius 的倍數指定，僅視覺。

光學與物理 presence 已分離：Vesper／Ares 的散射不會製造 `EarthAtmosphere` 密度、壓力或 aero 力。Aurelia 的物理大氣保持原設定。使用者編輯 JSON 後重新開啟；本輪沒有 runtime shader／physics 熱切換。

球殼雲帶目前在 256×128 球網格上取樣顏色；近景有限解析度，尚非 per-pixel shader。日冕殼與環明確登記在 FarBody ownership 中，世界重建逐個移除 mesh/material；tile task generation 沿用 multi_body 的重建淘汰規則。

## 驗收清單

1. 每顆在 near／orbit／far 觀察、拖曳至背光面；檢查岩石坑形、gas 雲帶、Halo 環、Sol 發光與曝光。
2. 特別檢查 Vesper 厚雲與 Ares 薄大氣；切到 Selene/Cinder 確認無大氣。Aurelia 海／雲不退化。
3. 循環全部天體兩輪，觀察 HUD 資產數；回同天體／同視角後應回到同一合理範圍（LOD pending 歸零後比較）。gas/star 的 active terrain meshes 必須為 0。
4. F6 → 切天體／視角 → F7，確認配置與焦點還原；重複 R／F7、切焦點時不出現舊 tile。
5. F8 停錄後以 `--verify` 核對 journal；以 `--verify-save` 核對 checkpoint。

GPU 驗收不能被 headless 通過代替；本輪 agent TigerVNC 畫面檢查紀錄另列，最後使用者驗收仍待完成。

## 本輪 agent 檢查（2026-10-05）

- `cargo test -p void-fleet-flight -p void-scenery -p void-terrain -j 2`：87 passed，0 failed；包含原 golden、舊多天體場景、十顆固定視角／physics 不變、checkpoint／replay、五個 solid collider 頂點、光學 LUT 真空／極端有限性。最後調整 crater 分布後追加相關 6 tests 再通過。
- 三個 core crates `clippy --all-targets -j 2 -- -D warnings` 通過。
- 自 scenery worktree 編譯 `solar_scenery`（共用主 repo target 僅做 cache），獨立 final binary `/tmp/void-solar-scenery-model14-final`。TigerVNC 私有 display `:10`、NVIDIA RTX 5060 Laptop／Vulkan 實跑，無 shader validation／panic。不是 headless 截圖替代。
- 十顆 × 三固定視角：`/tmp/void-solar-evidence/{body}-{near|orbit|far}.png`。相機近景目前正向下俯視，某些位置只見局部均勻地面／厚雲，需後续專用地平線相機改善；不可把此輪當成地面美術完成。
- F6 → 切焦點 → F7 三次後，Halo 穩定 `102 app meshes / 104 materials / 30 textures`，無 active terrain meshes；切 solid 視角增加 LOD meshes，回 gas 視角回到此基線。`/tmp/void-solar-halo-v2-load.png` 記錄 generation 6。
- 該實際 GUI journal `/tmp/void-solar-recording-v2.jsonl` 以獨立 binary `--verify` 通過；`/tmp/void-solar-save-v2.json` `--verify-save` 通過。
- 使用者最終視窗驗收、commit／push／主遊戲整合均未進行。
- `cargo test -p void-app --lib multi_body::tests -j 2`：5 passed；包含新增三輪 solar renderer mesh／material／LUT ownership 重建後全部回零測試，並保留兩大氣獨立 tables、無 terrain 大氣與 entity 替換檢查。
- `cargo clippy -p void-app --lib --example solar_scenery --example multi_body -j 2 -- -D warnings` 通過。`cargo fmt --all` 與 `git diff --check` 通過。
- Sol 低曝光實拍 `/tmp/void-solar-evidence/sol-low-exposure.png` 可看見 granulation；預設曝光保留高亮發光／日冕，沒有用 terrain 假冒恆星。

最後 HUD 參數讀數實拍：`/tmp/void-solar-evidence/final-ares-hud.png`，明示 radius／camera height／exposure／光學與物理 air presence。final binary 也再次通過上述 GUI journal 與 checkpoint verify。完整匯出配置 `/tmp/void-solar-world-final.json`。

### 共享 cache 污染排除與最終重建

先前 `/tmp/void-solar-final-build.log` 只列 `void-app` 重編；在發現其他分支的共享 target local artifact 沿用問題後，不能以那次 build 單獨证明全部核心來自 scenery 分支。因此從本 worktree 以 metadata 求出 `void-app` 的 **21 個本地傳遞依賴（含 app 本身）**，逐 package `cargo clean -p`，完整保留外部 Bevy／Rapier，再用 `-j 2` 重建 solar example。

- 清理證據：`/tmp/void-solar-pure-clean.log`，只列 21 個 `void-*` package。
- 重建證據：`/tmp/void-solar-pure-build.log`，21 個 package 全部列出 `/home/pekka/Desktop/void-bevy-scenery/` 來源；與 metadata 集合逐一比對相同。耗時 1m09s。
- 已覆寫 `/tmp/void-solar-scenery-model14-final`；SHA-256：`ecb0a4d2162c1249260c4789a5a7a5bda08a7c6cc0bfa76a16c9e49718a18959`（亦存 `/tmp/void-solar-pure-binary.sha256`）。
- 以此純分支 binary 再驗證先前實際 GUI journal／checkpoint，兩者通過：`/tmp/void-solar-pure-journal-verify.log`、`/tmp/void-solar-pure-checkpoint-verify.log`。
- 本次補驗沒有修改功能或重跑 GUI；前述截圖是先前 GUI 實拍，最終核心來源由完整本地重建日誌與重新 verify 補證。共享 cache 已釋放給下一任務。

### Root 審查修正：稳定 ID 快捷鍵

Root 修正舊 multi_body 快捷鍵以 terrain map 順序選「第二顆」的假設：`1` 固定 home、`2` 固定配置且有 terrain 的 `selene`；L／I 明確在 Selene 建立 2 km／20 km AGL 下降 fixture。缺少 Selene terrain 時顯示不可用原因，不會改選其他天體。Root 已跑擴充後六項 multi_body unit tests 通過。

- 本輪 targeted app lib＋solar/multi_body examples Clippy `-D warnings` 通過；最終 example build 39.20s。證據 `/tmp/void-solar-shortcuts-clippy.log`、`/tmp/void-solar-shortcuts-build.log`。
- 新 binary 已覆寫 `/tmp/void-solar-scenery-model14-final`，取代上一節 SHA；目前 SHA-256：`cad86d8bc7fe200efba873c5f8568b26b370aa3915d09679d790e001a018bf8b`（`/tmp/void-solar-shortcuts-binary.sha256`）。本次僅 root 的 app shortcut 修正需重編；沿用已驗證的純 scenery 本地依賴。
- TigerVNC :10 由 Ares 起始，實際操作 `2 → L → 2 → I → 2 → F6 → F8`；Digit2 三次均為 Selene。L 顯示 selected v3／AGL 2000m on selene，I 顯示 selected v4／AGL 20000m on selene；無 panic／shader error。
- 實拍 `/tmp/void-solar-evidence/shortcut-digit2.png`、`shortcut-L.png`、`shortcut-I.png`（另含 after-L／after-I 截圖）。
- 新實錄 `/tmp/void-solar-shortcuts.jsonl` 與 checkpoint `/tmp/void-solar-shortcuts-save.json` 均以新 final binary headless verify 通過：`/tmp/void-solar-shortcuts-verify.log`、`/tmp/void-solar-shortcuts-save-verify.log`。
- 本次私有 GUI 與 TigerVNC 已關閉；共享 cache 在 build/copy 後已立即釋放。
