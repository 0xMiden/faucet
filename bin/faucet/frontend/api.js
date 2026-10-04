// Custom error class to include status code
export class ApiError extends Error {
    constructor(message, statusCode) {
        super(message);
        this.name = 'ApiError';
        this.statusCode = statusCode;
    }
}

export async function getConfig() {
    const response = await fetch('/config.json');
    if (!response.ok) {
        throw new ApiError(response.statusText, response.status);
    }
    return await response.json();
}

export async function getMetadata(backendUrl) {
    const response = await fetch(backendUrl + '/get_metadata');
    if (!response.ok) {
        throw new ApiError(response.statusText, response.status);
    }
    return response.json();
}

export async function getPowChallenge(backendUrl, recipient, amount) {
    const response = await fetch(backendUrl + '/pow?' + new URLSearchParams({
        amount: amount,
        account_id: recipient
    }));

    if (!response.ok) {
        const message = await response.text();
        throw new ApiError(message, response.status);
    }

    return response.json();
}

export async function getTokens(backendUrl, challenge, nonce, recipient, amount) {
    const params = {
        account_id: recipient,
        asset_amount: parseInt(amount),
        challenge: challenge,
        nonce: nonce
    };

    const response = await fetch(backendUrl + '/get_tokens?' + new URLSearchParams(params));

    if (!response.ok) {
        const message = await response.text();
        throw new ApiError(message, response.status);
    }

    return response.json();
}
