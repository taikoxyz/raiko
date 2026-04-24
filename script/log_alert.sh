#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-tg}"                     # tg | slack
LOG_FILE="${2:-/var/log/app.log}"   # log file path
PATTERN="${3:-ERROR|FATAL|EXCEPTION}"
MAX_ALERTS="${4:-100}"              # stop sending after this many alerts
[[ "$MAX_ALERTS" =~ ^[0-9]+$ ]] || { echo "max_alerts must be a non-negative integer"; exit 1; }

HOST="$(hostname)"
now() { date -u +"%Y-%m-%dT%H:%M:%SZ"; }

send_tg() {
  local text="$1"
  : "${TG_BOT_TOKEN:?Set TG_BOT_TOKEN}"
  : "${TG_CHAT_ID:?Set TG_CHAT_ID}"
  curl -fsS "https://api.telegram.org/bot${TG_BOT_TOKEN}/sendMessage" \
    -d chat_id="$TG_CHAT_ID" \
    --data-urlencode text="$text" >/dev/null
}

send_slack() {
  local text="$1"
  : "${SLACK_WEBHOOK:?Set SLACK_WEBHOOK}"
  local esc
  esc="$(printf '%s' "$text" | sed 's/\\/\\\\/g; s/"/\\"/g')"
  curl -fsS -X POST -H 'Content-Type: application/json' \
    --data "{\"text\":\"$esc\"}" \
    "$SLACK_WEBHOOK" >/dev/null
}

case "$MODE" in
  tg|slack) ;;
  *) echo "Usage: $0 <tg|slack> [log_file] [pattern] [max_alerts]"; exit 1 ;;
esac

# -n0 = only new lines from now on. Change to -n +1 to replay existing lines.
count=0
tail -n0 -F "$LOG_FILE" \
| grep --line-buffered -E -i "$PATTERN" \
| while IFS= read -r line; do
    if ((count >= MAX_ALERTS)); then printf '[stopped] max_alerts=%s reached\n' "$MAX_ALERTS"; exit 0; fi
    msg="[$(now)] [$HOST] $line"
    if [[ "$MODE" == "tg" ]]; then
      send_tg "$msg"
      printf '[sent][tg] %s\n' "$msg"
    else
      send_slack "$msg"
      printf '[sent][slack] %s\n' "$msg"
    fi
    count=$((count + 1))
  done
