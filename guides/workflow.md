# 工作流程

一項工作從 spec 開始，合併進 master 時結束。

1. 使用者寫好 spec，放在 `specs/<名稱>.md`，開 branch。
2. agent 照 spec 做。做的途中範圍或做法有變，就直接改 spec 檔案，讓它一直反映實際在做的事。
3. 做完後，主 agent（派出 subagent 的那個）先審查 diff：有沒有照 AGENTS.md、spec 有沒有做到、有沒有留下沒人用的程式碼，並跑 clippy、相關測試和下面的未使用程式碼檢查。
4. 再請 Codex 用唯讀模式 review 同一份 diff（用法見 `~/.claude/codex-bridge/README.md`，模型用 `gpt-6-astra`，effort `medium`）。要它逐項列出檔案和行號、失敗的情境、信心程度，並說明查過哪些地方沒問題。Codex 說的每一項都要自己核實，再轉述給使用者。
5. 有問題就交回原本做的 agent 修。修完後，主 agent 看修正的 diff，再請 Codex 只 review 這次修正的部分。額度不夠時，這一輪可以改用 `gpt-6.1-sol`。
6. 審查都過了以後：
   - 會改變玩起來的感覺的（UI、操作、物理、畫面）：向使用者報告，等使用者開遊戲驗收再合併。
   - 不會改變的（編譯設定、量測工具、內部重構、guides）：直接合併，合併後向使用者報告。
   - 拿不準時，當成需要驗收。
7. 合併進 master：
   - merge commit 的說明寫 spec 最後版本的原文，加上完成報告（做了什麼、和 spec 不同的地方、留下的問題）。
   - 同一個 commit 刪掉 `specs/<名稱>.md`。
   - 合併前在 `crates/` 跑一次下面的檢查，列出的名稱都是沒人用的程式碼，要刪掉或說明為什麼留著：

     ```bash
     for n in $(grep -rhoE "pub(\(crate\))? (fn|struct|enum|const|trait|type|static) \w+" --include=*.rs */src | awk '{print $3}' | sort -u); do
       if [ "$(grep -rwo --include=*.rs --include=*.wgsl "$n" . | wc -l)" -le 1 ]; then echo "$n"; fi
     done
     ```

`specs/` 裡只放進行中的工作。做完的工作用 `git log --merges` 查。

## 同時進行多份工作

- 主目錄（`~/Desktop/void-bevy`）留給使用者，保持在 master。每份工作在自己的 worktree 裡做，例如 `~/Desktop/void-bevy-<名稱>`。
- 同一時間最多一份工作在跑重的編譯或量測。量編譯時間、記憶體或幀時間的工作，不能和其他編譯同時進行。

## 文件放哪裡

| 文件 | 內容 |
|---|---|
| `AGENTS.md` | 行為規則，只有使用者能改 |
| `guides/` | 各領域的原則和唯一做法，做到該領域時才讀 |
| `specs/` | 進行中的工作 |
| `README.md` | 這是什麼、怎麼跑 |

程式碼本身就是說明。`guides/` 只寫光看程式碼看不出來、但一定要遵守的東西，例如「做某件事就用這個現成的設計」，內容要先和程式碼核對過。
