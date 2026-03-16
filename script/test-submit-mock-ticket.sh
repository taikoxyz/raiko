#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT=43124
ADDR="127.0.0.1:${PORT}"
SERVER_LOG="$(mktemp -t submit-mock-ticket-server.XXXXXX.log)"
OUTPUT_LOG="$(mktemp -t submit-mock-ticket-output.XXXXXX.log)"

cleanup() {
    if [[ -n "${SERVER_PID:-}" ]] && kill -0 "${SERVER_PID}" >/dev/null 2>&1; then
        kill "${SERVER_PID}" >/dev/null 2>&1 || true
        wait "${SERVER_PID}" 2>/dev/null || true
    fi
    rm -f "${SERVER_LOG}" "${OUTPUT_LOG}"
}
trap cleanup EXIT

(
    cd "${ROOT_DIR}"
    python3 - <<'PY' >"${SERVER_LOG}" 2>&1
from http.server import BaseHTTPRequestHandler, HTTPServer
import json

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path != "/api/tickets":
            self.send_response(404)
            self.end_headers()
            return
        length = int(self.headers.get("Content-Length", "0"))
        _ = self.rfile.read(length)
        payload = {
            "ticket_id": "ticket-9",
            "rule_id": "ticket-9",
            "requirement": "demo requirement",
            "summary": "demo summary",
            "status": "running",
            "base_url": "http://127.0.0.1:23009",
            "error": None,
        }
        body = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path != "/api/tickets/ticket-9":
            self.send_response(404)
            self.end_headers()
            return
        payload = {
            "ticket_id": "ticket-9",
            "rule_id": "ticket-9",
            "requirement": "demo requirement",
            "summary": "demo summary",
            "status": "running",
            "base_url": "http://127.0.0.1:23009",
            "error": None,
        }
        body = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format, *args):
        return

HTTPServer(("127.0.0.1", 43124), Handler).serve_forever()
PY
) &
SERVER_PID=$!

for _ in $(seq 1 30); do
    if curl -fsS "http://${ADDR}/api/tickets/ticket-9" >/dev/null 2>&1; then
        break
    fi
    sleep 0.2
done

(
    cd "${ROOT_DIR}"
    STUDIO_ADDR="${ADDR}" script/submit-mock-ticket.sh "demo requirement"
) >"${OUTPUT_LOG}" 2>&1

grep -q "Ticket ID : ticket-9" "${OUTPUT_LOG}"
grep -q "Rule ID   : ticket-9" "${OUTPUT_LOG}"
grep -q "Status    : running" "${OUTPUT_LOG}"
grep -q "Base URL  : http://127.0.0.1:23009" "${OUTPUT_LOG}"
grep -q 'Fetching ticket receipt' "${OUTPUT_LOG}"
grep -q '"ticket_id": "ticket-9"\|"ticket_id":"ticket-9"' "${OUTPUT_LOG}"
grep -q '"rule_id": "ticket-9"\|"rule_id":"ticket-9"' "${OUTPUT_LOG}"
grep -q "${ROOT_DIR}/mock-gateway/generated/ticket-9" "${OUTPUT_LOG}"

echo "ok"
