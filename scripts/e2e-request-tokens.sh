#!/usr/bin/env bash
#
# The end-to-end test itself.
#
# Creates a recipient account with the `miden-client` CLI, requests tokens for it from the faucet,
# lets the faucet client consume the resulting note, and checks that the account ends up holding
# the asset.
#
# The network, the funding service and the faucet have to be running already; `make test-e2e`
# starts them.

set -euo pipefail

CLIENT_BIN="${CLIENT_BIN:-./target/release/miden-faucet-client}"
WORK_DIR="${WORK_DIR:-target/e2e}"
MIDEN_CLIENT_BIN="${MIDEN_CLIENT_BIN:-${WORK_DIR}/bin/miden-client}"
FAUCET_URL="${FAUCET_URL:-http://127.0.0.1:18000}"
NODE_URL="${NODE_URL:-http://127.0.0.1:57391}"
# Bech32 prefix the faucet uses for a local network, so the client prints matching addresses.
NETWORK_ID="${NETWORK_ID:-mlcl}"
# Consuming the note pays a fee out of the note itself, so a request too small to cover the fee
# fails inside the transaction kernel.
AMOUNT="${AMOUNT:-1000000}"

# Keep the client's config, store and keys out of the developer's own ~/.miden.
MIDEN_CLIENT_HOME="${PWD}/${WORK_DIR}/miden-client"
export MIDEN_CLIENT_HOME
rm -rf "${MIDEN_CLIENT_HOME}"
mkdir -p "${MIDEN_CLIENT_HOME}"

echo "Pointing the client at ${NODE_URL}"
"${MIDEN_CLIENT_BIN}" init --network "${NODE_URL}" --network-id "${NETWORK_ID}"

echo "Creating the recipient account"
account="$("${MIDEN_CLIENT_BIN}" new-wallet --account-type public | grep -o '0x[0-9a-f]\{2,\}' | head -1)"
if [[ -z "${account}" ]]; then
    echo "error: could not read the new account's id" >&2
    exit 1
fi
echo "Recipient: ${account}"

echo "Requesting ${AMOUNT} base units from ${FAUCET_URL}"
"${CLIENT_BIN}" mint --url "${FAUCET_URL}" --target-account "${account}" --amount "${AMOUNT}"

# The consume transaction pays the fee out of the note, so the balance is the requested amount less
# that fee. What matters here is that the account ends up holding the asset at all.
echo "Reading the recipient's balance"
balance="$("${MIDEN_CLIENT_BIN}" account --show "${account}")"
echo "${balance}"

if ! grep -q "Fungible Asset" <<<"${balance}"; then
    echo "error: the recipient holds no asset after consuming the note" >&2
    exit 1
fi

echo "The recipient received and consumed the note"
