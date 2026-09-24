#!/usr/bin/env bash
#
# Downloads the `miden-client` CLI the end-to-end test uses to create the recipient account and to
# read its balance.
#
# The release assets are prebuilt, so this takes a couple of seconds instead of the several minutes
# `cargo install miden-client-cli` needs. The version comes from the `miden-client` pin in the
# workspace `Cargo.toml`, so bumping that dependency moves this too.

set -euo pipefail

WORK_DIR="${WORK_DIR:-target/e2e}"
MIDEN_CLIENT_BIN="${MIDEN_CLIENT_BIN:-${WORK_DIR}/bin/miden-client}"
VERSION="${MIDEN_CLIENT_VERSION:-$(sed -n 's/^miden-client *= *{ *version *= *"=\{0,1\}\([^"]*\)".*/\1/p' Cargo.toml)}"

if [[ -z "${VERSION}" ]]; then
    echo "error: could not read the miden-client version from Cargo.toml" >&2
    exit 1
fi

if [[ -x "${MIDEN_CLIENT_BIN}" ]] && "${MIDEN_CLIENT_BIN}" --version 2>/dev/null | grep -q "${VERSION}"; then
    echo "miden-client ${VERSION} is already at ${MIDEN_CLIENT_BIN}"
    exit 0
fi

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) asset="miden-client-aarch64-apple-darwin" ;;
    Linux-x86_64) asset="miden-client-x86_64-unknown-linux-gnu" ;;
    *)
        echo "error: no published miden-client for $(uname -s)-$(uname -m)" >&2
        exit 1
        ;;
esac

url="https://github.com/0xMiden/rust-sdk/releases/download/v${VERSION}/${asset}"
echo "Downloading ${url}"

mkdir -p "$(dirname "${MIDEN_CLIENT_BIN}")"
curl --location --fail --silent --show-error --output "${MIDEN_CLIENT_BIN}" "${url}"
chmod +x "${MIDEN_CLIENT_BIN}"

"${MIDEN_CLIENT_BIN}" --version
