// backend/utils/walletUtils.js
const { ethers } = require('ethers');
const { logger } = require('./logger');

const createWallet = async (telegramId, seedPhrase = null) => {
    try {
        let wallet;
        if (seedPhrase) {
            wallet = ethers.Wallet.fromMnemonic(seedPhrase);
        } else {
            wallet = ethers.Wallet.createRandom();
        }
        return {
            address: wallet.address,
            seedPhrase: wallet.mnemonic.phrase,
        };
    } catch (error) {
        logger.error(`Error creating wallet: ${error.message}`);
        throw error;
    }
};

const getBalance = async (address, chain) => {
    try {
        const provider = new ethers.JsonRpcProvider(process.env[`PRIVATE_RPC_${chain.toUpperCase()}`]);
        const balance = await provider.getBalance(address);
        return ethers.formatEther(balance);
    } catch (error) {
        logger.error(`Error getting balance: ${error.message}`);
        throw error;
    }
};

module.exports = { createWallet, getBalance };