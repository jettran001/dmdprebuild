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
        this.web3 = new Web3(privateRpc || 'https://mainnet.base.org');
        this.account = this.web3.eth.accounts.privateKeyToAccount(privateKey);
        this.web3.eth.accounts.wallet.add(this.account);
        this.mev = new MevMode(this);
        this.nodeId = 'base-node'; // Giả định node ID cho DePIN
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

        // Uniswap V3 setup for Base
        this.routerAddress = '0x3fC91A3afd70395Cd496C647d5a6CC9D4B2b7FAD'; // Uniswap V3 Router trên Base
        this.wethAddress = '0x4200000000000000000000000000000000000006'; // WETH trên Base
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

            const routerContract = new this.web3.eth.Contract(
                [
                    {
                        "inputs": [
                            {
                                "components": [
                                    { "internalType": "address", "name": "tokenIn", "type": "address" },
                                    { "internalType": "address", "name": "tokenOut", "type": "address" },
                                    { "internalType": "uint24", "name": "fee", "type": "uint24" },
                                    { "internalType": "address", "name": "recipient", "type": "address" },
                                    { "internalType": "uint256", "name": "deadline", "type": "uint256" },
                                    { "internalType": "uint256", "name": "amountIn", "type": "uint256" },
                                    { "internalType": "uint256", "name": "amountOutMinimum", "type": "uint256" },
                                    { "internalType": "uint160", "name": "sqrtPriceLimitX96", "type": "uint160" }
                                ],
                                "internalType": "struct ISwapRouter.ExactInputSingleParams",
                                "name": "params",
                                "type": "tuple"
                            }
                        ],
                        "name": "exactInputSingle",
                        "outputs": [{ "internalType": "uint256", "name": "amountOut", "type": "uint256" }],
                        "stateMutability": "payable",
                        "type": "function"
                    }
                ],
                this.routerAddress
            );

            const amountInWei = this.web3.utils.toWei(amount.toString(), 'ether');
            const deadline = Math.floor(Date.now() / 1000) + 60 * 20;
            const amountOutMin = this.web3.utils.toBN(amountInWei).mul(this.web3.utils.toBN(100 - this.maxSlippage * 100)).div(this.web3.utils.toBN(100));

            const params = {
                tokenIn: this.wethAddress,
                tokenOut: tokenAddress,
                fee: 3000, // Phí 0.3% (phổ biến trên Uniswap V3)
                recipient: this.account.address,
                deadline,
                amountIn: amountInWei,
                amountOutMinimum: amountOutMin,
                sqrtPriceLimitX96: 0
            };

            const tx = routerContract.methods.exactInputSingle(params);

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
            console.error('[BASE Buy Error]', error.message);
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

            const routerContract = new this.web3.eth.Contract(
                [
                    {
                        "inputs": [
                            {
                                "components": [
                                    { "internalType": "address", "name": "tokenIn", "type": "address" },
                                    { "internalType": "address", "name": "tokenOut", "type": "address" },
                                    { "internalType": "uint24", "name": "fee", "type": "uint24" },
                                    { "internalType": "address", "name": "recipient", "type": "address" },
                                    { "internalType": "uint256", "name": "deadline", "type": "uint256" },
                                    { "internalType": "uint256", "name": "amountIn", "type": "uint256" },
                                    { "internalType": "uint256", "name": "amountOutMinimum", "type": "uint256" },
                                    { "internalType": "uint160", "name": "sqrtPriceLimitX96", "type": "uint160" }
                                ],
                                "internalType": "struct ISwapRouter.ExactInputSingleParams",
                                "name": "params",
                                "type": "tuple"
                            }
                        ],
                        "name": "exactInputSingle",
                        "outputs": [{ "internalType": "uint256", "name": "amountOut", "type": "uint256" }],
                        "stateMutability": "payable",
                        "type": "function"
                    }
                ],
                this.routerAddress
            );

            const balance = await tokenContract.methods.balanceOf(this.account.address).call();
            const sellAmount = this.web3.utils.toBN(balance).mul(this.web3.utils.toBN(percentage * 100)).div(this.web3.utils.toBN(100));
            const deadline = Math.floor(Date.now() / 1000) + 60 * 20;
            const amountOutMin = 0;

            const params = {
                tokenIn: tokenAddress,
                tokenOut: this.wethAddress,
                fee: 3000,
                recipient: this.account.address,
                deadline,
                amountIn: sellAmount,
                amountOutMinimum: amountOutMin,
                sqrtPriceLimitX96: 0
            };

            await tokenContract.methods.approve(this.routerAddress, sellAmount).send({ from: this.account.address });
            const tx = routerContract.methods.exactInputSingle(params);

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
            console.error('[BASE Sell Error]', error.message);
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
                console.error('[BASE MEV Error]', error.message);
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
                console.error('[BASE Smart Trade Error]', error.message);
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
                network: 'base',
                address: tokenAddress
            };

            const data = await request(this.bitqueryEndpoint, query, variables, this.headers);
            const price = data.EVM.DEXTrades[0]?.Trade.Buy.Price || 0;
            return parseFloat(price);
        } catch (error) {
            console.error('[BASE getTokenPrice Error]', error.message);
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
                    EVM(mempool: true, network: base) {
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
                    console.error('[BASE fetchMempoolTrades Error]', err.message);
                    reject(err);
                }
            });

            setTimeout(() => {
                if (trades.length === 0) {
                    subscription.unsubscribe();
                    client.close();
                    resolve([]);
                }
            }, 5000);
        });
    }

    notifyUser(message, data) {
        if (io) {
            io.emit('tradeHistory', { type: message, ...data, chain: 'base', timestamp: Date.now() });
        }
        console.log(`[NOTIFY] ${message}: ${JSON.stringify(data)}`);
    }
}

module.exports = { SnipeBot };