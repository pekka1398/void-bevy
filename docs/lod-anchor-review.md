# 相同 terrain anchors 的 ECS 變更標記

承接`6460ff6`。每幀仍計算原f64 tile origin−camera相對量，再轉f32。當translation／rotation／scale所有f32位元相同時不寫回Transform，避免把靜止terrain標成Changed；不是停掉相機或座標更新。逐位元比較保留`+0/-0`，轉换後的非有限新transform明確panic。`--lod-always-update-anchors`是對照，`lod_anchor_updates`記錄實際寫入次數。

app lib45 tests、scoped Clippy/build通過；新增測試驗證相同值不出現Changed、signed zero確實寫入，以及baseline強制寫入。核對pinned Bevy prepass：PreUpdate在GlobalTransform比PreviousGlobalTransform更新時把前幀值轉存，GPU mesh extraction也檢查Changed<PreviousGlobalTransform>；停止位置寫入不會讓最後一次移動的history一直保留。沒有改core／physics／存檔版本或GPU幾何算法。

1920×1080、120 delivered frames、pipelined rendering、static ABBA：

| run | anchor writes每update | LOD draw p50 ms | main interval p50 ms |
| --- | ---: | ---: | ---: |
| always A1 | 1512 | 0.323429 | 11.351719 |
| skip B1 | 0 | 0.313240 | 10.286251 |
| skip B2 | 0 | 0.318590 | 10.546902 |
| always A2 | 1512 | 0.306627 | 10.482320 |

四次pixel／完整checkpoint相同；A1有較多main updates才收滿delivered captures（214，其他153–156），所以不拿A1較慢的數字宣稱FPS加速。可靠結論是相同值的重複write／Changed消失，沒有穩定整體幀時間改善證據。

同先前120-update motion、640×360、180 delivered samples、pipelined：baseline總write93,155（183 updates）；skip50,365（184 updates）；GPU verify skip53,409（187 updates）。背景完成與capture交付的update數不同，不能把總數直接當相同工作量的速度比例。三次終點pixel／checkpoint均與第四輪原motion control完全一致。GPU11,148 tiles全部逐位元通過，pending零、render errors空。

原始JSON／PNG在`lab-log/lod-anchors/`，驗收binary `target/acceptance/void-app-lod-anchor-probe`，manifest同目錄。此優化預設使用；沒有merge／push或人類GUI最終驗收。下一步只繼續有明確成本或並行疑點的LOD研究，避免從component優化推導不存在的FPS保證。
