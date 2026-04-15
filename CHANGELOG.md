# Changelog

## 0.15.0 (TBD)

- Reimplemented the transaction script in Rust using the miden-compiler ([#204](https://github.com/0xMiden/faucet/pull/204)).

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
