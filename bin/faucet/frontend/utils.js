export const Utils = {
    validateAddress: (address) => {
        return /^(0x[0-9a-fA-F]{30}|[a-z]{1,4}1[a-z0-9]{32})(?:_[a-z0-9]+)?$/i.test(address);
    },

    baseUnitsToTokens: (baseUnits, decimals) => {
        return (baseUnits / 10 ** decimals).toLocaleString(undefined, {
            maximumFractionDigits: 0,
        });
    },

    tokensToBaseUnits: (tokens, decimals) => {
        return tokens * (10 ** decimals);
    },

    idFromBech32: (address) => {
        return address.split('_')[0];
    },

    fromHex: (hex) => {
        hex = hex.trim().replace(/^0x/i, '');
        return new Uint8Array(hex.match(/.{1,2}/g).map(byte => parseInt(byte, 16)));
    },
};
