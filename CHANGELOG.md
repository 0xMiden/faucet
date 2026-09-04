# Changelog

## 0.16.0 (TBD)

- The faucet mints assets to the operator when its balance is below a threshold. The notes that fund the operator account (P2ID and P2IDE notes carrying the chain's native asset) are consumed in the mint transactions ([#292](https://github.com/0xMiden/faucet/pull/292)).
- Updated miden-client dependency to v0.16.0-rc.4 (miden-protocol / miden-standards / miden-testing v0.16.0-rc.9) and `miden-node-proto-build` to v0.16.0-rc.5, bumped the workspace version to 0.16.0-rc.3, and updated the declared `rust-version` and the Docker builder image to 1.98 ([#293](https://github.com/0xMiden/faucet/pull/293)).
- Updated the frontend `@miden-sdk/miden-sdk` and wallet adapter dependencies to `v0.16.0-rc.6` ([#293](https://github.com/0xMiden/faucet/pull/293)).
- Updated miden-client dependency to v0.16.0-rc.3 (miden-protocol / miden-standards / miden-testing v0.16.0-rc.6) and `miden-node-proto-build` to v0.16.0-rc.2, and bumped the workspace version to 0.16.0-rc.2 ([#286](https://github.com/0xMiden/faucet/pull/286)).
- The operator now attaches a `FEE_SPONSORSHIP` note to every MINT note on a fee-charging chain, prepaying the network transaction that consumes it, so the faucet no longer needs a funded vault. Sponsorships are only attached for a faucet that collects fees in the chain's native fee asset ([#290](https://github.com/0xMiden/faucet/pull/290)).
- Added fee support for chains with a non-zero `verification_base_fee`: the operator's MINT transaction commits native fee conversion info (rate 1/1) through its auth args, declares the faucet as a foreign account so the MINT note pricing FPI is served in one RPC call, and the faucet checks the operator's fee asset balance at startup and before each batch (requests are rejected with HTTP 503 while the operator cannot pay) ([#286](https://github.com/0xMiden/faucet/pull/286)).
- [BREAKING] `init` refuses to create a new faucet account on a fee-charging chain, since the account cannot pay for its own deployment and nothing sponsors a deployment transaction; import an existing faucet account instead ([#286](https://github.com/0xMiden/faucet/pull/286)).
- `api-key remove` now fails when the key is not present in the store instead of reporting a removal ([#286](https://github.com/0xMiden/faucet/pull/286)).
- Updated miden-client dependency to v0.16.0-rc.1 ([#285](https://github.com/0xMiden/faucet/pull/285)). 
- Migrate faucet to using a network account ([#262](https://github.com/0xMiden/faucet/pull/262)).
- [BREAKING] `init --import` now takes an operator account file instead of a faucet account file, and requires the new `--faucet-account-id` param ([#262](https://github.com/0xMiden/faucet/pull/262)).
- [BREAKING] Renamed the `MIDEN_FAUCET_IMPORT_ACCOUNT_PATH` env var to `MIDEN_FAUCET_IMPORT_OPERATOR_ACCOUNT_PATH` ([#262](https://github.com/0xMiden/faucet/pull/262)).
- [BREAKING] Removed the `--deploy` param and `MIDEN_FAUCET_DEPLOY` env var; a newly created faucet account is always deployed ([#262](https://github.com/0xMiden/faucet/pull/262)).
- Improved the faucet logging ([#267](https://github.com/0xMiden/faucet/pull/267)).
- Improved mint failure observability: emit the full error chain in the `faucet.mint` span by recording errors with their `Debug` implementation ([#267](https://github.com/0xMiden/faucet/pull/267)).
- Fixed a one second window in which an already solved `PoW` challenge could be redeemed a second time, because a challenge was still considered valid at the exact moment its solver stopped being rate limited ([#275](https://github.com/0xMiden/faucet/pull/275)).
- Updated `rand` to v0.10 and removed the direct `rand_chacha` dependency and updated the declared `rust-version` to `1.96.1` ([#284](https://github.com/0xMiden/faucet/pull/284)).
- Improved frontend loading: the SDK WASM download is deferred until the page finishes loading and served brotli pre-compressed, the JS bundle is minified, the header image was converted to lossless WebP, and all static assets are served with cache headers ([#288](https://github.com/0xMiden/faucet/pull/288)).
- Fixed the issuance counter staying empty until the next mint, the value is now cached and rendered once the metadata arrives.
- Added loading spinners for the footer values (faucet address, tokens claimed) and disabled the token amount selector until its options load ([#288](https://github.com/0xMiden/faucet/pull/288)).
- The frontend `/config.json` endpoint now returns a JSON object instead of a double-encoded JSON string ([#288](https://github.com/0xMiden/faucet/pull/288)).

## 0.16.0-alpha.1 (2026-07-20)

- Updated miden-client dependency to v0.16.0-alpha.1 ([#259](https://github.com/0xMiden/faucet/pull/259))

## 0.15.1 (2026-06-15)

- Fixed faucet minting to stamp generated assets with the callback flag derived from the faucet account.

## 0.15.0 (2026-06-12)

- Added `note_transport_url` field to the `/get_metadata` endpoint response ([#243](https://github.com/0xMiden/faucet/pull/243)).
- Generate a random PoW secret at startup when `--pow-secret` parameter is unset ([#251](https://github.com/0xMiden/faucet/pull/251)).

## 0.14.3 (2026-04-29)

- Updated miden-client dependency to v0.14.5 ([#244](https://github.com/0xMiden/faucet/pull/244)).
- Improved mint failure observability: each step inside `submit_new_transaction` now records its own error, `apply_transaction` is instrumented as a sibling span, and `RpcError` propagations record structured `grpc.endpoint`/`grpc.code`/`grpc.endpoint_error` fields on the parent span ([#245](https://github.com/0xMiden/faucet/pull/245)).

## 0.14.2 (2026-04-21)

- Fixed faucet state sync to request storage map details for tracked public accounts ([#241](https://github.com/0xMiden/faucet/pull/241)).

## 0.14.1 (2026-04-16)

- Updated miden-client dependency to v0.14.3 ([#239](https://github.com/0xMiden/faucet/pull/239)).

## 0.14.0 (2026-04-08)

- [BREAKING] Removed `--api-key` param from the `start` command, API keys are now persisted in the store and automatically loaded on startup.  ([#225](https://github.com/0xMiden/miden-faucet/pull/225)).
- [BREAKING] Added `api-key` CLI command with `list`/`remove`/`create` subcommands ([#225](https://github.com/0xMiden/miden-faucet/pull/225)).
- Replaced metadata polling with Server-Sent Events (SSE) for issuance updates ([#224](https://github.com/0xMiden/miden-faucet/pull/224)).
- OpenTelemetry traces are now flushed before program termination on panic ([#222](https://github.com/0xMiden/miden-faucet/pull/222)).

## 0.13.1 (2026-02-18)

- Replaced hidden wallet icon with explicit "Connect Wallet" / "Disconnect" button next to recipient address input ([#228](https://github.com/0xMiden/miden-faucet/pull/228)).
- Removed automatic wallet connection popup during private note minting ([#228](https://github.com/0xMiden/miden-faucet/pull/228)).
- Fixed modal content border rendering inconsistency caused by subpixel border ([#228](https://github.com/0xMiden/miden-faucet/pull/228)).

## 0.13.0 (2026-01-28)

- Added web-client to wait for note commitment on the node ([#174](https://github.com/0xMiden/miden-faucet/pull/174)).
- [BREAKING] Replaced the `api-bind-url` param for `api-bind-port` ([#156](https://github.com/0xMiden/miden-faucet/pull/156)).
- [BREAKING] Replaced the `frontend-url` param for `frontend-bind-port` ([#156](https://github.com/0xMiden/miden-faucet/pull/156)).
- [BREAKING] Added `no-frontend` param to optionally disable the frontend server ([#156](https://github.com/0xMiden/miden-faucet/pull/156)).
- Redesigned the frontend ([#201](https://github.com/0xMiden/miden-faucet/pull/201)).
- [BREAKING] Added `note-transport-url` CLI param to set the note transport layer ([#191](https://github.com/0xMiden/miden-faucet/pull/191)).
- Updated faucet optional deployment to use an empty transaction (#[182](https://github.com/0xMiden/miden-faucet/pull/182)).
- Added a feature `tokio` for the `miden-pow-rate-limiter` crate ([#188](https://github.com/0xMiden/miden-faucet/pull/188).)
- Added integration to import mint private notes directly to the wallet ([#189](https://github.com/0xMiden/miden-faucet/pull/189)).
- Added a new `miden-faucet-client` binary with the `mint` command ([#196](https://github.com/0xMiden/miden-faucet/pull/196), [#215](https://github.com/0xMiden/miden-faucet/pull/215)). 

## 0.12.4 (2025-12-04)

- Added version to the metadata endpoint ([#169](https://github.com/0xMiden/miden-faucet/pull/169)).
- Small UI improvements ([#180](https://github.com/0xMiden/miden-faucet/pull/180)).
- Updated `miden-client` to v0.12.5 ([#186](https://github.com/0xMiden/miden-faucet/pull/186)).

## 0.12.3 (2025-11-17)

- Fixed challenge cache lock poisoned bug by removing validation on `challenges_timestamps` cleanup ([#165](https://github.com/0xMiden/miden-faucet/pull/165)).
- Added an error display for failed metadata requests ([#166](https://github.com/0xMiden/miden-faucet/pull/166)).
- Improved rate limiter by tracking challenges by submission timestamp ([#167](https://github.com/0xMiden/miden-faucet/pull/167)).

## 0.12.2 (2025-11-12)

- Removed the web-client to improve frontend loading time ([#158](https://github.com/0xMiden/miden-faucet/pull/158)).

## 0.12.1 (2025-11-11)

- Fixed release workflow by storing frontend artifacts in `OUT_DIR` (([#154](https://github.com/0xMiden/miden-faucet/pull/154))).
- Added `api-public-url` CLI param to fix the backend and frontend communication ([#153](https://github.com/0xMiden/miden-faucet/pull/153)).

## 0.12.0 (2025-11-10)

- Added requested `amount` to PoW Challenge ([#68](https://github.com/0xMiden/miden-faucet/pull/68)).
- Added `pow_base_difficulty_amount` CLI param and updated default values ([#68](https://github.com/0xMiden/miden-faucet/pull/68)).
- Added Retry-After header for HTTP response on rate limited error ([#70](https://github.com/0xMiden/miden-faucet/pull/70)).
- Replaced SHA3-256 with SHA-256 for PoW ([#79](https://github.com/0xMiden/miden-faucet/pull/79)).
- Refactored CLI commands into `init` and `start` ([#84](https://github.com/0xMiden/miden-faucet/pull/84), [#145](https://github.com/0xMiden/miden-faucet/pull/145)).
- Added wallet connection to prefill the recipient address ([#100](https://github.com/0xMiden/miden-faucet/pull/100)).
- Redesigned the frontend ([#110](https://github.com/0xMiden/miden-faucet/pull/110)).
- Separated frontend and backend servers ([#119](https://github.com/0xMiden/miden-faucet/pull/119)).
- Frontend now awaits for the transaction to be committed ([#127](https://github.com/0xMiden/miden-faucet/pull/127)).
- Added 10-block expiration delta to faucet minting transactions ([#136](https://github.com/0xMiden/miden-faucet/pull/136)).

## 0.11.8 (2025-10-27)

- Sync state before creating transactions to avoid desync errors (#[132](https://github.com/0xMiden/miden-faucet/pull/132)).

## 0.11.7 (2025-10-15)

- Patched miden-client to 0.11.10 ([#121](https://github.com/0xMiden/miden-faucet/pull/121)).

## 0.11.6 (2025-10-08)

- Improved telemetry ([#111](https://github.com/0xMiden/miden-faucet/pull/111)).

## 0.11.5 (2025-09-18)

- Patched miden-client to 0.11.6 ([#90](https://github.com/0xMiden/miden-faucet/pull/90)).
- Set batch size to 64 ([#90](https://github.com/0xMiden/miden-faucet/pull/90)).

## 0.11.4 (2025-09-16)

- Reduce faucet batch size to 8 ([#87](https://github.com/0xMiden/miden-faucet/pull/87)).

## 0.11.3 (2025-09-09)

- Fixed display for minted token amount ([#82](https://github.com/0xMiden/miden-faucet/pull/82)).

## 0.11.2 (2025-09-08)

- Refreshed dependencies.

## 0.11.1 (2025-09-02)

- Added `--network` CLI parameter to specify the type of network to which the faucet connects ([#74](https://github.com/0xMiden/miden-faucet/pull/74)).

## 0.11.0 (2025-09-01)

### Changes

- Introduced `miden-faucet-lib` crate ([#10](https://github.com/0xMiden/miden-faucet/pull/10)).
- Integrated miden-client ([#11](https://github.com/0xMiden/miden-faucet/pull/11)).
- Added `/get_note` endpoint ([#19](https://github.com/0xMiden/miden-faucet/pull/19)).
- Redesigned the home frontend ([#20](https://github.com/0xMiden/miden-faucet/pull/20)).
- Redesigned the tokens request flows ([#25](https://github.com/0xMiden/miden-faucet/pull/25)).
- Added faucet supply amounts to the metadata ([#30](https://github.com/0xMiden/miden-faucet/pull/30)).
- Added supply exceeded check ([#31](https://github.com/0xMiden/miden-faucet/pull/31)).
- Use HTTP 429 status code for rate limited error ([#51](https://github.com/0xMiden/miden-faucet/pull/51)).
- Replace amount options validation for maximum claimable amount ([#52](https://github.com/0xMiden/miden-faucet/pull/52)).
- Added `mdbook` documentation ([#61](https://github.com/0xMiden/miden-faucet/pull/61)).
- Added `--explorer-url` CLI parameter to optionally set the explorer url ([#63](https://github.com/0xMiden/miden-faucet/pull/63)).
- Introduced `miden-pow-rate-limiter` crate ([#67](https://github.com/0xMiden/miden-faucet/pull/67))
- [BREAKING] Incremented MSRV to 1.89.

## 0.10.0 (2025-07-10)

For previous changes see the changelog in https://github.com/0xMiden/miden-node.
