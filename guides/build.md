# 編譯、測試、target

## 資源限制（AGENTS.md 第 4 條）

agent 跑的編譯、遊戲、測試都放進同一個 `void-agent.slice`，合計共用一個上限。設定在重開機後會消失，所以每個 session 開始時先跑一次（重複跑沒關係）：

```bash
systemctl --user set-property --runtime void-agent.slice MemoryMax=11G MemorySwapMax=0 CPUWeight=20
```

編譯、測試、clippy 一律經過 `tools/slice`：它把指令放進 slice，並且讓所有 agent 的這種指令一個一個跑（鎖 `/tmp/void-build.lock`）：其他 worktree 或 agent 的編譯會印出 `tools/slice: waiting for ...` 然後排隊，不會兩個 `-j 8` 同時擠在 11G 裡。使用者自己的編譯不在這把鎖裡。

```bash
tools/slice cargo build -j 8 -p void-app
```

- 量時間的工作（計時、benchmark）整段都要拿著同一把鎖：`flock /tmp/void-build.lock <script>`，script 裡的指令直接用 `systemd-run --user --scope --quiet --slice=void-agent.slice --`，不要再經過 `tools/slice`（會等自己的鎖）。
- 使用者自己在終端機跑的 `cargo run` 不經過這把鎖，也不在 slice 裡。要複製或刪除 target 時，改拿 cargo 自己的鎖 `<target>/debug/.cargo-lock`：cargo 編譯時整段都拿著它，所以不管是誰的編譯都會等它結束。
- 遊戲不要經過 `tools/slice`，否則整場遊戲都拿著鎖。先用 `tools/slice cargo build` 編好，再照 `guides/computeruse.md` 用 `systemd-run` 啟動。

超過上限時程式會被系統終止。這時停下來告訴使用者，不要重試。用 `systemctl --user show -p MemoryPeak void-agent.slice` 可以看到目前為止的峰值。

## 什麼情況用什麼指令

| 要做的 | 指令 |
|---|---|
| 改完先確認編得過 | `tools/slice cargo check -j 8 --workspace --all-targets` |
| 只測改到的 crate | `tools/slice cargo test -j 8 -p void-vessels` |
| 全部測試（合併前） | `tools/slice cargo test -j 8 --workspace` |
| 編遊戲 | `tools/slice cargo build -j 8 -p void-app` |
| 合併前 | `tools/slice cargo clippy -j 8 --workspace --all-targets` |

每個 crate 的整合測試都在 `tests/<crate>/main.rs` 底下，一個 crate 只連結一個測試執行檔。新的測試檔放進那個資料夾，在 `main.rs` 加一行 `mod`，不要在 `tests/` 底下直接放 `.rs`（會多出一個要連結的執行檔）。

## 開發編譯和發行版

開發編譯把 Bevy 編成共用函式庫（`void-app` 的 `dev` feature，預設開啟），改一行之後只重新連結約 44MB 的執行檔，不用再連結整個 Bevy。

- `cargo run -p void-app` 和直接執行 `target/debug/void-app` 都可以，不用設 `LD_LIBRARY_PATH`：執行檔靠 rpath（`crates/app/build.rs`）找到旁邊 `deps/` 裡的 Bevy 和 Rust 工具鏈裡的 libstd。所以執行檔要留在 target 裡跑，不能單獨複製到別處。

- 發行版一定要關掉 `dev`，Bevy 靜態連結：

  ```bash
  cargo build -j 8 --release -p void-app --no-default-features
  ```

  忘了加 `--no-default-features` 時，`--release` 會直接編譯失敗，不會產生依賴 `target/` 裡 `.so` 的執行檔。
- 只單獨編 `void-assembly-lab`（不含 `void-app`）時，Bevy 的 feature 組合不同，會另外重編一部分 Bevy。平常用 `-p void-app` 或 `--workspace` 就不會。

工作區的 crate 只留行號表（`debug = "line-tables-only"`），除錯資訊放在 `target/debug/deps/*.dwo`（`split-debuginfo = "unpacked"`）。panic 的 backtrace 照樣有檔名和行號，但要讀得到 `.dwo`：執行檔搬離 `target/` 或 target 被清掉之後，backtrace 就只剩函式名。

## `-j` 和記憶體

`-j 8`，在 `/mnt/data` 上的 target，筆電電源設定 Performance 時量到的（記憶體是該次指令的 scope 峰值，含 page cache；括號是實際配置的記憶體 anon 峰值）：

| 情況 | 時間 | 記憶體峰值 |
|---|---|---|
| 完整編譯 `void-app`（空的 target） | 347 s | 6.0G（4.3G） |
| `crates/vessels` 改一行後重編 `void-app` | 3.6 s | 1.0G（0.6G） |
| `crates/app` 改一行後重編 `void-app` | 2.2 s | 0.6G（0.5G） |
| `check --workspace --all-targets`，空的 | 73 s | 2.8G（2.4G） |
| `check --workspace --all-targets`，改一行後 | 1.7 s | 0.5G（0.3G） |

完整編譯大部分時間花在 Bevy 一串互相依賴的 crate（`bevy_render`、`bevy_pbr`、`bevy_ui` 各要 70–90 秒，只能一個接一個），所以 `-j` 開到 6 以上就快不了多少（改動前、電源 Balanced 時：`-j 6` 371 s、`-j 8` 317 s、`-j 2` 約 990 s，`-j 2` 那次和別的編譯重疊）。

page cache 在 slice 不夠時會先被回收，真正會讓 slice 被終止的是 anon。一個 `-j 8` 完整編譯約 4.3G，兩個同時跑就很接近上限，所以要排隊。

## target 放哪裡

每個 checkout 用自己的 target，放在 `/mnt/data/void-target/<名稱>`（另一顆 NVMe，空間多），checkout 裡的 `target` 是指過去的 symlink。`/target` 已經寫在 `.git/info/exclude`，symlink 不會進 git。

| checkout | 路徑 | target |
|---|---|---|
| 主目錄（使用者的，master） | `~/Desktop/void-bevy` | `/mnt/data/void-target/main` |
| worktree | `~/Desktop/void-bevy-<名稱>` | `/mnt/data/void-target/<名稱>` |

各用各的 target，最終執行檔不會互相蓋掉，編譯也不會互相等 target 的鎖。每個 target 編完遊戲加 check 約 6G，連測試約 7G。

開一個 worktree：

```bash
cd ~/Desktop/void-bevy
git worktree add ~/Desktop/void-bevy-<名稱> -b feature/<名稱>
flock /mnt/data/void-target/main/debug/.cargo-lock cp -a /mnt/data/void-target/main /mnt/data/void-target/<名稱>
ln -s /mnt/data/void-target/<名稱> ~/Desktop/void-bevy-<名稱>/target
```

`cp -a` 拿主目錄已經編好的 target 當起點：依賴不用重編，第一次編譯只重編工作區的 crate（複製約 8 秒，編 `void-app` 約 25 秒）。`flock` 等主目錄正在跑的編譯結束，複製時也不會有新的編譯寫進去。主目錄的 target 太大（超過約 15G）時先清理（見下面），或改成 `mkdir -p /mnt/data/void-target/<名稱>`，第一次從零編（`-j 8` 約 6 分鐘）。branch 改了依賴或 `Cargo.toml` 的 profile 時，那部分本來就會重編。

收掉一個 worktree（合併之後）。worktree 裡還有沒 commit 的改動時 `git worktree remove` 會拒絕，target 也就不會被刪：

```bash
cd ~/Desktop/void-bevy
git worktree remove ~/Desktop/void-bevy-<名稱> && \
  flock /mnt/data/void-target/<名稱>/debug/.cargo-lock rm -rf /mnt/data/void-target/<名稱>
```

## 清理

- worktree 收掉時，它的 target 同時刪掉。
- target 只會越長越大（舊的編譯結果、換過的 feature、升級過的 Rust 都留著）。`du -sh /mnt/data/void-target/*` 某個超過 30G，或 Rust 升級過，就整個刪掉重編（`flock /mnt/data/void-target/<名稱>/debug/.cargo-lock rm -rf /mnt/data/void-target/<名稱>/*`），比挑檔案清乾淨，也只要約 6 分鐘。
- `/mnt/data` 上只動 `void-target/`，其他資料夾（例如 KSP 的副本）不是這個專案的。
