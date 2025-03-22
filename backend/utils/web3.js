const ethers = require('ethers');

const getProvider = (chain) => {
    switch (chain) {
        case 'bsc':
            return new ethers.providers.JsonRpcProvider('https://bsc-dataseed1.binance.org');
        case 'eth':
            return new ethers.providers.JsonRpcProvider('https://mainnet.infura.io/v3/YOUR_INFURA_KEY');
        default:
            throw new Error('Unsupported chain');
    }
};

const getWallet = (privateKey, chain) => {
    const provider = getProvider(chain);
    return new ethers.Wallet(privateKey, provider);
};

module.exports = { getProvider, getWallet };