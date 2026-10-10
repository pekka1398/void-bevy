# 工作流程

一項工作從 spec 開始，合併進 master 時結束。

1. 使用者寫好 spec，放在 `specs/<名稱>.md`，開 branch。
2. agent 照 spec 做。做的途中範圍或做法有變，就直接改 spec 檔案，讓它一直反映實際在做的事。
3. 做完後 agent 在 session 裡向使用者報告，回答提問，使用者開遊戲驗收。
4. 合併進 master：
   - merge commit 的說明寫 spec 最後版本的原文，加上完成報告（做了什麼、和 spec 不同的地方、留下的問題）。
   - 同一個 commit 刪掉 `specs/<名稱>.md`。
   - 合併前在 `crates/` 跑一次下面的檢查，列出的名稱都是沒人用的程式碼，要刪掉或說明為什麼留著：

     ```bash
     for n in $(grep -rhoE "pub(\(crate\))? (fn|struct|enum|const|trait|type|static) \w+" --include=*.rs */src | awk '{print $3}' | sort -u); do
       if [ "$(grep -rwo --include=*.rs --include=*.wgsl "$n" . | wc -l)" -le 1 ]; then echo "$n"; fi
     done
     ```

`specs/` 裡只放進行中的工作。做完的工作用 `git log --merges` 查。

## 文件放哪裡

| 文件 | 內容 |
|---|---|
| `AGENTS.md` | 行為規則，只有使用者能改 |
| `guides/` | 各領域的原則和唯一做法，做到該領域時才讀 |
| `specs/` | 進行中的工作 |
| `README.md` | 這是什麼、怎麼跑 |

程式碼本身就是說明。`guides/` 只寫光看程式碼看不出來、但一定要遵守的東西，例如「做某件事就用這個現成的設計」，內容要先和程式碼核對過。
