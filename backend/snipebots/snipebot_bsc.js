const Web3 = require('web3');
const { io } = require('../server');
const MevMode = require('./mevMode');
const DepinManager = require('../depin/depinManager');
const { request, gql } = require('graphql-request');
const { SubscriptionClient } = require('subscriptions-transport-ws');
const ws = require('ws');

const depinManager = new DepinManager();

class SnipeBot {
    constructor(privateKey, privateRpc) {
        this.web3 = new Web3(privateRpc || 'https://bsc-dataseed.binance.org/');
        this.account = this.web3.eth.accounts.privateKeyToAccount(privateKey);
        this.web3.eth.accounts.wallet.add(this.account);
        this.mev = new MevMode(this);
        this.nodeId = 'bsc-node';
        this.isMonitoring = false;
        this.isMonitoringSmart = false;
        this.profitLoss = { profit: 0, loss: 0 };
        this.minProfit = 0.01;
        this.maxSlippage = 0.05;
        this.tslPercentage = 0.05;

        // Bitquery API setup
        this.bitqueryApiKey = process.env.BITQUERY_API_KEY || 'your-bitquery-api-key';
        this.bitqueryEndpoint = 'https://graphql.bitquery.io';
        this.bitqueryWsEndpoint = 'wss://graphql.bitquery.io/graphql';
        this.headers = { 'X-API-KEY': this.bitqueryApiKey };
    }

    async buyToken(tokenAddress, amount, poolId) {
        try {
            const tokenContract = new this.web3.eth.Contract(
                [
                    { "constant": true, "inputs": [], "name": "balanceOf", "outputs": [{ "name": "", "type": "uint256" }], "type": "function" },
                    { "constant": false, "inputs": [{ "name": "_to", "type": "address" }, { "name": "_value", "type": "uint256" }], "name": "transfer", "outputs": [{ "name": "", "type": "bool" }], "type": "function" }
                ],
                tokenAddress
            );

            const routerAddress = '0x10ED43C718714eb63d5aA57B78B54704E256024'; // PancakeSwap Router
            const routerContract = new this.web3.eth.Contract(
                [
                    { "constant": false, "inputs": [{ "name": "amountOutMin", "type": "uint256" }, { "name": "path", "type": "address[]" }, { "name": "to", "type": "address" }, { "name": "deadline", "type": "uint256" }], "name": "swapExactETHForTokens", "outputs": [{ "name": "amounts", "type": "uint256[]" }], "type": "function" }
                ],
                routerAddress
            );

            const amountInWei = this.web3.utils.toWei(amount.toString(), 'ether');
            const path = ['0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c', tokenAddress]; // WBNB -> Token
            const deadline = Math.floor(Date.now() / 1000) + 60 * 20;
            const amountOutMin = this.web3.utils.toBN(amountInWei).mul(this.web3.utils.toBN(100 - this.maxSlippage * 100)).div(this.web3.utils.toBN(100));

            const tx = routerContract.methods.swapExactETHForTokens(
                amountOutMin,
                path,
                this.account.address,
                deadline
            );

            const gas = await tx.estimateGas({ from: this.account.address, value: amountInWei });
            const gasPrice = await this.web3.eth.getGasPrice();

            const receipt = await tx.send({
                from: this.account.address,
                value: amountInWei,
                gas,
                gasPrice
            });

            const buyPrice = await this.getTokenPrice(tokenAddress);
            const tradeData = { tokenAddress, buyAmount: amount, buyPrice, timestamp: Date.now() };
            const cid = await depinManager.storeData(this.nodeId, tradeData);
            this.notifyUser('Buy', { tokenAddress, amount, buyPrice, cid });

            this.profitLoss.profit += buyPrice * amount * this.minProfit;
            return { buyAmount: amount, buyPrice };
        } catch (error) {
            console.error('[BSC Buy Error]', error.message);
            throw error;
        }
    }

    async sellToken(tokenAddress, percentage) {
        try {
            const tokenContract = new this.web3.eth.Contract(
                [
                    { "constant": true, "inputs": [], "name": "balanceOf", "outputs": [{ "name": "", "type": "uint256" }], "type": "function" },
                    { "constant": false, "inputs": [{ "name": "_to", "type": "address" }, { "name": "_value", "type": "uint256" }], "name": "transfer", "outputs": [{ "name": "", "type": "bool" }], "type": "function" },
                    { "constant": false, "inputs": [{ "name": "_spender", "type": "address" }, { "name": "_value", "type": "uint256" }], "name": "approve", "outputs": [{ "name": "", "type": "bool" }], "type": "function" }
                ],
                tokenAddress
            );

            const routerAddress = '0x10ED43C718714eb63d5aA57B78B54704E256024'; // PancakeSwap Router
            const routerContract = new this.web3.eth.Contract(
                [
                    { "constant": false, "inputs": [{ "name": "amountIn", "type": "uint256" }, { "name": "amountOutMin", "type": "uint256" }, { "name": "path", "type": "address[]" }, { "name": "to", "type": "address" }, { "name": "deadline", "type": "uint256" }], "name": "swapExactTokensForETH", "outputs": [{ "name": "amounts", "type": "uint256[]" }], "type": "function" }
                ],
                routerAddress
            );

            const balance = await tokenContract.methods.balanceOf(this.account.address).call();
            const sellAmount = this.web3.utils.toBN(balance).mul(this.web3.utils.toBN(percentage * 100)).div(this.web3.utils.toBN(100));
            const amountOutMin = 0;
            const path = [tokenAddress, '0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c']; // Token -> WBNB
            const deadline = Math.floor(Date.now() / 1000) + 60 * 20;

            await tokenContract.methods.approve(routerAddress, sellAmount).send({ from: this.account.address });
            const tx = routerContract.methods.swapExactTokensForETH(
                sellAmount,
                amountOutMin,
                path,
                this.account.address,
                deadline
            );

            const gas = await tx.estimateGas({ from: this.account.address });
            const gasPrice = await this.web3.eth.getGasPrice();

            const receipt = await tx.send({
                from: this.account.address,
                gas,
                gasPrice
            });

            const sellPrice = await this.getTokenPrice(tokenAddress);
            const tradeData = { tokenAddress, sellAmount: sellAmount.toString(), sellPrice, timestamp: Date.now() };
            const cid = await depinManager.storeData(this.nodeId, tradeData);
            this.notifyUser('Sell', { tokenAddress, amount: sellAmount.toString(), sellPrice, cid });

            this.profitLoss.loss += sellPrice * (sellAmount / 1e18) * (1 - this.tslPercentage);
            return sellAmount.toString();
        } catch (error) {
            console.error('[BSC Sell Error]', error.message);
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
                console.error('[BSC MEV Error]', error.message);
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
                    const { buyPrice } = await this.buyToken(tokenAddress, buyAmount, null);
                    if (buyPrice > this.minProfit) {
                        const sellAmount = await this.sellToken(tokenAddress, this.tslPercentage);
                        this.notifyUser('Smart Trade', { tokenAddress, buyAmount, buyPrice, sellAmount });
                    }
                }
            } catch (error) {
                console.error('[BSC Smart Trade Error]', error.message);
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
            const query = gql`
                query ($network: EthereumNetwork!, $address: String!) {
                    EVM(network: $network) {
                        DEXTrades(
                            where: {Trade: {Buy: {Currency: {SmartContract: {is: $address}}}}}
                            orderBy: {descending: Block_Time}
                            limit: {count: 1}
                        ) {
                            Trade {
                                Buy {
                                    Price
                                    Currency {
                                        Symbol
                                    }
                                }
                            }
                        }
                    }
                }
            `;

            const variables = {
                network: 'bsc',
                address: tokenAddress
            };

            const data = await request(this.bitqueryEndpoint, query, variables, this.headers);
            const price = data.EVM.DEXTrades[0]?.Trade.Buy.Price || 0;
            return parseFloat(price);
        } catch (error) {
            console.error('[BSC getTokenPrice Error]', error.message);
            return 0;
        }
    }

    async fetchMempoolTrades() {
        return new Promise((resolve, reject) => {
            const client = new SubscriptionClient(this.bitqueryWsEndpoint, {
                reconnect: true,
                connectionParams: {
                    headers: this.headers
                }
            }, ws);

            const query = gql`
                subscription {
                    EVM(mempool: true, network: bsc) {
                        buyside: DEXTrades {
                            Transaction {
                                Hash
                                From
                                To
                            }
                            Trade {
                                Buy {
                                    Currency {
                                        SmartContract
                                        Symbol
                                    }
                                    Amount
                                    Buyer
                                    Price
                                }
                            }
                        }
                    }
                }
            `;

            const trades = [];
            const subscription = client.request({ query }).subscribe({
                next({ data }) {
                    const tradeData = data?.EVM?.buyside || [];
                    tradeData.forEach(trade => {
                        trades.push({
                            tokenAddress: trade.Trade.Buy.Currency.SmartContract,
                            amount: parseFloat(trade.Trade.Buy.Amount),
                            price: parseFloat(trade.Trade.Buy.Price)
                        });
                    });
                    if (trades.length > 0) {
                        subscription.unsubscribe();
                        client.close();
                        resolve(trades);
                    }
                },
                error(err) {
                    console.error('[BSC fetchMempoolTrades Error]', err.message);
                    reject(err);
                }
            });

            setTimeout(() => {
                if (trades.length === 0) {
                    subscription.unsubscribe();
                    client.close();
                    resolve([]); // Trả về mảng rỗng nếu không có trades
                }
            }, 5000);
        });
    }

    notifyUser(message, data) {
        if (io) {
            io.emit('tradeHistory', { type: message, ...data, chain: 'bsc', timestamp: Date.now() });
        }
        console.log(`[NOTIFY] ${message}: ${JSON.stringify(data)}`);
    }
}

module.exports = { SnipeBot };