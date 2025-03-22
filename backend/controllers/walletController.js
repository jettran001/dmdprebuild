// backend/controllers/walletController.js
const { logger } = require('../utils/logger');
const Wallet = require('../models/wallet');
const { createWallet, getBalance } = require('../utils/walletUtils');
const { getSnipeBot } = require('./snipebotController');

class WalletController {
async getMultiChainBalance(req) {
    try {
        const { telegramId, chains } = req.body;
        if (!telegramId || !chains) throw new Error('Missing required fields');

        const wallet = await Wallet.findOne({ telegramId });
        if (!wallet) throw new Error('Wallet not found');

        const balances = {};
        for (const chain of chains) {
            const balance = await getBalance(wallet.address, chain);
            balances[chain] = balance;

            // Giả lập số dư token (thay bằng API thực tế như Moralis)
            const tokens = [
                { symbol: 'TOKEN1', balance: Math.random() * 100, price: 1 },
                { symbol: 'TOKEN2', balance: Math.random() * 50, price: 2 },
            ];
            balances[chain] = { native: balance, tokens };
        }

        return balances;
    } catch (error) {
        logger.error(`Error getting multichain balance: ${error.message}`);
        throw error;
    }
}
async createWallet(req) {
    try {
        const { telegramId } = req.body;
        if (!telegramId) throw new Error('Missing required field: telegramId');

        let wallet = await Wallet.findOne({ telegramId });
        if (wallet) return { status: 'Wallet already exists', wallet };

        const walletData = await createWallet(telegramId);
        wallet = new Wallet({
            telegramId,
            address: walletData.address,
            seedPhrase: walletData.seedPhrase,
        });
        await wallet.save();

        return { status: 'Wallet created', wallet };
    } catch (error) {
        logger.error(`Error creating wallet: ${error.message}`);
        throw error;
    }
}

async importWallet(req) {
    try {
        const { telegramId, seedPhrase } = req.body;
        if (!telegramId || !seedPhrase) throw new Error('Missing required fields: telegramId, seedPhrase');

        let wallet = await Wallet.findOne({ telegramId });
        if (wallet) throw new Error('Wallet already exists for this Telegram ID');

        const walletData = await createWallet(telegramId, seedPhrase);
        wallet = new Wallet({
            telegramId,
            address: walletData.address,
            seedPhrase: walletData.seedPhrase,
        });
        await wallet.save();

        return { status: 'Wallet imported', wallet };
    } catch (error) {
        logger.error(`Error importing wallet: ${error.message}`);
        throw error;
    }
}

async getWalletBalance(req) {
    try {
        const { telegramId, chain } = req.params;
        const wallet = await Wallet.findOne({ telegramId });
        if (!wallet) throw new Error('Wallet not found');

        const balance = await getBalance(wallet.address, chain);
        return { status: 'Success', balance };
    } catch (error) {
        logger.error(`Error getting wallet balance: ${error.message}`);
        throw error;
    }
}

async connectToBot(req) {
    try {
        const { telegramId, chain, tokenAddress } = req.body;
        const wallet = await Wallet.findOne({ telegramId });
        if (!wallet) throw new Error('Wallet not found');

        const bot = getSnipeBot(chain);
        const gasParams = await new (require('../gasOptimizer'))().optimizeGas(chain, 50, await bot.getMempool(), bot);
        const result = await bot.monitorMempoolMEV(tokenAddress, null, gasParams);
        return { status: 'Bot connected and sniping started', result };
    } catch (error) {
        logger.error(`Error connecting wallet to bot: ${error.message}`);
        throw error;
    }
}
}

module.exports = new WalletController();