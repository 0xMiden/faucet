# Miden faucet

Token faucet application for Miden testnet.

## Documentation

For comprehensive guides, API reference, and examples, see the [Miden Faucet Documentation](https://0xmiden.github.io/faucet).

## Running the faucet

The faucet comes with two CLI tools:
- **miden-faucet**: Runs the faucet server.
- **miden-faucet-client**: Used for interacting with a live faucet, i.e. for requesting tokens from a running faucet.

The faucet owns no account and submits no transactions. It validates requests and forwards them to a
[funding service](https://github.com/0xMiden/node), which holds the chain's native asset and creates
a public P2ID note per request. The faucet is the gatekeeper in front of it: the funding service has
no authentication or rate limiting of its own, so only the faucet should be able to reach it.

1. Install both faucet binaries:
```bash
make install-faucet
```

2. Start the faucet, pointing it at the funding service:
```bash
miden-faucet start \
  --funding-service-url http://localhost:50401 \
  --decimals 6 \
  --explorer-url https://testnet.midenscan.com \
  --network testnet
```

`start` reads the funding service's `/status` first and fails if it cannot be reached. It also
fails if `--max-claimable-amount` is larger than the funding service's own maximum. There is no
`init` step: the faucet holds no account.

## Docker

Every release is published as an image tagged with that release's version. Replace `<version>` below with a tag
from the [releases](https://github.com/0xMiden/faucet/releases) page, for example `v0.16.0-rc.1`.

```bash
docker pull ghcr.io/0xmiden/miden-faucet:<version>
```

**Data dir:** the API keys file defaults to `/faucet/api_keys.txt`. Mount a volume at `/faucet` if
you use API keys.

```bash
docker run --rm -p 8000:8000 -p 8080:8080 \
  -v miden-faucet-data:/faucet \
  -e MIDEN_FAUCET_NETWORK=testnet \
  -e MIDEN_FAUCET_NODE_URL=https://rpc.testnet.miden.io \
  -e MIDEN_FAUCET_FUNDING_SERVICE_URL=http://funding-service:50401 \
  -e MIDEN_FAUCET_DECIMALS=6 \
  ghcr.io/0xmiden/miden-faucet:<version>
```

See the [CLI documentation](https://0xmiden.github.io/faucet/getting-started/cli.html) for all options.

## Requesting tokens from a live faucet

You can use the `miden-faucet-client` binary to request tokens from any running faucet instance, whether it's your local faucet or the remote testnet faucet:
```bash
miden-faucet-client mint --url <FAUCET_API_URL> --target-account <ACCOUNT_ID> --amount <BASE_UNITS>
```

After a few seconds you may go to `http://localhost:8080` and see the faucet UI.

## Faucet security features:
The faucet implements several security measures to prevent abuse:

- **Proof of Work requests**:
  - Users must complete a computational challenge before their request is processed.
  - The challenge difficulty increases with the load. The load is measured by the amount of challenges that were submitted but still haven't expired.
  - Each challenge is signed with a secret only known by the server. It should NOT be shared.
  - **Rate limiting**: if an account submitted a challenge, it can't submit another one until the previous one is expired. The challenge lifetime duration is fixed and set when running the faucet.
  - **API Keys**: the faucet is initialized with a set of API Keys that can be distributed to developers. The difficulty of the challenges requested using the API Key will increase only with the load of that key, it won't be influenced by the overall load of the faucet.

- **Claim cap**: each request is capped by `--max-claimable-amount`, which the faucet refuses to
  start with if it is larger than the funding service's own maximum.

## Contributing

Interested in contributing? Check [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

This project is [MIT licensed](./LICENSE).
