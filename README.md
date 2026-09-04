# Miden faucet

Token faucet application for Miden testnet.

## Documentation

For comprehensive guides, API reference, and examples, see the [Miden Faucet Documentation](https://0xmiden.github.io/faucet).

## Running the faucet

The faucet comes with two CLI tools:
- **miden-faucet**: Runs the faucet, used for initializing and starting the faucet.
- **miden-faucet-client**: Used for interacting with a live faucet, i.e. for requesting tokens from a running faucet.

1. Install both faucet binaries:
```bash
make install-faucet
```

2. Initialize the faucet server. This will generate an new operator account and a network faucet account with the specified token configuration, and save the account data to a local SQLite store:

```bash
miden-faucet init \
  --token-symbol MIDEN \
  --decimals 6 \
  --max-supply 100000000000000000 \
  --network testnet
```

> [!TIP]
> `miden-faucet init` can also be run with existing operator and faucet accounts:
>
> ```bash
> miden-faucet init \
>   --import /path/to/operator_account.mac \
>   --faucet-account-id 0x<faucet_account_id> \
>   --network testnet
> ```
>
> - `--import` — path to an exported operator account file
> - `--faucet-account-id` — ID of a faucet account that already exists on the target network

3. Start the faucet:
```bash
miden-faucet start \
  --explorer-url https://testnet.midenscan.com \
  --network testnet
```

## Running on a chain that charges fees (devnet)

On a chain that charges transaction fees, every transaction pays in the chain's native asset out of
the executing account's vault. The operator account is the one that pays: it covers each MINT
transaction and prepays the network transaction that turns the MINT note into the P2ID note, so it
must be funded with the native asset and topped up as its balance drains. `start` refuses to run
while the operator holds none of it, and requests are answered with HTTP 503 while its balance is
too low to cover a transaction.

`init` cannot create a new faucet account on such a chain, since the account would have to pay for
its own deployment out of an empty vault. Import an existing one with `--import` and
`--faucet-account-id`.

On devnet the operator is funded at genesis, so no manual funding is needed to get started. The
faucet account id is the one `miden-validator genesis` prints when the network is bootstrapped:

```bash
miden-faucet init \
  --import faucet_operator.mac \
  --faucet-account-id 0x<faucet_account_id> \
  --network devnet

miden-faucet start --network devnet --remote-tx-prover-url https://tx-prover.devnet.miden.io
```

## Docker

Every release is published as an image tagged with that release's version. Replace `<version>` below with a tag
from the [releases](https://github.com/0xMiden/faucet/releases) page, for example `v0.16.0-rc.1`.

```bash
docker pull ghcr.io/0xmiden/miden-faucet:<version>
```

**Data dir:** Store defaults to `/faucet/store.sqlite`. Mount a volume at `/faucet` for persistence.

Run `init` first, then `start`.

**1. Init — new account (testnet):**

```bash
docker run --rm -v miden-faucet-data:/faucet \
  -e MIDEN_FAUCET_NETWORK=testnet \
  -e MIDEN_FAUCET_NODE_URL=https://rpc.testnet.miden.io \
  -e MIDEN_FAUCET_TOKEN_SYMBOL=MIDEN \
  -e MIDEN_FAUCET_DECIMALS=6 \
  -e MIDEN_FAUCET_MAX_SUPPLY=100000000000000000 \
  ghcr.io/0xmiden/miden-faucet:<version> init
```

**2. Init — import existing account:**

```bash
docker run --rm -v miden-faucet-data:/faucet \
  -e MIDEN_FAUCET_NETWORK=testnet \
  -e MIDEN_FAUCET_NODE_URL=https://rpc.testnet.miden.io \
  -e MIDEN_FAUCET_IMPORT_OPERATOR_ACCOUNT_PATH=/faucet/accounts/faucet_operator_miden.mac \
  -e MIDEN_FAUCET_FAUCET_ACCOUNT_ID=<FAUCET_ACCOUNT_ID> \
  -v /path/to/your/accounts:/faucet/accounts:ro \
  ghcr.io/0xmiden/miden-faucet:<version> init
```

Put `faucet_miden.mac` in your local `./accounts` dir before running.

**3. Start the faucet:**

```bash
docker run --rm -p 8000:8000 -p 8080:8080 \
  -v miden-faucet-data:/faucet \
  ghcr.io/0xmiden/miden-faucet:<version>
```

See `bin/faucet/.env.example` for all options. Copy it to `bin/faucet/.env` and
fill in your values — `.env` is git-ignored, which `.env.example` is not, so keep
real secrets such as `MIDEN_FAUCET_POW_SECRET` out of the template.

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

- **Requests batching**:
  - Maximum batch size: 256 requests
  - Requests are processed in batches to optimize performance
  - Failed requests within a batch are handled individually

## Contributing

Interested in contributing? Check [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

This project is [MIT licensed](./LICENSE).
