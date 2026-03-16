#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT=43123
ADDR="127.0.0.1:${PORT}"
LOG_FILE="$(mktemp -t mock-studio-demo-port-guard.XXXXXX.log)"
SERVER_LOG="$(mktemp -t mock-studio-demo-port-guard-server.XXXXXX.log)"

cleanup() {
    if [[ -n "${SERVER_PID:-}" ]] && kill -0 "${SERVER_PID}" >/dev/null 2>&1; then
        kill "${SERVER_PID}" >/dev/null 2>&1 || true
        wait "${SERVER_PID}" 2>/dev/null || true
    fi
    rm -f "${LOG_FILE}" "${SERVER_LOG}"
}
trap cleanup EXIT

(
    cd "${ROOT_DIR}"
    python3 -m http.server "${PORT}" --bind 127.0.0.1 >"${SERVER_LOG}" 2>&1
) &
SERVER_PID=$!

for _ in $(seq 1 30); do
    if curl -fsS "http://${ADDR}/" >/dev/null 2>&1; then
        break
    fi
    sleep 0.2
done

set +e
(
    cd "${ROOT_DIR}"
    OPENROUTER_API_KEY=dummy STUDIO_ADDR="${ADDR}" timeout 20s script/run-mock-studio-demo.sh
) >"${LOG_FILE}" 2>&1
STATUS=$?
set -e

if [[ ${STATUS} -eq 0 ]]; then
    echo "expected demo script to fail when studio address is occupied" >&2
    cat "${LOG_FILE}" >&2
    exit 1
fi

if ! grep -q "already in use" "${LOG_FILE}"; then
    echo "expected occupied-address error message" >&2
    cat "${LOG_FILE}" >&2
    exit 1
fi

if grep -q "Submitting ticket" "${LOG_FILE}"; then
    echo "script should fail before submitting a ticket" >&2
    cat "${LOG_FILE}" >&2
    exit 1
fi

echo "ok"
