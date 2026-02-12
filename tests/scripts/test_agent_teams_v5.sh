#!/bin/bash
# Agent Teams v5: 初期プロンプトをコマンド引数で渡す
set -e

SESSION="apiary-teams-v5-$$"
TIMEOUT=120
LOG="/tmp/apiary-teams-v5.log"

cleanup() {
    echo "Cleaning up session: $SESSION"
    tmux kill-session -t "$SESSION" 2>/dev/null || true
}
trap cleanup EXIT

echo "=== Agent Teams Split-Pane Test v5 ===" | tee "$LOG"
echo "Session: $SESSION" | tee -a "$LOG"

# 1. tmux セッション作成（Claude がこの中で走れば auto → split-pane）
tmux new-session -d -s "$SESSION" -x 200 -y 50

# 2. Claude を起動（プロンプトを引数で渡す）
#    --max-turns で暴走防止
#    CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 で Teams 有効
PROMPT="Create an agent team with 2 teammates. Teammate 1 should read src/main.rs and write a one-sentence summary. Teammate 2 should read src/pod/detector.rs and write a one-sentence summary. Combine results when done."

tmux send-keys -t "$SESSION" \
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 claude --max-turns 10 \"$PROMPT\"" Enter

echo "Claude starting with agent team prompt..." | tee -a "$LOG"

# 3. pane 数を監視
INITIAL_PANES=$(tmux list-panes -s -t "$SESSION" | wc -l | tr -d ' ')
MAX_PANES=$INITIAL_PANES
PANES_CHANGED=false

for i in $(seq 1 $TIMEOUT); do
    sleep 1
    CURRENT_PANES=$(tmux list-panes -s -t "$SESSION" 2>/dev/null | wc -l | tr -d ' ')

    if [ "$CURRENT_PANES" -gt "$MAX_PANES" ]; then
        MAX_PANES=$CURRENT_PANES
        PANES_CHANGED=true
        echo "[${i}s] *** PANES: $INITIAL_PANES -> $CURRENT_PANES ***" | tee -a "$LOG"
        tmux list-panes -s -t "$SESSION" -F "  #{pane_id} pid=#{pane_pid} cmd=#{pane_current_command}" | tee -a "$LOG"
    fi

    # 15秒ごとにステータス表示
    if [ $((i % 15)) -eq 0 ]; then
        echo "[${i}s] panes=$CURRENT_PANES (max=$MAX_PANES)" | tee -a "$LOG"
        # 各 pane の末尾を表示
        for pane_id in $(tmux list-panes -s -t "$SESSION" -F "#{pane_id}"); do
            PANE_TAIL=$(tmux capture-pane -p -t "$pane_id" 2>/dev/null | grep -v '^$' | tail -2 || echo "(empty)")
            echo "  [$pane_id] $PANE_TAIL" | tee -a "$LOG"
        done
    fi

    # Claude の完了検出: max-turns 到達 or "Cooked" パターン
    MAIN_CAP=$(tmux capture-pane -p -t "$SESSION" 2>/dev/null || echo "")
    if [ "$i" -gt 30 ] && echo "$MAIN_CAP" | grep -qE '(max turns|Auto-compact|session ended)'; then
        echo "[${i}s] Claude reached max turns or ended." | tee -a "$LOG"
        break
    fi
done

echo "" | tee -a "$LOG"
echo "=== Final Result ===" | tee -a "$LOG"
FINAL_PANES=$(tmux list-panes -s -t "$SESSION" 2>/dev/null | wc -l | tr -d ' ')
echo "Initial panes: $INITIAL_PANES" | tee -a "$LOG"
echo "Max panes:     $MAX_PANES" | tee -a "$LOG"
echo "Final panes:   $FINAL_PANES" | tee -a "$LOG"
echo "Panes changed: $PANES_CHANGED" | tee -a "$LOG"

if [ "$PANES_CHANGED" = "true" ]; then
    echo "" | tee -a "$LOG"
    echo "SUCCESS: Agent Teams created $MAX_PANES panes (from $INITIAL_PANES)!" | tee -a "$LOG"
    tmux list-panes -s -t "$SESSION" -F "  #{pane_id} pid=#{pane_pid} cmd=#{pane_current_command}" | tee -a "$LOG"
else
    echo "" | tee -a "$LOG"
    echo "No new panes created." | tee -a "$LOG"
fi

# 全 pane のキャプチャを保存
echo "" | tee -a "$LOG"
echo "=== All pane captures ===" | tee -a "$LOG"
for pane_id in $(tmux list-panes -s -t "$SESSION" -F "#{pane_id}" 2>/dev/null); do
    echo "--- $pane_id ---" | tee -a "$LOG"
    tmux capture-pane -p -t "$pane_id" 2>/dev/null | grep -v '^$' | tail -25 | tee -a "$LOG"
done
