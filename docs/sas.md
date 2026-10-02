# sas

`crates/sas`（`void-sas`）是 `lab/sas/src/StabilityAssist.ts` 的移植：KSP 式的姿態穩定。

- `StabilityAssist::command(rotation, angular_velocity, inertia, pilot, dt)` 回傳和按鍵相同的 `turn` 指令（每軸 −1 到 1，乘上 `STEERING_TORQUE`）。lab 傳 `AttitudeSample`；這裡拆成三個參數，讓 crate 不依賴 Rapier。接到火箭上時，把 `PartJointRocket::advance` 的 steering 閉包寫成 `|s, dt| sas.command(s.rotation, s.angular_velocity, &s.inertia_local, pilot, dt)`。
- 階段 `Off / Pilot / Damping / Holding`、`SAS_TUNING`、`attitude_error` 與 lab 相同。不合理的調參、輸入或慣量會 panic。

## 檢查

`cargo test -p void-sas --release`：

- 與 lab 逐位元相同：`golden/sas.ts` 記錄 lab 控制器每一步看到的姿態與輸出（兩段共 2100 步，含踢、按鍵、放開、開關），Rust 控制器吃同樣的輸入，指令、階段與鎖定姿態都完全相同。另有 200 組 `attitude_error`。積分器不在比對內（V8 的 sin/cos 不是 fdlibm）。`atan2` 用 `void_math::atan2`（fdlibm，與 V8 相同）。
- lab 的 `sas-check.ts` 全部檢查，門檻相同，數字與 lab 一致（例如滿載被踢 0.2 rad/s：偏 1.55°，2.20 s 回穩，過衝 0.96%）。包括經 `PartJointRocket` 在 contact 與 flight 模式中每步呼叫一次並保持姿態。

## example `sas`

`cargo run -p void-app --example sas`：lab 的頁面。示範火箭（真實慣量）、固定的線框球與座標軸、青色箭頭是機首、灰色是鎖定的機首，左下圖表是最近 15 秒（青：離鎖定角度，橙：角速度，紫：最大指令）。

T 開關 SAS；W/S 俯仰、A/D 偏航、Q/E 滾轉；K 踢 0.2 rad/s，Shift+K 踢 1 rad/s；R 重來；V 切換整節／上級；`-` `=` 慣量倍率；1/2、3/4、5/6、7/8 調整 rate、attitude/rate、brake、lock rate，0 回預設。
