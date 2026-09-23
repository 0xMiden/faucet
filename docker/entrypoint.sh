#!/bin/sh
set -e
set -u

# Default to 'start' command if no arguments provided
if [ "$#" -eq 0 ]; then
  set -- start
fi

# Data lives at /faucet by default; override via MIDEN_FAUCET_API_KEYS
: "${MIDEN_FAUCET_API_KEYS:=/faucet/api_keys.txt}"

# Ensure the data directory exists
DATA_DIR="$(dirname "${MIDEN_FAUCET_API_KEYS}")"
mkdir -p "${DATA_DIR}"

cd "${DATA_DIR}" || exit 1
exec miden-faucet "$@"
