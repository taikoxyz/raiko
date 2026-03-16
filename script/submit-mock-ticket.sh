#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STUDIO_ADDR="${STUDIO_ADDR:-127.0.0.1:4010}"
STUDIO_URL="http://${STUDIO_ADDR}"

if [[ $# -lt 1 ]]; then
    echo "Usage: script/submit-mock-ticket.sh '<requirement>'" >&2
    exit 1
fi

REQUIREMENT="$1"

if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required." >&2
    exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
    echo "python3 is required." >&2
    exit 1
fi

echo "Submitting ticket to ${STUDIO_URL}"
CREATE_RESPONSE="$(
    curl -fsS "${STUDIO_URL}/api/tickets" \
        -H 'content-type: application/json' \
        -d "$(python3 - "${REQUIREMENT}" <<'PY'
import json, sys
print(json.dumps({"requirement": sys.argv[1]}, ensure_ascii=False))
PY
)"
)"

echo "Create response:"
echo "${CREATE_RESPONSE}"

readarray -t PARSED < <(python3 - "${CREATE_RESPONSE}" <<'PY'
import json, sys
payload = json.loads(sys.argv[1])
print(payload.get("ticket_id", ""))
print(payload.get("rule_id", ""))
print(payload.get("status", ""))
print(payload.get("base_url", "") or "")
print(payload.get("error", "") or "")
PY
)

TICKET_ID="${PARSED[0]}"
RULE_ID="${PARSED[1]}"
STATUS="${PARSED[2]}"
BASE_URL="${PARSED[3]}"
ERROR_MESSAGE="${PARSED[4]}"

if [[ -z "${TICKET_ID}" ]]; then
    echo "Failed to parse ticket response." >&2
    exit 1
fi

echo
echo "Ticket ID : ${TICKET_ID}"
echo "Rule ID   : ${RULE_ID}"
echo "Status    : ${STATUS}"
if [[ -n "${BASE_URL}" ]]; then
    echo "Base URL  : ${BASE_URL}"
fi
if [[ -n "${ERROR_MESSAGE}" ]]; then
    echo "Error     : ${ERROR_MESSAGE}"
fi

echo
echo "Fetching ticket receipt"
RECEIPT="$(curl -fsS "${STUDIO_URL}/api/tickets/${TICKET_ID}")"
echo "${RECEIPT}"

echo
echo "Generated rule directory:"
echo "  ${ROOT_DIR}/mock-gateway/generated/${RULE_ID}"
