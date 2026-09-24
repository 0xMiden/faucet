#!/usr/bin/env bash
#
# The end-to-end test itself: requests tokens from a running faucet with the client binary, and
# checks that the faucet answers with a note.
#
# The network, the funding service and the faucet have to be running already; `make test-e2e`
# starts them.

set -euo pipefail

CLIENT_BIN="${CLIENT_BIN:-./target/debug/miden-faucet-client}"
FAUCET_URL="${FAUCET_URL:-http://127.0.0.1:18000}"
# A well-formed account id. The funding service creates the note for it whether or not the account
# exists on chain, and nothing here consumes the note, so it needs no keys.
TARGET_ACCOUNT="${TARGET_ACCOUNT:-0x52488050da9336511810b97a086a5e}"
# Small enough to leave the funding account with something for the next run.
AMOUNT="${AMOUNT:-100}"

echo "Requesting ${AMOUNT} base units for ${TARGET_ACCOUNT} from ${FAUCET_URL}"
output="$(
    "${CLIENT_BIN}" mint \
        --url "${FAUCET_URL}" \
        --target-account "${TARGET_ACCOUNT}" \
        --amount "${AMOUNT}" \
        --no-consume
)"
echo "${output}"

if ! grep -q 'note commitment: 0x' <<<"${output}"; then
    echo "error: the faucet did not report a note" >&2
    exit 1
fi

echo "The faucet returned a note"
