#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$(mktemp -d -t mock-studio-demo-bin.XXXXXX)"
LOG_FILE="$(mktemp -t mock-studio-demo-public-base-url.XXXXXX.log)"
SERVER_LOG="$(mktemp -t mock-studio-demo-public-base-url-server.XXXXXX.log)"
PORT=43125
ADDR="127.0.0.1:${PORT}"

cleanup() {
    rm -rf "${BIN_DIR}"
    rm -f "${LOG_FILE}" "${SERVER_LOG}"
}
trap cleanup EXIT

cat >"${BIN_DIR}/cargo" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" >"${CAPTURE_ARGS_FILE}"
printf 'stub cargo invoked\n' >&2
sleep 30
EOF
chmod +x "${BIN_DIR}/cargo"

cat >"${BIN_DIR}/curl" <<'EOF'
#!/usr/bin/env bash
exit 1
EOF
chmod +x "${BIN_DIR}/curl"

cat >"${BIN_DIR}/python3" <<'EOF'
#!/usr/bin/env bash
exec /usr/bin/python3 "$@"
EOF
chmod +x "${BIN_DIR}/python3"

set +e
(
    cd "${ROOT_DIR}"
    PATH="${BIN_DIR}:$PATH" \
    CAPTURE_ARGS_FILE="${LOG_FILE}" \
    OPENROUTER_API_KEY=dummy \
    STUDIO_ADDR="${ADDR}" \
    PUBLIC_BASE_URL="https://mock.example.com" \
    timeout 5s script/run-mock-studio-demo.sh
) >"${SERVER_LOG}" 2>&1
STATUS=$?
set -e

if [[ ${STATUS} -ne 124 ]]; then
    echo "expected timeout because stub cargo never starts a real server" >&2
    cat "${SERVER_LOG}" >&2
    exit 1
fi

grep -q '^run$' "${LOG_FILE}"
grep -q '^-p$' "${LOG_FILE}"
grep -q '^raiko-mock-studio$' "${LOG_FILE}"
grep -q '^--bind$' "${LOG_FILE}"
grep -q "^${ADDR}$" "${LOG_FILE}"
grep -q '^--public-base-url$' "${LOG_FILE}"
grep -q '^https://mock.example.com$' "${LOG_FILE}"

echo "ok"
