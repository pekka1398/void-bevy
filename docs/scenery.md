# scenery：大氣、雲、星空（CPU 部分）

`crates/scenery`（`void-scenery`）移植 lab/scenery 在 CPU 上做的事。著色器（地面、大氣傳輸、雲、合成）之後在 `void-app` 用 WGSL 移植，以肉眼對照 TS。

| TS | Rust |
| --- | --- |
| `src/Atmosphere.ts` | `atmosphere.rs`：Earth 類大氣參數、密度與消光、穿透率表（256×64）、`sky_radiance` 參考積分 |
| `src/SkyTables.ts` | `tables.rs`：多重散射表 Ψ（32×32）、天空輻照度表（32×16）、`march_sky`（著色器的 CPU 版）、雙線性查表 |
| `src/CloudField.ts` | `clouds.rs`：天氣圖（2048×1024 RGBA8，多執行緒）、形狀與細節噪聲體積（64³、32³）、`cloud_density`、雲殼區間 |
| `src/Stars.ts` | `stars.rs`：14,000 顆星的方向與顏色（mulberry32，同一個種子） |
| `src/OrbitView.ts` | `orbit_view.rs`：從地面到 200,000 km 的軌道視角 |
| `src/LayeredTerrain.ts` | 已在 `void-terrain` |

表格以 `f32` RGBA 輸出（和 TS 的 `Float32Array` 一樣），噪聲與天氣圖是 RGBA8。

## 檢查（`cargo test -p void-scenery`，對照資料由 `golden/scenery.ts` 產生）

| 檢查 | 結果 |
| --- | --- |
| 穿透率表、多重散射表、輻照度表（全部 texel） | **差 0** |
| `sky_radiance`（500 步）、`march_sky`（32 步，有／無多重散射），5 條光線 | **差 0** |
| 天氣圖（每 16 列加最後一列；整張 8 MB 太大不放進 repo） | **差 0 byte** |
| 形狀、細節噪聲體積（整個） | **差 0 byte** |
| 星空位置與顏色 | **差 0** |
| `cloud_weather` 300 個方向、`cloud_density` 300 組、雲殼區間 200 條 | 1e-16 內 |
| 軌道視角 9 個操作後的姿態 | 1e-15 內 |

不完全為 0 的只有用到 `sin`、`cos` 的地方（V8 的這兩個和 fdlibm 不同，見 terrain.md）。

在 dev profile 下，整個測試（建三張表、整張天氣圖、兩個噪聲體積，加上比對）約 0.3 s；TS 在瀏覽器裡建表約 0.6 s、天氣與噪聲約 1.5 s。
