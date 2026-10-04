import { Utils } from "./utils";

export class UIController {
    constructor() {
        this.recipientInput = document.getElementById('recipient-address');
        this.tokenSelect = document.getElementById('token-amount');
        this.sendButton = document.getElementById('send-button');
        this.walletConnectButton = document.getElementById('wallet-connect-button');
        this.faucetAddress = document.getElementById('faucet-address');
        this.faucetAddressLoader = document.getElementById('faucet-address-loader');
        this.remainingFunds = document.getElementById('remaining-funds');
        this.remainingFundsLoader = document.getElementById('remaining-funds-loader');
        this.tokenAmountHint = document.getElementById('token-amount-hint');
        this.explorerUrl = null;
    }

    setupEventListeners(onSendTokens, onWalletConnect, onTokenSelect) {
        this.sendButton.addEventListener('click', () => onSendTokens());
        this.walletConnectButton.addEventListener('click', onWalletConnect);
        this.tokenSelect.addEventListener('change', (event) => onTokenSelect(event.target.value));
        this.recipientInput.addEventListener('input', () => this.syncSendButton());
        this.syncSendButton();
    }

    // The send button stays inactive until the recipient field holds a valid address.
    syncSendButton() {
        this.sendButton.disabled = !Utils.validateAddress(this.recipientInput.value.trim());
    }

    getFormData() {
        return {
            recipient: this.recipientInput.value.trim(),
            amount: this.tokenSelect.value,
            amountAsTokens: this.tokenSelect[this.tokenSelect.selectedIndex].textContent
        };
    }

    setWalletConnected(address) {
        this.recipientInput.value = address;
        this.recipientInput.disabled = true;
        this.walletConnectButton.disabled = true;
        this.syncSendButton();
    }

    setWalletButtonEnabled(enabled) {
        this.walletConnectButton.disabled = !enabled;
    }

    resetForm() {
        // If wallet is connected, keep the address intact
        if (!this.recipientInput.disabled) {
            this.recipientInput.value = '';
        }
        this.syncSendButton();
    }

    hideModals() {
        const mintingModal = document.getElementById('minting-modal');
        mintingModal.classList.remove('active');

        const completedPublicModal = document.getElementById('completed-public-modal');
        completedPublicModal.classList.remove('active');
    }

    showMintingModal(recipient, amountAsTokens) {
        const modal = document.getElementById('minting-modal');
        const tokenAmount = document.getElementById('modal-token-amount');
        const recipientAddress = document.getElementById('modal-recipient-address');

        // Update modal content
        tokenAmount.textContent = amountAsTokens;
        recipientAddress.textContent = recipient;

        modal.classList.add('active');
    }

    hideMintingModal() {
        const mintingModal = document.getElementById('minting-modal');
        mintingModal.classList.remove('active');
    }

    setupExplorerButton(explorerButton, noteId) {
        if (this.explorerUrl) {
            explorerButton.style.display = 'block';
            explorerButton.onclick = () => window.open(`${this.explorerUrl}/note/${noteId}`, '_blank');
        } else {
            explorerButton.style.display = 'none';
        }
    }

    showCompletedPublicModal(recipient, amountAsTokens, noteId) {
        document.getElementById('completed-public-token-amount').textContent = amountAsTokens;
        document.getElementById('completed-public-recipient-address').textContent = recipient;
        const completedPublicModal = document.getElementById('completed-public-modal');
        completedPublicModal.classList.add('active');

        const publicExplorerButton = document.getElementById('public-explorer-button');
        this.setupExplorerButton(publicExplorerButton, noteId);
        completedPublicModal.onclick = (e) => {
            if (e.target !== publicExplorerButton) {
                this.hideModals();
                this.resetForm();
            }
        };
    }

    showRequestFailedError(title, description) {
        this.showError(title, description);

        const icon = document.getElementById('error-icon');
        icon.style.display = 'block';
    }

    showConnectionError(title, description) {
        this.showError(title, description);

        const icon = document.getElementById('warning-icon');
        icon.style.display = 'block';
    }

    showInvalidRequestError(title, description) {
        this.showError(title, description);

        const icon = document.getElementById('invalid-icon');
        icon.style.display = 'block';
    }

    showWaitError(title, description) {
        this.showError(title, description);

        const icon = document.getElementById('wait-error-icon');
        icon.style.display = 'block';
    }

    showStillLoading(title, description) {
        this.showError(title, description);

        const icon = document.getElementById('wait-icon');
        icon.style.display = 'block';

        const errorMessage = document.getElementById('home-error-message');
        errorMessage.classList.add('pending');
    }

    hideIcons() {
        const warningIcon = document.getElementById('warning-icon');
        warningIcon.style.display = 'none';

        const waitErrorIcon = document.getElementById('wait-error-icon');
        waitErrorIcon.style.display = 'none';

        const waitIcon = document.getElementById('wait-icon');
        waitIcon.style.display = 'none';

        const invalidIcon = document.getElementById('invalid-icon');
        invalidIcon.style.display = 'none';

        const errorIcon = document.getElementById('error-icon');
        errorIcon.style.display = 'none';
    }

    showError(title, description) {
        this.hideIcons();

        const errorTitle = document.getElementById('home-error-message-title');
        errorTitle.textContent = title;

        const errorDescription = document.getElementById('home-error-message-description');
        errorDescription.textContent = description;

        const errorMessage = document.getElementById('home-error-message');
        errorMessage.classList.remove('pending');
        errorMessage.classList.add('visible');
    }

    hideErrors() {
        this.hideIcons();

        const errorMessage = document.getElementById('home-error-message');
        errorMessage.classList.remove('visible');
        errorMessage.classList.remove('pending');
    }

    setTokenHint(estimatedTime) {
        this.tokenAmountHint.textContent = `Larger amounts take more time to mint. Estimated: ${estimatedTime}`;
    }

    setTokenOptions(tokenAmountOptions, decimals) {
        this.tokenSelect.innerHTML = '';
        for (const amount of tokenAmountOptions) {
            const option = document.createElement('option');
            const baseUnits = Utils.tokensToBaseUnits(amount, decimals);
            option.value = baseUnits;
            option.textContent = amount;
            this.tokenSelect.appendChild(option);
        }
        this.tokenSelect.disabled = false;
    }

    setFaucetId(id) {
        this.faucetAddress.textContent = id;
        this.faucetAddressLoader.hidden = true;
        this.faucetAddress.hidden = false;
    }

    /// The funding service reports no balance when it cannot be reached, which shows as "-".
    setRemainingFunds(balance, decimals) {
        this.remainingFunds.textContent =
            balance == null ? '-' : Utils.baseUnitsToTokens(balance, decimals);
        this.remainingFundsLoader.hidden = true;
        this.remainingFunds.hidden = false;
    }

    setExplorerUrl(url) {
        this.explorerUrl = url;
    }

    // Swap the loading placeholders for the "-" placeholders when the data can't be loaded.
    showFooterPlaceholders() {
        this.faucetAddressLoader.hidden = true;
        this.faucetAddress.hidden = false;
        this.remainingFundsLoader.hidden = true;
        this.remainingFunds.hidden = false;
        // The token select is still showing its "Loading…" placeholder if the options never came.
        if (this.tokenSelect.disabled && this.tokenSelect.options.length > 0) {
            this.tokenSelect.options[0].textContent = '-';
        }
    }
}
