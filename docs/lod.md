# lod：立方體球四分樹

`crates/lod`（`void-lod`）移植 `lab/lod/src/lod` 的核心，不依賴 Bevy：

| TS | Rust |
| --- | --- |
| `CubeSphere.ts`、`TileKey.ts`、`TileSearch.ts` | `cube.rs` |
| `FaceAdjacency.ts`、`TileNeighbors.ts` | `adjacency.rs`（相鄰表在第一次使用時推導並自我檢查，和 TS 一樣） |
| `TileMeshBuilder.ts` | `mesh.rs` |
| `TileRenderer.ts` 的 `stitchEdges` | `mesh.rs`：接縫縫合是幾何計算，放在核心而不是渲染端 |
| `PlanetLod.ts` | `planet_lod.rs`：節點放在以 tile code 為鍵的 arena，不用物件指標 |

沒有移植：`TileRenderer`（Three.js 的 BatchedMesh）和 `TileWorkerPool`／`TileWorkerHost`（瀏覽器 worker），這些由 Bevy 端重做。lab 的 `src/app/`（demo 地形、相機、面板）不屬於核心；demo 地形移到測試裡當作 fixture。

## 和 TS 逐位元相同

選擇的結果會受「最後幾位數」影響：兩個 tile 的優先權只差 1e-12 時，誰先建由那幾位決定。所以這個 crate 刻意做到和 lab 逐位元相同：

- **`hypot`（`math.rs`）**：照 V8 的 `Math.hypot` 實作（除以最大值、Kahan 補償求平方和，再乘回）。和 Node 的 `Math.hypot` 比對 100 萬組輸入（1e-300 到 1e300）完全相同；`sqrt(dot)` 則有 36% 不同。它也比 `sqrt(dot)` 精確、不會溢位。
- **`tan`、`atan`、`acos`、`asin` 用 `libm` crate**（fdlibm，V8 也用 fdlibm）。和 Node 比對 20 萬組輸入完全相同；Rust 標準庫呼叫系統的 glibc，有 4–7% 不同。
- **`OrderedMap`（`ordered.rs`）**：照 JavaScript `Map` 的插入順序走訪。請求順序、淘汰順序、平衡時的折疊順序都取決於它。
- f32 只在 TS typed array 存值的地方捨入（例如縫合的混合先用 f64 算）。

## 檢查（`cargo test -p void-lod`，對照資料由 `golden/lod.ts` 產生）

| 檢查 | 結果 |
| --- | --- |
| 面相鄰表、cube↔sphere、`tileContaining`、`tilesAround` | 完全相同 |
| tile mesh（lab 的 demo 地形）：L0、L3、立方體角落的 L5、著陸點的 L18 | 位置、法線、高度、顏色、誤差估計、邊界：**差 0** |
| 接縫縫合（L6 接 L5，跨立方體面） | **差 0** |
| 選擇：lab bench 的 5 條腳本路徑 × 2 種建造模型（全部建好／每幀 6 個），共 10,400 幀 | 每幀的 tile 數、請求、剔除、折疊、建造的 tile、渲染順序、請求順序與優先權、最後的快取：**全部相同** |

最初用 `sqrt(dot)` 和標準庫的 `tan` 時，「static chase、每幀 6 個」在第 154 幀選了不同的第 5、6 個 tile：兩組 tile 的優先權只差 3e-13（相對），一個 ulp 就決定了順序。換成上面兩項後才逐位元相同，檢查也因此用「差 0」當門檻，之後任何改動造成的差異都會被抓到。

lab 自己的地形（`src/app/DemoSurface.ts`）在 `demo.rs`：`DemoTerrain::preset("seam" | "normal" | "landing")`，參數來自 `presets/planets.json`。網格檢查用的就是它，所以和 lab 逐位元相同。
