const { io } = require('../server'); // Import io từ server.js
const { publishTrade } = require('../utils/mqtt');
const { TonClient, WalletContractV4, TonClient4, toNano } = require('@ton/ton');
const { apiRequest } = require('../utils/api');
const MevMode = require('./mevMode');
const DepinManager = require('../depin/depinManager');
const depinManager = new DepinManager();
let io;


class SnipeBot {
    constructor(walletPrivateKey, privateRpc = null) {
        this.client = new TonClient({
            endpoint: privateRpc || 'https://toncenter.com/api/v2/jsonRPC'
        });
        this.keyPair = Buffer.from(walletPrivateKey, 'hex'); // Private key dạng hex
        this.wallet = WalletContractV4.create({ 
            workchain: 0,
            publicKey: Buffer.from(this.keyPair.slice(0, 32)) // Public key từ private key
        });
        this.pendingTxs = new Map();
        this.profitLoss = { profit: 0, loss: 0 };
        this.minProfit = 0.01; // 0.01 TON
        this.tslPercentage = 0.05;
        this.maxSlippage = 0.05;
        this.frontRunOrders = 3;
        this.routerAddress = 'EQB-ajMyiLtA2hJ8neXkhhyJZ8bOrbDATERaCvy4vKp4D0iX'; // STON.fi Router
        this.mev = new MevMode(this); // Khởi tạo MevMode
        this.nodeId = 'ton-node'; // Giả định node ID
    }

    async offchainVerify(data) {
        // Giả lập xác thực offchain (dùng GPU nếu cần)
        const hash = require('crypto').createHash('sha256').update(JSON.stringify(data)).digest('hex');
        return hash;
    }
    
    async getTradeFromIPFS(cid) {
        const tradeData = await depinManager.fetchData(this.nodeId, cid);
        console.log(`[IPFS] Retrieved trade: ${JSON.stringify(tradeData)}`);
        return tradeData;
    }

    async monitorMempoolMEV() {
        while (this.isMonitoring) {
            try {
                const trades = await apiRequest('https://api.ston.fi/v1/trades', 'GET');
                for (const trade of trades) {
                    const tokenAddress = trade.token_out;
                    const score = await this.checkContract(tokenAddress);
                    if (score === '🟢') {
                        const buyAmount = 0.1;
                        await this.mev.frontRunMEV(tokenAddress, buyAmount);
                        const buyPrice = trade.price || 0.01;
                        const tradeData = { tokenAddress, buyAmount, buyPrice, timestamp: Date.now() };
                        // Lưu trữ tradeData trên IPFS
                        const cid = await depinManager.storeData(this.nodeId, tradeData);
                        console.log(`[IPFS] Trade stored with CID: ${cid}`);

                        this.notifyUser('MEV Buy', { tokenAddress, amount: buyAmount });
                        publishTrade('snipebot/ton/trades', { tokenAddress, buyAmount, buyPrice });
                        return { tokenAddress, buyAmount, buyPrice };
                    }
                }
                await new Promise(resolve => setTimeout(resolve, 1000));
            } catch (error) {
                console.error('Error in monitorMempoolMEV:', error);
            }
        }
    }

    stopMonitoring() {
        this.isMonitoring = false;
    }

    async init() {
        this.walletContract = await this.client.open(this.wallet);
    }

    async buyToken(tokenAddress, amount, poolId = null) {
        try {
            await this.init();
            const amountNano = toNano(amount.toString()); // TON -> nanoTON
        const payload = {
            $$type: 'Swap',
            tokenIn: 'EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAM9c', // TON native
            tokenOut: tokenAddress,
            amount: amountNano,
            minAmountOut: '0',
            slippage: this.maxSlippage
        };

        const tx = {
            messages: [{
                address: this.routerAddress,
                amount: amountNano,
                payload: Buffer.from(JSON.stringify(payload)).toString('base64')
            }],
            validUntil: Math.floor(Date.now() / 1000) + 60 * 5 // 5 phút
        };

        const result = await this.walletContract.sendTransfer({
            seqno: await this.walletContract.getSeqno(),
            secretKey: this.keyPair,
            messages: [tx.messages[0]]
        });
    } catch (error) {
        console.error(`[BUY ERROR] ${tokenAddress}: ${error.message}`);
        throw error;
    }
    }

    async sellToken(tokenAddress, percentage, poolId = null) {
        await this.init();
        const balance = await this.getBalance(tokenAddress);
        const amountIn = balance * (percentage / 100);
        const amountNano = toNano(amountIn.toString());

        const payload = {
            $$type: 'Swap',
            tokenIn: tokenAddress,
            tokenOut: 'EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAM9c', // TON native
            amount: amountNano,
            minAmountOut: '0',
            slippage: this.maxSlippage
        };

        const tx = {
            messages: [{
                address: this.routerAddress,
                amount: toNano('0.05'), // Phí gas
                payload: Buffer.from(JSON.stringify(payload)).toString('base64')
            }],
            validUntil: Math.floor(Date.now() / 1000) + 60 * 5
        };

        const result = await this.walletContract.sendTransfer({
            seqno: await this.walletContract.getSeqno(),
            secretKey: this.keyPair,
            messages: [tx.messages[0]]
        });
        console.log(`[SELL] ${tokenAddress} - ${percentage}% (${amountIn} tokens) on STON.fi: ${result}`);
        return percentage;
    }

    async getBalance(tokenAddress) {
        const jettonWallet = await this.client.getJettonWallet(this.wallet.address, tokenAddress);
        const balanceData = await this.client.runMethod(jettonWallet, 'get_wallet_data');
        return parseInt(balanceData.stack.readBigNumber().toString()) / 10**9; // nanoTON -> TON
    }

    async monitorMempoolSmart() {
        while (true) {
            try {
                // TON không có mempool, dùng API STON.fi để lấy giao dịch gần đây
                const trades = await apiRequest('https://api.ston.fi/v1/trades', 'GET');
                for (const trade of trades) {
                    const tokenAddress = trade.token_out;
                    const score = await this.checkContract(tokenAddress);
                    if (score === '🟢') {
                        const amount = 0.1; // Ví dụ
                        await this.buyToken(tokenAddress, amount);
                        this.notifyUser('Smart Buy', { tokenAddress, amount });
                    }
                }
                await new Promise(resolve => setTimeout(resolve, 1000)); // Chờ 1 giây
            } catch (error) {
                console.error('Error in monitorMempoolSmart:', error);
            }
        }
    }

    async checkContract(tokenAddress) {
        // Giả lập kiểm tra hợp đồng (cần API thực tế)
        return '🟢'; // Ví dụ
    }

//     notifyUser(message, data) {
//         if (io) {
//             io.emit('botStatus', { chain: 'ton', status: this.monitorMempoolSmart ? 'Running' : 'Stopped' });
//             io.emit('tradeHistory', { type: message, tokenAddress: data.tokenAddress, amount: data.amount, chain: 'ton', timestamp: Date.now() });
//         }
//         console.log(`[NOTIFY] ${message}: ${JSON.stringify(data)}`);
//     }
// }
        notifyUser(message, data) {
            if (io) io.emit('tradeHistory', { type: message, ...data, chain: 'ton', timestamp: Date.now() });
            publishTrade('snipebot/ton/trades', { type: message, ...data });
        console.log(`[NOTIFY] ${message}: ${JSON.stringify(data)}`);
    }


// Các hàm khác nếu cần: frontRunMEV, sellAll, etc.


}
module.exports = { SnipeBot };