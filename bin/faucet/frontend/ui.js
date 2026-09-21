import { Utils } from "./utils";

export class UIController {
    constructor() {
        this.recipientInput = document.getElementById('recipient-address');
        this.tokenSelect = document.getElementById('token-amount');
        this.stage = document.getElementById('stage');
        this.publicTab = document.getElementById('tab-public');
        this.privateTab = document.getElementById('tab-private');
        this.noteTypeNote = document.getElementById('note-type-note');
        this.sendButton = document.getElementById('send-button');
        this.sendButtonLabel = document.getElementById('send-button-label');
        this.isPrivateNote = true;
        this.walletConnectButton = document.getElementById('wallet-connect-button');
        this.faucetAddress = document.getElementById('faucet-address');
        this.faucetAddressLoader = document.getElementById('faucet-address-loader');
        this.issuanceFill = document.getElementById('issuance-fill');
        this.issuance = document.getElementById('issuance');
        this.issuanceLoader = document.getElementById('issuance-loader');
        this.issuanceValues = document.getElementById('issuance-values');
        this.tokensSupply = document.getElementById('tokens-supply');
        this.tokenAmountHint = document.getElementById('token-amount-hint');
        this.explorerUrl = null;
    }

    setupEventListeners(onSendTokens, onWalletConnect, onTokenSelect) {
        this.publicTab.addEventListener('click', () => this.setNoteType(false));
        this.privateTab.addEventListener('click', () => this.setNoteType(true));
        this.sendButton.addEventListener('click', () => onSendTokens(this.isPrivateNote));
        this.walletConnectButton.addEventListener('click', onWalletConnect);
        this.tokenSelect.addEventListener('change', (event) => onTokenSelect(event.target.value));
        this.recipientInput.addEventListener('input', () => this.syncSendButton());
        this.syncSendButton();
    }

    // The send button stays inactive until the recipient field holds a valid address.
    syncSendButton() {
        this.sendButton.disabled = !Utils.validateAddress(this.recipientInput.value.trim());
    }

    // Switches the public / private toggle. The send button label and the note under the toggle follow it.
    setNoteType(isPrivateNote) {
        this.isPrivateNote = isPrivateNote;
        this.stage.dataset.noteType = isPrivateNote ? 'private' : 'public';
        this.publicTab.setAttribute('aria-selected', String(!isPrivateNote));
        this.privateTab.setAttribute('aria-selected', String(isPrivateNote));
        this.sendButtonLabel.textContent = isPrivateNote ? 'Send private note' : 'Send public note';
        this.noteTypeNote.textContent = isPrivateNote
            ? 'Private note data is not visible on-chain.'
            : 'The note and its amount are visible to anyone on-chain.';
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

        const completedPrivateModal = document.getElementById('completed-private-modal');
        completedPrivateModal.classList.remove('active');

        const completedPublicModal = document.getElementById('completed-public-modal');
        completedPublicModal.classList.remove('active');
    }

    showMintingModal(recipient, amountAsTokens, isPrivateNote) {
        const modal = document.getElementById('minting-modal');
        const tokenAmount = document.getElementById('modal-token-amount');
        const recipientAddress = document.getElementById('modal-recipient-address');
        const noteType = document.getElementById('modal-note-type');

        // Update modal content
        tokenAmount.textContent = amountAsTokens;
        recipientAddress.textContent = recipient;
        noteType.textContent = isPrivateNote ? 'Private' : 'Public';

        modal.classList.add('active');
    }

    setPrivateMintedSubtitle(subtitle) {
        const privateMintedSubtitle = document.getElementById('private-minted-subtitle');
        privateMintedSubtitle.innerHTML = subtitle;
    }

    hideMintingModal() {
        const mintingModal = document.getElementById('minting-modal');
        mintingModal.classList.remove('active');
    }

    showCompletedPrivateModal(recipient, amountAsTokens, txId) {
        document.getElementById('completed-private-token-amount').textContent = amountAsTokens;
        document.getElementById('completed-private-recipient-address').textContent = recipient;
        const completedPrivateModal = document.getElementById('completed-private-modal');
        completedPrivateModal.classList.add('active');
        const privateExplorerButton = document.getElementById('private-explorer-button');
        this.setupExplorerButton(privateExplorerButton, txId);
        this.showPrivateSuccessTick();
    }

    setupExplorerButton(explorerButton, txId) {
        if (this.explorerUrl) {
            explorerButton.style.display = 'block';
            explorerButton.onclick = () => window.open(`${this.explorerUrl}/tx/${txId}`, '_blank');
        } else {
            explorerButton.style.display = 'none';
        }
    }

    showCompletedPublicModal(recipient, amountAsTokens, txId) {
        document.getElementById('completed-public-token-amount').textContent = amountAsTokens;
        document.getElementById('completed-public-recipient-address').textContent = recipient;
        const completedPublicModal = document.getElementById('completed-public-modal');
        completedPublicModal.classList.add('active');

        const publicExplorerButton = document.getElementById('public-explorer-button');
        this.setupExplorerButton(publicExplorerButton, txId);
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
        errorMessage.style.backgroundColor = '#F6DED2';
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
        this.hideNextSteps();

        const errorTitle = document.getElementById('home-error-message-title');
        errorTitle.textContent = title;

        const errorDescription = document.getElementById('home-error-message-description');
        errorDescription.textContent = description;

        const errorMessage = document.getElementById('home-error-message');
        errorMessage.style.display = 'flex';
    }

    hideErrors() {
        this.hideIcons();

        const errorMessage = document.getElementById('home-error-message');
        errorMessage.style.display = 'none';
        errorMessage.style.backgroundColor = '#FFE8E9';

    }

    setTokenHint(estimatedTime) {
        this.tokenAmountHint.textContent = `Larger amounts take more time to mint. Estimated: ${estimatedTime}`;
    }

    showCloseButton() {
        const closeButton = document.getElementById('private-close-button');
        closeButton.style.display = 'block';
        closeButton.onclick = () => {
            closeButton.style.display = 'none';
            this.hideErrors();
            this.hideModals();
            this.resetForm();
            const bigDownloadButton = document.getElementById('private-download-button');
            bigDownloadButton.classList.remove('pressed')

            const instructionsDownloadButton = document.getElementById('instructions-download-button');
            instructionsDownloadButton.classList.remove('pressed')

            this.hideNextSteps();
        };
    }

    setupDownloadButton(onDownloadNote) {
        const bigDownloadButton = document.getElementById('private-download-button');
        bigDownloadButton.onclick = async () => {
            this.hideErrors();
            bigDownloadButton.classList.add('pressed');
            this.showCloseButton();
            this.showWarningText();

            await onDownloadNote();
        };

        const instructionsDownloadButton = document.getElementById('instructions-download-button');
        instructionsDownloadButton.onclick = async () => {
            this.hideErrors();
            instructionsDownloadButton.classList.add('pressed');
            await onDownloadNote();
        };
    }

    showPrivateSuccessTick() {
        const checkmark = document.getElementById('private-success-tick');
        checkmark.style.display = 'flex';

        const bigDownloadButton = document.getElementById('private-download-button');
        bigDownloadButton.style.display = 'none';
    }

    hidePrivateSuccessTick() {
        const checkmark = document.getElementById('private-success-tick');
        checkmark.style.display = 'none';
    }

    showOptionalDownload(onDownloadNote) {
        this.setupDownloadButton(onDownloadNote);
        this.showNextSteps();
        this.setNextStepsTitle('If you don\'t see the note in your wallet, you can import it manually:');

        document.getElementById('save-note-step').style.display = 'none';
        document.getElementById('download-note-step').style.display = 'block';

        this.showPrivateSuccessTick();
    }

    showDownload(onDownloadNote) {
        this.setupDownloadButton(onDownloadNote);
        this.showNextSteps();
        this.setNextStepsTitle('Next Steps');
        const bigDownloadButton = document.getElementById('private-download-button');
        bigDownloadButton.style.display = 'flex';

        document.getElementById('save-note-step').style.display = 'block';
        document.getElementById('download-note-step').style.display = 'none';

        this.hidePrivateSuccessTick();
    }

    showNextSteps() {
        const nextSteps = document.getElementById('next-steps');
        nextSteps.style.display = 'block';

        const nextStepsList = document.getElementById('next-steps-list');
        nextStepsList.style.display = 'block';
    }

    setNextStepsTitle(title) {
        const nextStepsTitle = document.getElementById('next-steps-title');
        nextStepsTitle.textContent = title;
    }

    showWarningText() {
        const warningText = document.getElementById('warning-text');
        warningText.style.display = 'block';
    }

    hideNextSteps() {
        const nextSteps = document.getElementById('next-steps');
        nextSteps.style.display = 'none';
        const warningText = document.getElementById('warning-text');
        warningText.style.display = 'none';
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

    setExplorerUrl(url) {
        this.explorerUrl = url;
    }

    setIssuanceAndSupply(issuance, max_supply, decimals) {
        this.issuance.textContent = Utils.baseUnitsToTokens(issuance, decimals);
        this.tokensSupply.textContent = Utils.baseUnitsToTokens(max_supply, decimals);
        this.issuanceFill.style.setProperty('--issuance', (issuance / max_supply) * 100);
        this.issuanceLoader.hidden = true;
        this.issuanceValues.hidden = false;
    }

    // Swap the loading placeholders for the "-" placeholders when the data can't be loaded.
    showFooterPlaceholders() {
        this.faucetAddressLoader.hidden = true;
        this.faucetAddress.hidden = false;
        this.issuanceLoader.hidden = true;
        this.issuanceValues.hidden = false;
        // The token select is still showing its "Loading…" placeholder if the options never came.
        if (this.tokenSelect.disabled && this.tokenSelect.options.length > 0) {
            this.tokenSelect.options[0].textContent = '-';
        }
    }
}
