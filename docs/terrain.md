# terrain：行星地表

`crates/terrain`（`void-terrain`）移植：

| TS | Rust |
| --- | --- |
| `lab/landing/src/terrain/Surface.ts`、`TerrainConfig.ts` | `lib.rs`：`Terrain`、`TerrainConfig`（同一份 JSON：`{"kind": "layered" \| "hills", "options": {...}}`） |
| `lab/landing/src/terrain/SurfaceContract.ts` | `lib.rs`：`check_terrain_contract`、`lattice_directions` |
| `lab/scenery/src/LayeredTerrain.ts` | `layered.rs`、`noise.rs`（帶解析梯度的 gradient noise） |
| `lab/landing/src/terrain/HillsTerrain.ts` | `hills.rs`、`noise.rs`（Perlin） |

`Terrain` 實作 `void_lod::SurfaceSampler`，所以畫面 tile 和碰撞 tile 用同一個取樣器、同一個 `build_tile_mesh`：畫的就是撞的。點查詢（`Terrain::height`）用完整細節；tile 依自己的格距去掉更細的八度，和 TS 一樣。

`ground_cover` 的 `asin`、`exp` 用 `libm`（fdlibm），和 V8 相同。

## 檢查（`cargo test -p void-terrain`，對照資料由 `golden/terrain.ts` 產生）

| 檢查 | 結果 |
| --- | --- |
| layered（主遊戲的地形）：1,500 個方向 × 4 種格距（完整細節、30 m、1 km、60 km）的高度與顏色 | **差 0** |
| Pebble、Luna、Terra、Aurelia 的 hills：同上 | **差 0** |
| layered 上用 lod 建的 tile（L2、L9、L16）：原點、位置、法線、顏色、高度 | **差 0** |
| landing 的地形契約（非單位方向 panic、確定性、範圍、1 cm 內不跳 1 m），每種地形 20,000 個方向 | 全部成立 |
| `lattice_directions` | 1 ulp 內 |

`lattice_directions` 只用來產生測試方向，上面每個取樣都用 TS 寫出的方向。它用到 `sin`、`cos`：V8 的這兩個既不是 fdlibm（`libm` 在約 1% 的輸入上不同）也不是系統 glibc（約 3%），所以只在 1 ulp 內一致；`tan`、`acos`、`asin`、`atan`、`exp` 則和 fdlibm 逐位元相同。

## Example

`cargo run -p void-app --example lod`（預設 `--terrain layered`；`--terrain sphere` 是依層級著色的光滑球）。高度以地面為準：探測器沿地形移動，HUD 的高度是離地高度。還沒有海和大氣，海床顯示成暗色盆地；那是 scenery 移植的工作。
