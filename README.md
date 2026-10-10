# VOID

太空飛行沙盒，用 Bevy、Rust 和 Rapier 寫成。

```bash
cargo run -j 8 -p void-app            # 主遊戲
cargo run -j 8 -p void-app -- --help  # 參數說明

# 發行版：Bevy 靜態連結，執行檔不依賴 target/ 裡的 .so
cargo build -j 8 --release -p void-app --no-default-features
```

- 開發規則：[AGENTS.md](AGENTS.md)
- 編譯、測試、target 怎麼放：[guides/build.md](guides/build.md)
- 各領域的做法：[guides/](guides/)
- 進行中的工作：[specs/](specs/)
