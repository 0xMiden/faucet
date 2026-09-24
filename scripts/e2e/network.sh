#!/usr/bin/env bash
#
# Starts and stops the node and funding service the end-to-end test runs against.
#
# The node repo already has a compose stack for a complete local network, so it is cloned and used
# as is. The images are the published ones, so nothing is built.

set -euo pipefail

WORK_DIR="${WORK_DIR:-target/e2e}"
REGISTRY="${REGISTRY:-ghcr.io/0xmiden}"
PROJECT="${PROJECT:-miden-faucet-e2e}"
VERSION="${NODE_VERSION:-$(sed -n 's/^miden-node-proto-build *= *{ *version *= *"=\{0,1\}\([^"]*\)".*/\1/p' bin/faucet/Cargo.toml)}"

if [[ -z "${VERSION}" ]]; then
    echo "error: could not read the node version from bin/faucet/Cargo.toml" >&2
    exit 1
fi

CHECKOUT="${WORK_DIR}/node-${VERSION}"

export MIDEN_NODE_IMAGE="${REGISTRY}/miden-node:v${VERSION}"
export MIDEN_VALIDATOR_IMAGE="${REGISTRY}/miden-validator:v${VERSION}"
export MIDEN_NTX_BUILDER_IMAGE="${REGISTRY}/miden-ntx-builder:v${VERSION}"
export MIDEN_REMOTE_PROVER_IMAGE="${REGISTRY}/miden-remote-prover:v${VERSION}"
export MIDEN_FUNDING_SERVICE_IMAGE="${REGISTRY}/miden-funding-service:v${VERSION}"
export MIDEN_USDCX_GENESIS_IMAGE="${REGISTRY}/miden-usdcx-genesis:v${VERSION}"

compose() {
    docker compose \
        --project-name "${PROJECT}" \
        --file "${CHECKOUT}/docker-compose.yml" \
        "$@"
}

case "${1:-}" in
    up)
        if [[ ! -d "${CHECKOUT}" ]]; then
            echo "Cloning the node repo at v${VERSION}"
            git clone --depth 1 --branch "v${VERSION}" https://github.com/0xMiden/node "${CHECKOUT}"
        fi
        # Starting the funding service pulls in everything it depends on: the genesis bootstrap,
        # the validators, the sequencer and the prover.
        compose up --detach funding-service
        ;;
    down) compose down --volumes --remove-orphans ;;
    logs) compose logs --no-color ;;
    *)
        echo "usage: $0 up|down|logs" >&2
        exit 2
        ;;
esac
