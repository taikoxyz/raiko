#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STUDIO_ADDR="${STUDIO_ADDR:-127.0.0.1:4010}"
STUDIO_URL="http://${STUDIO_ADDR}"
REQUIREMENT="${1:-请为 /v3/proof/batch/shasta 生成一个 mock：前 3 次返回 registered，第 4 次返回 error，message 是 forced failure on 4th request。}"
STUDIO_LOG="${STUDIO_LOG:-$(mktemp -t mock-studio-demo.XXXXXX.log)}"

if [[ -z "${OPENROUTER_API_KEY:-}" ]]; then
    echo "OPENROUTER_API_KEY is required." >&2
    echo "Example:" >&2
    echo "  export OPENROUTER_API_KEY=..." >&2
    exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo is required." >&2
    exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required." >&2
    exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
    echo "python3 is required." >&2
    exit 1
fi

if ! python3 - "${STUDIO_ADDR}" <<'PY'
import socket
import sys

addr = sys.argv[1]
try:
    host, port = addr.rsplit(":", 1)
    port = int(port)
except ValueError:
    print(f"Invalid STUDIO_ADDR: {addr}", file=sys.stderr)
    sys.exit(1)

sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
try:
    sock.bind((host, port))
except OSError:
    print(f"mock_studio address already in use: {addr}", file=sys.stderr)
    sys.exit(1)
finally:
    sock.close()
PY
then
    exit 1
fi

cleanup() {
    if [[ -n "${STUDIO_PID:-}" ]] && kill -0 "${STUDIO_PID}" >/dev/null 2>&1; then
        kill "${STUDIO_PID}" >/dev/null 2>&1 || true
        wait "${STUDIO_PID}" 2>/dev/null || true
    fi
}
trap cleanup EXIT

echo "Starting mock studio on ${STUDIO_ADDR}"
(
    cd "${ROOT_DIR}"
    cargo run -p raiko-mock-studio -- "${STUDIO_ADDR}" >"${STUDIO_LOG}" 2>&1
) &
STUDIO_PID=$!

echo "Studio log: ${STUDIO_LOG}"
echo "Waiting for ${STUDIO_URL} ..."
for _ in $(seq 1 60); do
    if curl -fsS "${STUDIO_URL}/" >/dev/null 2>&1; then
        break
    fi
    sleep 1
done

if ! curl -fsS "${STUDIO_URL}/" >/dev/null 2>&1; then
    echo "mock_studio did not become ready." >&2
    echo "--- studio log ---" >&2
    cat "${STUDIO_LOG}" >&2 || true
    exit 1
fi

echo "Submitting ticket"
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

RULE_DIR="${ROOT_DIR}/mock-gateway/generated/${RULE_ID}"
echo
echo "Generated rule directory:"
echo "  ${RULE_DIR}"

if [[ -d "${RULE_DIR}" ]]; then
    echo "Key files:"
    for path in \
        "conversation.md" \
        "spec.json" \
        "ticket.rs" \
        "llm/spec_prompt.md" \
        "llm/spec_response.json" \
        "llm/handler_prompt.md" \
        "llm/handler_response.json" \
        "build.log" \
        "runtime.log" \
        "receipt.json"
    do
        if [[ -f "${RULE_DIR}/${path}" ]]; then
            echo "  - ${RULE_DIR}/${path}"
        fi
    done
fi

if [[ -n "${BASE_URL}" ]]; then
    echo
    echo "Quick health check:"
    curl -fsS "${BASE_URL}/health" || true
    echo
    echo
    echo "Example mock request:"
    cat <<EOF
curl -s ${BASE_URL}/v3/proof/batch/shasta \\
  -H 'content-type: application/json' \\
  -d '{
    "l1_network": "ethereum",
    "network": "taiko",
    "proof_type": "native",
    "prover": "0x0000000000000000000000000000000000000000",
    "aggregate": false,
    "proposals": [
      {
        "proposal_id": 101,
        "l1_inclusion_block_number": 9001
      }
    ]
  }'
EOF
fi
