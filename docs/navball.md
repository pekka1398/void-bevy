# navball

`crates/navball`（`void-navball`）是 `lab/navball/src/Navball.ts` 的移植：姿態球。

- 幾何：`navball_basis`、`horizon_axes`（極點用 grid north）、`to_ball`、`heading_pitch`、`horizon_direction`、`MARKER_MIN_SPEED`。輸入是同一座標系裡的普通向量，和 lab 一樣，非單位向量、機首與頂部不垂直、本初子午線不在赤道上都會 panic。
- 繪圖：`NavballPainter` 不用 canvas，直接畫進 RGBA 緩衝區。天空與地面逐像素，公式與 lab 相同。格線、標記、準星與外框用抗鋸齒的線條覆蓋率，以 source-over 混合，和 canvas 一樣。文字標籤（方位與俯仰數字）以位置與透明度交給呼叫端去畫。

## 檢查

`cargo test -p void-navball --release`：

- `golden/navball.ts`：300 組隨機姿態與緯度（含兩極），basis、球面座標、heading 與 pitch 與 lab 逐位元相同；`horizon_direction` 用 sin/cos，差在 1e-15 以內。
- lab 的 `navball-check.ts` 全部檢查：直立火箭的方位、heading/pitch 來回、水平飛行與爬升標記、到 ±90° 的右手座標、兩極的 grid north、錯誤輸入 panic。另外有一項 painter 的簡單像素檢查。

## example `navball`

`cargo run -p void-app --example navball`：lab 的頁面，320 px 與 150 px（lab/flight 的大小）兩顆球。WASD QE 依機體軸轉動；按鍵取代滑桿：`[` `]` 緯度、J L 速度方位、I K 速度俯仰、`-` `=` 速度（對數），R 回到起始狀態。

注意：lab 的球是「從外面看的地球儀」，螢幕右方是 `top × nose`。面向東方時，右邊是 60、左邊是 120，和 KSP 左右相反；D（偏航向右）會往這個方向轉。這是 lab 本身的約定，移植時照原樣保留。
