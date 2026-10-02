#!/usr/bin/env bash
#
# Starts and stops the faucet the end-to-end test runs against.

set -euo pipefail

FAUCET_BIN="${FAUCET_BIN:-./target/release/miden-faucet}"
FAUCET_URL="${FAUCET_URL:-http://127.0.0.1:18000}"
NODE_URL="${NODE_URL:-http://127.0.0.1:57291}"
FUNDING_SERVICE_URL="${FUNDING_SERVICE_URL:-http://127.0.0.1:50401}"
DECIMALS="${DECIMALS:-6}"

WORK_DIR="${WORK_DIR:-target/e2e}"
PID_FILE="${WORK_DIR}/faucet.pid"
LOG_FILE="${WORK_DIR}/faucet.log"
API_KEYS_FILE="${WORK_DIR}/api_keys.txt"

# How long the faucet gets to reach the funding service and bind its API.
START_TIMEOUT_SECONDS="${START_TIMEOUT_SECONDS:-60}"

start() {
    stop

    mkdir -p "${WORK_DIR}"
    # The faucet reads its API keys on startup, so the file has to exist even with no keys in it.
    touch "${API_KEYS_FILE}"

    echo "Starting the faucet on ${FAUCET_URL}, logging to ${LOG_FILE}"
    "${FAUCET_BIN}" start \
        --funding-service-url "${FUNDING_SERVICE_URL}" \
        --node-url "${NODE_URL}" \
        --network localhost \
        --decimals "${DECIMALS}" \
        --api-bind-port "${FAUCET_URL##*:}" \
        --api-keys-file "${API_KEYS_FILE}" \
        --no-frontend \
        > "${LOG_FILE}" 2>&1 &
    echo $! > "${PID_FILE}"

    for _ in $(seq "${START_TIMEOUT_SECONDS}"); do
        if curl --silent --fail --output /dev/null "${FAUCET_URL}/get_metadata"; then
            echo "The faucet is serving its metadata"
            return 0
        fi
        sleep 1
    done

    echo "error: the faucet did not answer within ${START_TIMEOUT_SECONDS}s" >&2
    cat "${LOG_FILE}" >&2
    return 1
}

stop() {
    [[ -f "${PID_FILE}" ]] || return 0

    local pid
    pid="$(cat "${PID_FILE}")"
    rm -f "${PID_FILE}"

    kill "${pid}" 2> /dev/null || return 0
    echo "Stopped the faucet (pid ${pid})"
}

case "${1:-}" in
    up) start ;;
    down) stop ;;
    *)
        echo "usage: $0 up|down" >&2
        exit 2
        ;;
esac
