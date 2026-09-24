#!/usr/bin/env bash
#
# Downloads the `miden-client` CLI the end-to-end test uses to create the recipient account and to
# read its balance.

set -euo pipefail

BIN=target/e2e/bin/miden-client
# Reads the version out of the line `miden-client = { version = "..." }`.
VERSION=$(grep '^miden-client ' Cargo.toml | cut -d '"' -f 2 | tr -d '=')

if [[ -x "$BIN" ]] && "$BIN" --version | grep -q "$VERSION"; then
    echo "miden-client $VERSION is already at $BIN"
    exit 0
fi

platform="$(uname -s)-$(uname -m)"
case "$platform" in
    Darwin-arm64) asset=miden-client-aarch64-apple-darwin ;;
    Linux-x86_64) asset=miden-client-x86_64-unknown-linux-gnu ;;
    *)
        echo "error: no published miden-client for $platform" >&2
        exit 1
        ;;
esac

url="https://github.com/0xMiden/rust-sdk/releases/download/v$VERSION/$asset"
echo "Downloading $url"
mkdir -p "$(dirname "$BIN")"
curl --location --fail --silent --show-error --output "$BIN" "$url"
chmod +x "$BIN"
"$BIN" --version
