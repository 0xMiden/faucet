import { MidenWalletAdapter } from "@miden-sdk/miden-wallet-adapter-miden";
import { PrivateDataPermission, WalletAdapterNetwork, WalletReadyState } from "@miden-sdk/miden-wallet-adapter-base";
import { Endpoint, NoteId, RpcClient, getWasmOrThrow } from "@miden-sdk/miden-sdk/lazy";
import { Utils } from './utils.js';
import { UIController } from './ui.js';
import { getConfig, getMetadata, getPowChallenge, getTokens } from "./api.js";

const SECOND = 1000;
const MINUTE = 60 * SECOND;

export class MidenFaucetApp {
    constructor() {
        this.ui = new UIController();
        this.tokenAmountOptions = [100, 500, 1000];
        this.metadataInitialized = false;
        this.apiUrl = null;
        this.nodeUrl = null;
        this.rpcClient = null;
        this.wasmReady = null;
        this.baseAmount = null;
        this.powLoadDifficulty = null;

        // Check if Web Crypto API is available
        if (!window.crypto || !window.crypto.subtle) {
            console.error("Web Crypto API not available");
            this.ui.showConnectionError('Web Crypto API not available', 'Please use a modern browser');
            return;
        }

        this.walletAdapter = new MidenWalletAdapter({ appName: 'Miden Faucet' });

        this.init();
    }

    async init() {
        try {
            const config = await getConfig();
            this.apiUrl = config.api_url;
            this.nodeUrl = config.node_url;
            this.setupEventListeners();
            this.setupWalletDetection();
            this.startMetadataPolling();

            // The WASM SDK is only needed to poll for notes after a mint.
            // Defer the download until the page has fully loaded so it
            // doesn't compete with the page assets for bandwidth. A mint
            // request triggers it on demand if it hasn't started yet.
            const warmUpWasm = () => this.ensureWasmLoaded().catch((error) => {
                console.warn('Background WASM load failed, will retry on demand:', error);
            });
            if (document.readyState === 'complete') {
                warmUpWasm();
            } else {
                window.addEventListener('load', warmUpWasm, { once: true });
            }
        } catch (error) {
            console.error('Failed to initialize app:', error);
            this.ui.showFooterPlaceholders();
            this.handleApiError(error, 'Connection failed', 'Some data couldn\'t be loaded right now.');
        }
    }

    ensureWasmLoaded() {
        if (!this.wasmReady) {
            this.wasmReady = getWasmOrThrow().then(() => {
                this.rpcClient = new RpcClient(new Endpoint(this.nodeUrl));
            }).catch((error) => {
                // Clear the cached promise so a later call retries the load.
                this.wasmReady = null;
                throw error;
            });
        }
        return this.wasmReady;
    }

    setupWalletDetection() {
        // Enable button if wallet extension is already installed
        if (this.walletAdapter.readyState === WalletReadyState.Installed) {
            this.ui.setWalletButtonEnabled(true);
        }

        // Listen for future readyState changes
        this.walletAdapter.on('readyStateChange', (readyState) => {
            if (!this.walletAdapter.connected) {
                this.ui.setWalletButtonEnabled(readyState === WalletReadyState.Installed);
            }
        });
    }

    setupEventListeners() {
        const onSendTokens = () => this.handleSendTokens();
        const onWalletConnect = () => this.handleWalletButtonClick();
        const onTokenSelect = (requestedAmount) => this.updateTokenHint(requestedAmount);
        this.ui.setupEventListeners(onSendTokens, onWalletConnect, onTokenSelect);
    }

    async connectWallet() {
        try {
            await this.walletAdapter.connect(PrivateDataPermission.UponRequest, WalletAdapterNetwork.Testnet);
            return true;
        } catch (error) {
            console.error("WalletConnectionError:", error);
            return false;
        }
    }

    async handleWalletButtonClick() {
        this.ui.setWalletButtonEnabled(false);
        try {
            const connected = await this.connectWallet();
            if (connected && this.walletAdapter.address) {
                this.ui.setWalletConnected(this.walletAdapter.address);
            } else {
                this.ui.showConnectionError("Connection failed", "Failed to connect wallet.");
                this.ui.setWalletButtonEnabled(true);
            }
        } catch (error) {
            this.ui.setWalletButtonEnabled(true);
        }
    }

    async handleSendTokens() {
        try {
            const { recipient, amount, amountAsTokens } = this.ui.getFormData();

            if (!recipient) {
                this.ui.showInvalidRequestError('Invalid address', 'Please enter a recipient address.');
                return;
            }
            if (!amount || amount === '0') {
                this.ui.showInvalidRequestError('Invalid amount', 'Please enter a non zero amount.');
                return;
            }
            if (!Utils.validateAddress(recipient)) {
                this.ui.showInvalidRequestError('Invalid address', 'Please enter a valid recipient address.');
                return;
            }

            this.ui.hideErrors();
            this.ui.showMintingModal(recipient, amountAsTokens);

            const powData = await getPowChallenge(this.apiUrl, recipient, amount);
            const nonce = await this.findValidNonce(powData.challenge, powData.target);

            const getTokensResponse = await getTokens(this.apiUrl, powData.challenge, nonce, recipient, amount);

            await this.pollNote(getTokensResponse.note_id);

            this.ui.showCompletedPublicModal(recipient, amountAsTokens, getTokensResponse.tx_id);

            // The funding account just paid out, so refresh what it has left.
            this.fetchMetadata().catch((error) => {
                console.warn('Could not refresh the remaining funds:', error);
            });
        } catch (error) {
            this.ui.hideMintingModal();
            this.handleApiError(error, 'Request failed', error.message);
            return;
        }
    }

    startMetadataPolling() {
        this.fetchMetadata().catch((error) => {
            this.ui.showFooterPlaceholders();
            this.ui.showConnectionError('Connection failed', 'Some data couldn\'t be loaded right now.');
            console.error('Error fetching metadata:', error);
        });
    }

    async fetchMetadata() {
        const data = await getMetadata(this.apiUrl);

        this.decimals = data.decimals;
        this.powLoadDifficulty = data.pow_load_difficulty;
        this.baseAmount = data.base_amount;
        this.ui.setRemainingFunds(data.balance, data.decimals);

        if (!this.metadataInitialized) {
            this.metadataInitialized = true;
            this.ui.setFaucetId(data.id);
            this.ui.setExplorerUrl(data.explorer_url);
            this.ui.setTokenOptions(this.tokenAmountOptions, data.decimals);
            this.updateTokenHint(this.tokenAmountOptions[0]);
        }
    }

    updateTokenHint(requestedAmount) {
        const estimatedTime = this.computePowTimeEstimation(requestedAmount, this.baseAmount, this.powLoadDifficulty);
        this.ui.setTokenHint(estimatedTime);
    }

    computePowTimeEstimation(requestedAmount, baseAmount, loadDifficulty) {
        const requestComplexity =
            Math.floor(requestedAmount / Number(baseAmount)) + 1;
        const difficulty = requestComplexity * Number(loadDifficulty);
        const difficultyBits = Math.log2(difficulty);

        let estimatedTime;
        if (difficultyBits <= 17) {
            estimatedTime = `<5s`;
        } else if (difficultyBits <= 18) {
            estimatedTime = `5-15s`;
        } else if (difficultyBits <= 19) {
            estimatedTime = `15-30s`;
        } else if (difficultyBits <= 20) {
            estimatedTime = `30s-1m`;
        } else if (difficultyBits <= 21) {
            estimatedTime = `1-5m`;
        } else {
            estimatedTime = `5m+`;
        }

        return estimatedTime;
    }

    pollNote(noteId) {
        return new Promise((resolve, reject) => {
            const start = Date.now();

            const tick = async () => {
                // bail if we already timed out
                if (Date.now() - start >= 5 * MINUTE) {
                    return reject(new Error('Timeout while waiting for tx to be committed. Please try again later.'));
                }

                try {
                    await this.ensureWasmLoaded();
                    const note = await this.rpcClient.getNotesById([NoteId.fromHex(noteId)]);
                    if (note && note.length > 0) {
                        return resolve();
                    }
                } catch (err) {
                    console.error('Error polling for note:', err);
                    this.ui.showConnectionError('Connection failed', 'Could not fetch note confirmation. Retrying...');
                }

                // choose next delay: 0.5s up to 10s, then 1s up to 40s, then 5s
                const elapsed = Date.now() - start;
                const nextDelay =
                    elapsed <= 10 * SECOND ? 0.5 * SECOND :
                        elapsed <= 40 * SECOND ? 1 * SECOND :
                            5 * SECOND;

                setTimeout(() => tick(), nextDelay);
            };

            tick();
        });
    }

    async findValidNonce(challenge, target) {
        let nonce = 0;
        let targetNum = BigInt(target);
        const challengeBytes = Utils.fromHex(challenge);

        while (true) {
            nonce = Math.floor(Math.random() * Number.MAX_SAFE_INTEGER);

            try {
                // Convert nonce to 8-byte big-endian format to match backend
                const nonceBytes = new ArrayBuffer(8);
                const nonceView = new DataView(nonceBytes);
                nonceView.setBigUint64(0, BigInt(nonce), false); // false = big-endian
                const nonceByteArray = new Uint8Array(nonceBytes);

                // Combine challenge and nonce
                const combined = new Uint8Array(challengeBytes.length + nonceByteArray.length);
                combined.set(challengeBytes);
                combined.set(nonceByteArray, challengeBytes.length);

                // Compute SHA-256 hash using Web Crypto API
                const hashBuffer = await window.crypto.subtle.digest('SHA-256', combined);
                const hashArray = new Uint8Array(hashBuffer);

                // Take the first 8 bytes of the hash and parse them as u64 in big-endian
                const first8Bytes = hashArray.slice(0, 8);
                const dataView = new DataView(first8Bytes.buffer);
                const digest = dataView.getBigUint64(0, false); // false = big-endian

                // Check if the hash is less than the target
                if (digest < targetNum) {
                    return nonce;
                }
            } catch (error) {
                console.error('Error computing hash:', error);
                throw new Error('Failed to compute hash: ' + error.message);
            }

            // Yield to browser to prevent freezing
            if (nonce % 1000 === 0) {
                await new Promise(resolve => setTimeout(resolve, 0));
            }
        }
    }

    handleApiError(error, defaultTitle, defaultMessage) {
        // Check if it's an ApiError with a status code
        if (error.statusCode) {
            const statusCode = error.statusCode;
            const errorMessage = error.message || defaultMessage;

            switch (statusCode) {
                case 400: // Bad Request
                    this.ui.showInvalidRequestError('Invalid request', errorMessage);
                    break;
                case 429: // Too Many Requests (Rate Limited)
                    this.ui.showWaitError('Error!', errorMessage || 'Too many requests.');
                    break;
                case 500: // Internal Server Error
                    this.ui.showConnectionError('Server error', errorMessage || 'An internal server error occurred.');
                    break;
                case 503: // Service Unavailable
                    this.ui.showConnectionError('Service unavailable', errorMessage || 'The faucet is currently unavailable.');
                    break;
                default:
                    // For other status codes, use the default error handler
                    this.ui.showRequestFailedError(defaultTitle, errorMessage);
            }
        } else {
            // For non-API errors (e.g. network errors), use default handler
            this.ui.showRequestFailedError(defaultTitle, defaultMessage);
        }
    }
}
