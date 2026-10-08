# 水阻力候選驗收

分支 `work/splashdown-performance`；worktree `/home/pekka/Desktop/void-bevy-water-perf`。
model27，未合入master。本次implicit阻力、主遊戲控制及最後核對由主agent自行完成。

## 操作

```sh
scripts/run-water-review.sh
scripts/run-water-review.sh 200 90 60
```

海面上方8m、暫停開始。P開始／暫停；R重跑；Shift+R切下一組並暫停；滾輪拉近。
三個參數依序是速度m/s、船身傾角度、飛行方向偏離垂直向下的角度。

| 組別 | 速度 | 船身傾角 | 入水方向偏離垂直 |
| --- | --- | --- | --- |
| 1 | 2 | 0° | 0° |
| 2 | 20 | 45° | 0° |
| 3 | 80 | 90° | 0° |
| 4 | 200 | 0° | 0° |
| 5 | 80 | 45° | 45° |
| 6 | 200 | 90° | 60° |

看入水是否停住、減速是否太生硬／太軟、斜入水與翻轉是否自然、最後是否浮起。
場景沒有撞擊破壞；agent視窗核對不代替人類驗收。

## 計算與核對

水阻力只處理海面相對運動，不夾總速度。浮力仍按排水量和質量；碰撞、推力、
資源、熱、owner、存檔及journal沿用原runtime。Hull cache隨Fleet釋放，air-only
查詢避免重複取地形；見[splashdown-performance.md](splashdown-performance.md)。

```sh
cargo run -p void-fleet-flight --example splashdown_timing -j 2 -- \
  --initial-world crates/fleet-flight/fixtures/water-performance-world.json 200 90 60
```

CSV的`scene_dt_ms`是新模型Scene cadence，不是舊`stable_dt_ms`界線。predict單獨計時。
不同物理規則的軌跡不要求與26逐行相同。

檢查採受影響environment/modules/vessels/fleet-flight/landing、app lib及lint，`-j 2`，
未跑全workspace。新增強阻力不反向、偏心平移與旋轉耗能／轉框架、native阻力不
重複補半步速度、R／Shift+R不累積船及錄放檢查。

原高速浸水測試要求總速率單調下降；浮力可使阻力減速後的物體向上加速。
改為檢查無驅動的水平分量耗散，保留原`+1`門檻及終態速度平方`<100`；
純阻力耗能由獨立耦合測試核對，不放寬界線掩蓋能量注入。

初步同配方入水區間：model26最慢1.83s，implicit候選約3ms／Advance0.05s。
六組12s headless中位數約1.3–1.5ms，第一次Scene初始化約68ms；皆finite，
終態距海面約0.25–0.38m、速率0.13–0.19m/s。最低COM海拔約-1.38m
（200m/s垂直入水）；這些數據不代表視覺合理性已通過。

GUI recording包含同步journal寫入成本；性能另跑不錄製場景，不能把paused寫入
停頓當成水阻力成本。證據存於
`/home/pekka/Archives/VOID/2026-10-08/water-implicit-review/`。

最後核對：受影響套件原整組260 passed／1既有ignored，新增native boundary regression
另通過；最終水／air-mode／modules測試、相關all-target lint／fmt通過。app lib
29 passed及lint通過。TigerVNC實際跑20m/s傾45°、200m/s船身90°／入水60°，
主遊戲R／Shift+R重建、model27視窗journal的headless驗證通過。
不錄製GUI profile：兩次首次Scene建立約69–71ms，其餘simulation span最慢約3ms；
profile含暫停幀，不用其中位數宣稱遊戲frame rate。人類動態驗收尚未完成。
