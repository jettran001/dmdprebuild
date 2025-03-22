const { connect, KeyPair, keyStores, utils } = require('near-api-js');
const { io } = require('../server');
const MevMode = require('./mevMode');
const DepinManager = require('../depin/depinManager');
const axios = require('axios');

const depinManager = new DepinManager();

class SnipeBot {
    constructor(privateKey, rpcUrl) {
        this.config = {
            networkId: 'mainnet',
            nodeUrl: rpcUrl || 'https://rpc.mainnet.near.org',
            walletUrl: 'https://wallet.mainnet.near.org',
            helperUrl: 'https://helper.mainnet.near.org',
            explorerUrl: 'https://explorer.mainnet.near.org',
        };
        this.keyStore = new keyStores.InMemoryKeyStore();
        this.mev = new MevMode(this);
        this.nodeId = 'near-node'; // Giả định node ID cho DePIN
        this.isMonitoring = false;
        this.isMonitoringSmart = false;
        this.profitLoss = { profit: 0, loss: 0 };
        this.minProfit = 0.01;
        this.maxSlippage = 0.05;
        this.tslPercentage = 0.05;

        // Ref Finance setup
        this.refFinanceContract = 'v2.ref-finance.near';
        this.wnearAddress = 'wrap.near'; // WNEAR trên NEAR

        // Setup NEAR connection
        this.setup(privateKey);
    }

    async setup(privateKey) {
        const keyPair = KeyPair.fromString(privateKey);
        await this.keyStore.setKey(this.config.networkId, '', keyPair);
        this.near = await connect({ ...this.config, keyStore: this.keyStore });
        this.account = await this.near.account(keyPair.publicKey.toString().replace('ed25519:', ''));
    }

    async buyToken(tokenAddress, amount) {
        try {
            const amountInYocto = utils.format.parseNearAmount(amount.toString()); // Chuyển đổi sang yoctoNEAR
            const tokenOut = tokenAddress;
            const tokenIn = this.wnearAddress;

            // Đảm bảo tài khoản có WNEAR (gửi NEAR để wrap thành WNEAR nếu cần)
            const wnearBalance = await this.getTokenBalance(this.wnearAddress);
            if (parseFloat(wnearBalance) < amount) {
                await this.account.functionCall({
                    contractId: this.wnearAddress,
                    methodName: 'near_deposit',
                    args: {},
                    gas: '30000000000000',
                    attachedDeposit: amountInYocto,
                });
            }

            // Swap trên Ref Finance
            const actions = [
                {
                    pool_id: await this.getPoolId(tokenIn, tokenOut),
                    token_in: tokenIn,
                    token_out: tokenOut,
                    amount_in: amountInYocto,
                    min_amount_out: '0', // Cần tính chính xác hơn nếu muốn
                }
            ];

            const result = await this.account.functionCall({
                contractId: this.refFinanceContract,
                methodName: 'swap',
                args: {
                    actions,
                },
                gas: '30000000000000',
                attachedDeposit: '1', // 1 yoctoNEAR
            });

            const buyPrice = await this.getTokenPrice(tokenAddress);
            const tradeData = { tokenAddress, buyAmount: amount, buyPrice, timestamp: Date.now() };
            const cid = await depinManager.storeData(this.nodeId, tradeData);
            this.notifyUser('Buy', { tokenAddress, amount, buyPrice, cid });

            this.profitLoss.profit += buyPrice * amount * this.minProfit;
            return { buyAmount: amount, buyPrice };
        } catch (error) {
            console.error('[NEAR Buy Error]', error.message);
            throw error;
        }
    }

    async sellToken(tokenAddress, percentage) {
        try {
            const tokenIn = tokenAddress;
            const tokenOut = this.wnearAddress;

            const balance = await this.getTokenBalance(tokenIn);
            const sellAmount = parseFloat(balance) * percentage;
            const amountInYocto = utils.format.parseNearAmount(sellAmount.toString());

            // Swap trên Ref Finance
            const actions = [
                {
                    pool_id: await this.getPoolId(tokenIn, tokenOut),
                    token_in: tokenIn,
                    token_out: tokenOut,
                    amount_in: amountInYocto,
                    min_amount_out: '0',
                }
            ];

            const result = await this.account.functionCall({
                contractId: this.refFinanceContract,
                methodName: 'swap',
                args: {
                    actions,
                },
                gas: '30000000000000',
                attachedDeposit: '1',
            });

            const sellPrice = await this.getTokenPrice(tokenAddress);
            const tradeData = { tokenAddress, sellAmount: sellAmount.toString(), sellPrice, timestamp: Date.now() };
            const cid = await depinManager.storeData(this.nodeId, tradeData);
            this.notifyUser('Sell', { tokenAddress, amount: sellAmount.toString(), sellPrice, cid });

            this.profitLoss.loss += sellPrice * sellAmount * (1 - this.tslPercentage);
            return sellAmount.toString();
        } catch (error) {
            console.error('[NEAR Sell Error]', error.message);
            throw error;
        }
    }

    async sellAll(tokenAddress) {
        return this.sellToken(tokenAddress, 1);
    }

    async monitorMempoolMEV() {
        this.isMonitoring = true;
        const trades = await this.fetchMempoolTrades();
        for (const trade of trades) {
            if (!this.isMonitoring) break;
            try {
                const tokenAddress = trade.tokenAddress;
                const score = await this.checkContract(tokenAddress);
                if (score === '🟢') {
                    const buyAmount = 0.1;
                    await this.mev.frontRunMEV(tokenAddress, buyAmount);
                    const buyPrice = await this.getTokenPrice(tokenAddress);
                    const tradeData = { tokenAddress, buyAmount, buyPrice, timestamp: Date.now() };
                    const cid = await depinManager.storeData(this.nodeId, tradeData);
                    this.notifyUser('MEV Buy', { tokenAddress, amount: buyAmount, buyPrice, cid });
                    return { tokenAddress, buyAmount, buyPrice, cid };
                }
            } catch (error) {
                console.error('[NEAR MEV Error]', error.message);
            }
        }
    }

    async monitorMempoolSmart() {
        this.isMonitoringSmart = true;
        const trades = await this.fetchMempoolTrades();
        for (const trade of trades) {
            if (!this.isMonitoringSmart) break;
            try {
                const tokenAddress = trade.tokenAddress;
                const score = await this.checkContract(tokenAddress);
                if (score === '🟢') {
                    const buyAmount = 0.05;
                    const { buyPrice } = await this.buyToken(tokenAddress, buyAmount);
                    if (buyPrice > this.minProfit) {
                        const sellAmount = await this.sellToken(tokenAddress, this.tslPercentage);
                        this.notifyUser('Smart Trade', { tokenAddress, buyAmount, buyPrice, sellAmount });
                    }
                }
            } catch (error) {
                console.error('[NEAR Smart Trade Error]', error.message);
            }
        }
    }

    stopMonitoring() {
        this.isMonitoring = false;
        this.isMonitoringSmart = false;
    }

    async checkContract(tokenAddress) {
        return '🟢'; // Giả định kiểm tra hợp đồng
    }

    async getTokenPrice(tokenAddress) {
        try {
            const response = await axios.get(`https://api.ref.finance/get-token-price?token_id=${tokenAddress}`);
            const price = response.data.price || 0;
            return parseFloat(price);
        } catch (error) {
            console.error('[NEAR getTokenPrice Error]', error.message);
            return 0;
        }
    }

    async fetchMempoolTrades() {
        try {
            const response = await axios.get(`https://api.ref.finance/list-recent-transactions`);
            const trades = response.data || [];
            return trades.map(trade => ({
                tokenAddress: trade.token_out,
                amount: parseFloat(trade.amount_out),
                price: parseFloat(trade.price),
            }));
        } catch (error) {
            console.error('[NEAR fetchMempoolTrades Error]', error.message);
            return [];
        }
    }

    async getTokenBalance(tokenAddress) {
        try {
            const result = await this.account.viewFunction({
                contractId: tokenAddress,
                methodName: 'ft_balance_of',
                args: { account_id: this.account.accountId },
            });
            return utils.format.formatNearAmount(result);
        } catch (error) {
            console.error('[NEAR getTokenBalance Error]', error.message);
            return '0';
        }
    }

    async getPoolId(tokenIn, tokenOut) {
        try {
            const pools = await axios.get(`https://api.ref.finance/list-pools`);
            const pool = pools.data.find(p => 
                (p.token0 === tokenIn && p.token1 === tokenOut) || 
                (p.token0 === tokenOut && p.token1 === tokenIn)
            );
            return pool ? pool.pool_id : null;
        } catch (error) {
            console.error('[NEAR getPoolId Error]', error.message);
            return null;
        }
    }

    notifyUser(message, data) {
        if (io) {
            io.emit('tradeHistory', { type: message, ...data, chain: 'near', timestamp: Date.now() });
        }
        console.log(`[NOTIFY] ${message}: ${JSON.stringify(data)}`);
    }
}

module.exports = { SnipeBot };