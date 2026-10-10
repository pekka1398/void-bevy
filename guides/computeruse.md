# 用 TigerVNC + 瀏覽器操作遊戲

agent 在獨立的虛擬螢幕 `:7` 上操作遊戲，使用者用瀏覽器看同一個畫面。所有 xdotool 指令都只送到 `:7`，使用者的桌面是 `:1`。

## 資源限制（AGENTS.md 第 4 條）

agent 跑的編譯、遊戲、測試都放進同一個 `void-agent.slice`，合計共用一個上限。設定在重開機後會消失，所以每個 session 開始時先跑一次（重複跑沒關係）：

```bash
systemctl --user set-property --runtime void-agent.slice MemoryMax=11G MemorySwapMax=0 CPUWeight=20
```

之後每個重的指令前面都加上 `systemd-run --user --scope --quiet --slice=void-agent.slice --`，例如：

```bash
systemd-run --user --scope --quiet --slice=void-agent.slice -- cargo test -j 2 -p void-vessels
```

超過上限時程式會被系統終止。這時停下來告訴使用者，不要重試。用 `systemctl --user show -p MemoryPeak void-agent.slice` 可以看到目前為止的峰值。

## 啟動

```bash
S=<scratchpad 目錄>

# 1. 虛擬螢幕，只開在本機
nohup Xvnc :7 -geometry 1440x900 -depth 24 -localhost -SecurityTypes None \
  -rfbport 5907 -AlwaysShared > $S/xvnc.log 2>&1 &

# 2. 瀏覽器用的轉接，一定要綁 127.0.0.1（VNC 沒密碼，綁到所有介面會讓區網的人連進來）
nohup websockify --web /usr/share/novnc 127.0.0.1:6080 localhost:5907 > $S/ws.log 2>&1 &

# 3. 遊戲，放進 void-agent.slice；從 worktree 目錄啟動，存檔會寫到這裡的 saves/
cd <worktree> && DISPLAY=:7 nohup systemd-run --user --scope --quiet --slice=void-agent.slice -- \
  <target>/debug/void-app > $S/game.log 2>&1 &

# 4. 在使用者桌面開瀏覽器
DISPLAY=:1 xdg-open "http://127.0.0.1:6080/vnc.html?autoconnect=true&resize=scale"
```

啟動後立刻用 `ps` 記下 Xvnc、websockify、void-app 的 PID，跟使用者說。結束時只用這些數字 PID `kill`。

驗收結束就關掉遊戲、websockify、Xvnc（遊戲沒人看也會一直吃約 1 GB 記憶體和一個多核心）。使用者要接著看時才留著，並告訴他 PID。

用 `ss -ltnp | grep -E "6080|5907"` 確認兩個埠都只在 `127.0.0.1` 上。

## 遊戲視窗

`:7` 上沒有視窗管理器，所以：

- 視窗不會自動填滿螢幕，要手動拉：
  `xdotool windowmove $W 0 0; xdotool windowsize $W 1440 900`
- 鍵盤焦點不會自動給遊戲，要手動設：`xdotool windowfocus $W`。按鍵完全沒反應時，先檢查這個。
- `$W` 用 `xdotool search --name '^VOID$'` 取得。

## 輸入

遊戲每幀才讀一次輸入，按下和放開太快會被吃掉：

- 按鍵：`keydown`，等 0.12 秒，`keyup`。
- 點擊：先把滑鼠移過去等 0.8 秒，再 `mousedown`、等 0.4 秒、`mouseup`。
- 輸入數字欄位：點欄位，用 BackSpace／Delete 清空，逐字按鍵（`-` 用 `minus`、`.` 用 `period`），最後按 Return。

## 座標和確認

- 座標從最新的截圖讀。版面可能因為文字換行而整排移位，所以每做一步就截圖確認結果，再決定下一步。
- 截圖：`DISPLAY=:7 import -window root $S/x.png`。要看細節就用 `convert x.png -crop WxH+X+Y` 裁出一塊。
- 點了沒效果時，先截圖看是沒點到還是位置變了，再重點。按下一個快捷鍵（例如 P）之前，先確認上一步已經生效。

## 輔助函式

```bash
export DISPLAY=:7
S=<scratchpad 目錄>
key(){ xdotool keydown $1; sleep 0.12; xdotool keyup $1; sleep 0.12; }
click(){ xdotool mousemove $1 $2; sleep 0.8; xdotool mousedown 1; sleep 0.4; xdotool mouseup 1; sleep 0.6; }
field(){ click $1 $2; for i in $(seq 14); do key BackSpace; done; for i in $(seq 14); do key Delete; done
  local s="$3"; for ((i=0;i<${#s};i++)); do c=${s:i:1}; case $c in -) k=minus;; .) k=period;; *) k=$c;; esac; key $k; done
  key Return; sleep 0.5; }
shot(){ import -window root $S/$1.png; }
xdotool windowfocus $(xdotool search --name "^VOID$" | head -1)
```

全程只用這組函式；遇到時間不夠之類的問題就改函式本身，讓每次操作都走同一條路。

## 驗收

- 用正常的遊戲世界，情境靠 DEV 的「放置船」做出來（AGENTS.md 第 8 條）。
- agent 的操作只是初步檢查，最後以使用者自己開遊戲看到的為準（AGENTS.md 第 7 條）。
- 回報時講看到了什麼、哪裡不對，附上關鍵數值（高度、速度、記憶體）。
