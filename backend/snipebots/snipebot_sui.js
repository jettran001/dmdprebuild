const { io } = require('../server'); // Import io từ server.js
const { CetusClmmSDK, Keypair } = require('@cetusprotocol/cetus-sdk');
const { apiRequest } = require('../utils/api');
const MevMode = require('./mevMode');

// Khai báo io ở scope toàn cục
let io;

class SnipeBot {
    constructor(walletPrivateKey, privateRpc = null) {
        this.keypair = Keypair.fromSecretKey(new Uint8Array(JSON.parse(walletPrivateKey)));
        this.sdk = new CetusClmmSDK({
            nodeUrl: privateRpc || 'https://mainnet.sui.io',
            keypair: this.keypair
        });
        this.pendingTxs = new Map();
        this.profitLoss = { profit: 0, loss: 0 };
        this.minProfit = 0.01;
        this.tslPercentage = 0.05;
        this.maxSlippage = 0.05;
        this.frontRunOrders = 3;
        this.suiCoinType = '0x2::sui::SUI'; // Coin type của SUI
        this.mev = new MevMode(this);
    }


    async monitorMempoolMEV() {
        while (true) {
            try {
                const trades = await apiRequest('https://api.ston.fi/v1/trades', 'GET');
                for (const trade of trades) {
                    const tokenAddress = trade.token_out;
                    const score = await this.checkContract(tokenAddress);
                    if (score === '🟢') {
                        const buyAmount = 0.1;
                        await this.mev.frontRunMEV(tokenAddress, buyAmount);
                        const buyPrice = trade.price || 0.01;
                        this.notifyUser('MEV Buy', { tokenAddress, amount: buyAmount });
                        publishTrade('snipebot/sui/trades', { tokenAddress, buyAmount, buyPrice });
                        return { tokenAddress, buyAmount, buyPrice };
                    }
                }
                await new Promise(resolve => setTimeout(resolve, 1000));
            } catch (error) {
                console.error('Error in monitorMempoolMEV:', error);
            }
        }
    }


    async getTokenPrice(tokenAddress) {
        // Giả lập giá
        return 0.01;
    }

    async checkContract(tokenAddress) {
        // Giả lập kiểm tra hợp đồng (cần API thực tế)
        return '🟢'; // Ví dụ
    }

    async monitorMempoolMEV() {
        provider.on('pending', async (txHash) => {
            const tx = await provider.getTransaction(txHash);
            if (!tx || !tx.to) return;

            const check = await this.checkContract(tx.to);
            this.notifyUser(`[MEV] Token ${tx.to}: ${check.symbol}`, check.status);

            if (check.symbol === '🔴') return;

            const amount = ethers.utils.formatEther(tx.value);
            const tokenPrice = await this.getPrice(tx.to);
            const startPrice = tokenPrice;

            if (amount > 0.1) {
                await this.frontRunMEV(tx, tx.to, startPrice);
            } else if (await this.checkArbitrage(tx.to)) {
                await this.arbitrageMEV(tx.to);
            }
        });
    }

    async frontRunMEV(tx, tokenAddress, startPrice) {
        const gasPrice = await provider.getGasPrice();
        const gasLimit = 300000;
        const gasCost = gasPrice.mul(gasLimit);
        const buyAmount = ethers.utils.parseEther('0.1'); // 0.1 ETH

        const expectedPriceAfter = startPrice * 1.1;
        const profit = (expectedPriceAfter - startPrice) * parseFloat(ethers.utils.formatEther(buyAmount)) - ethers.utils.formatEther(gasCost);
        if (profit < ethers.utils.formatEther(this.minProfit)) return;

        const txOptions = { gasPrice: gasPrice.mul(2), gasLimit };
        const buyTx = await this.buyToken(tokenAddress, buyAmount, txOptions);
        this.notifyUser(`[MEV] Front-run Buy: ${tokenAddress}`, { amount: 0.1, price: startPrice });

        await provider.waitForTransaction(tx.hash);
        const sellPrice = await this.getPrice(tokenAddress);
        if (sellPrice > startPrice) {
            await this.sellToken(tokenAddress, 100, txOptions);
            this.updateProfitLoss(sellPrice, startPrice, buyAmount);
            this.notifyUser(`[MEV] Front-run Sell: ${tokenAddress}`, { profit: sellPrice - startPrice });
        }
    }

    async checkArbitrage(tokenAddress) {
        const priceUniswap = await this.getPriceFromDEX(tokenAddress, 'Uniswap');
        const priceSushiswap = await this.getPriceFromDEX(tokenAddress, 'Sushiswap');
        return priceUniswap < priceSushiswap * 0.98;
    }

    async arbitrageMEV(tokenAddress) {
        const buyAmount = ethers.utils.parseEther('0.1');
        const gasPrice = await provider.getGasPrice();
        const txOptions = { gasPrice: gasPrice.mul(2), gasLimit: 300000 };

        const buyPrice = await this.getPriceFromDEX(tokenAddress, 'Uniswap');
        const sellPrice = await this.getPriceFromDEX(tokenAddress, 'Sushiswap');
        const profit = (sellPrice - buyPrice) * parseFloat(ethers.utils.formatEther(buyAmount)) - ethers.utils.formatEther(gasPrice.mul(300000));

        if (profit > ethers.utils.formatEther(this.minProfit)) {
            await this.buyToken(tokenAddress, buyAmount, txOptions, 'Uniswap');
            await this.sellToken(tokenAddress, 100, txOptions, 'Sushiswap');
            this.updateProfitLoss(sellPrice, buyPrice, buyAmount);
            this.notifyUser(`[MEV] Arbitrage: ${tokenAddress}`, { profit });
        }
    }

    async monitorMempoolSmart() {
        while (true) {
            try {
                // SUI không có mempool như Ethereum, dùng event listener thay thế
                const events = await this.sdk.client.getEvents({
                    MoveEvent: 'cetus_clmm::SwapEvent' // Ví dụ event swap của Cetus
                });
                for (const event of events) {
                    const tokenAddress = event.parsedJson.token_out; // Giả lập
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

    async quickTrade(tokenAddress, startPrice) {
        if (await this.checkSlippage(tokenAddress, 0.1)) {
            const amount = await this.buyToken(tokenAddress, 0.1);
            this.notifyUser(`[SMART] Quick Buy: ${tokenAddress}`, { amount: 0.1, price: startPrice });

            setTimeout(async () => {
                const currentPrice = await this.getPrice(tokenAddress);
                if (currentPrice > startPrice * 1.05) {
                    await this.sellAll(tokenAddress);
                    this.updateProfitLoss(currentPrice, startPrice, amount);
                    this.notifyUser(`[SMART] Quick Sell: ${tokenAddress}`, { profit: currentPrice - startPrice });
                } else {
                    await this.sellAll(tokenAddress);
                    this.notifyUser(`[SMART] Quick Sell: ${tokenAddress}`, { reason: 'No 5% gain after 5m' });
                }
            }, 5 * 60 * 1000);
        }
    }

    async frontRunSmart(tx, tokenAddress, startPrice) {
        let buyOrders = [tx];
        let orderCount = 1;

        provider.on('pending', async (nextTxHash) => {
            const nextTx = await provider.getTransaction(nextTxHash);
            if (nextTx.to === tokenAddress && nextTx.value > ethers.utils.parseEther('0.1') && orderCount < this.frontRunOrders) {
                buyOrders.push(nextTx);
                orderCount++;
            } else {
                provider.removeAllListeners('pending');
                if (await this.checkSlippage(tokenAddress, 0.1)) {
                    const amount = await this.buyToken(tokenAddress, 0.1);
                    this.notifyUser(`[SMART] Front-run Buy: ${tokenAddress}`, { amount: 0.1, price: startPrice });
                    await this.smartTradeWithTSL(tokenAddress, startPrice, '🟢');
                }
            }
        });
    }

    async smartTradeWithTSL(tokenAddress, startPrice, symbol) {
        if (await this.checkSlippage(tokenAddress, 0.1)) {
            const amount = await this.buyToken(tokenAddress, 0.1);
            this.notifyUser(`[SMART] Buy: ${tokenAddress}`, { amount: 0.1, price: startPrice });

            let highestPrice = startPrice;
            const interval = setInterval(async () => {
                const currentPrice = await this.getPrice(tokenAddress);
                if (currentPrice > highestPrice) highestPrice = currentPrice;
                if (currentPrice < highestPrice * (1 - this.tslPercentage)) {
                    await this.sellAll(tokenAddress);
                    this.updateProfitLoss(currentPrice, startPrice, amount);
                    this.notifyUser(`[SMART] TSL Sell: ${tokenAddress}`, { price: currentPrice });
                    clearInterval(interval);
                }
            }, 60 * 1000);
        }
    }

    async getAuditScore(tokenAddress) {
        try {
            const res = await apiRequest(`https://api.gopluslabs.io/api/v1/token_security/${tokenAddress}`, 'GET');
            return res.data.auditScore || 80;
        } catch {
            return 80;
        }
    }

    async getLiquidity(tokenAddress) {
        return 10000;
    }

    async isSellLocked(tokenAddress) {
        try {
            const tokenContract = new ethers.Contract(tokenAddress, ['function transfer(address,uint256) returns (bool)'], this.wallet);
            await tokenContract.callStatic.transfer(this.wallet.address, ethers.utils.parseEther('0.001'));
            return false;
        } catch {
            return true;
        }
    }

    async getTax(tokenAddress) {
        return { buy: 2, sell: 2 };
    }

    async isContractVerified(tokenAddress) {
        return true;
    }

    async canMintUnlimited(tokenAddress) {
        const tokenContract = new ethers.Contract(tokenAddress, ['function mint(address,uint256)'], provider);
        return !!tokenContract.mint;
    }

    async isPausable(tokenAddress) {
        const tokenContract = new ethers.Contract(tokenAddress, ['function pause()'], provider);
        return !!tokenContract.pause;
    }

    async checkOwnership(tokenAddress) {
        const tokenContract = new ethers.Contract(tokenAddress, ['function totalSupply() view returns (uint256)', 'function balanceOf(address) view returns (uint256)'], provider);
        const totalSupply = await tokenContract.totalSupply();
        const ownerBalance = await tokenContract.balanceOf(this.wallet.address);
        return (ownerBalance / totalSupply) * 100;
    }

    async getPrice(tokenAddress) {
        return (await apiRequest(`/api/price?pair=${tokenAddress}/ETH`, 'GET')) || 1;
    }

    async getPriceFromDEX(tokenAddress, dex) {
        return dex === 'Uniswap' ? 1 : 1.05;
    }

    async buyToken(tokenAddress, amount, poolId = null) {
        const pool = poolId ? await this.sdk.Pool.getPool(poolId) : await this.getPool(this.suiCoinType, tokenAddress);
        const tx = await this.sdk.Swap.createSwapTransaction({
            pool,
            a2b: true, // SUI -> Token
            amountIn: amount * 10**9, // SUI decimals
            amountOutMin: 0,
            slippage: this.maxSlippage
        });
        const result = await this.sdk.sendAndConfirmTransaction(tx);
        console.log(`[BUY] ${tokenAddress} - ${amount} SUI on Cetus`);
        return amount;
    }

    async sellToken(tokenAddress, percentage, poolId = null) {
        const pool = poolId ? await this.sdk.Pool.getPool(poolId) : await this.getPool(tokenAddress, this.suiCoinType);
        const balance = await this.getBalance(tokenAddress);
        const amountIn = balance * (percentage / 100);
        const tx = await this.sdk.Swap.createSwapTransaction({
            pool,
            a2b: false, // Token -> SUI
            amountIn: amountIn * 10**9,
            amountOutMin: 0,
            slippage: this.maxSlippage
        });
        const result = await this.sdk.sendAndConfirmTransaction(tx);
        console.log(`[SELL] ${tokenAddress} - ${percentage}% (${amountIn} tokens) on Cetus`);
        return percentage; // Sửa trả về percentage thay vì amount
    }

    async sellAll(tokenAddress) {
        const balance = await this.getBalance(tokenAddress);
        if (balance > 0) {
            console.log(`[SELL] ${tokenAddress} - 100%`);
            return balance;
        }
    }

    async getBalance(tokenAddress) {
        const coinType = tokenAddress;
        const balance = await this.sdk.client.getBalance({
            owner: this.keypair.getPublicKey().toSuiAddress(),
            coinType
        });
        return balance.totalBalance / 10**9; // Chuyển từ smallest unit về đơn vị thông thường
    }

    async getPool(tokenIn, tokenOut) {
        const pools = await this.sdk.Pool.getPools({});
        const pool = pools.find(p => 
            (p.coinTypeA === tokenIn && p.coinTypeB === tokenOut) || 
            (p.coinTypeA === tokenOut && p.coinTypeB === tokenIn)
        );
        if (!pool) throw new Error(`Pool not found for ${tokenIn} <-> ${tokenOut}`);
        return pool;
    }

    async checkSlippage(tokenAddress, amount) {
        const expectedPrice = await this.getPrice(tokenAddress);
        const simulatedPrice = await this.simulateSwap(tokenAddress, amount);
        const slippage = (simulatedPrice - expectedPrice) / expectedPrice;
        return slippage < this.maxSlippage;
    }

    async simulateSwap(tokenAddress, amount) {
        return (await this.getPrice(tokenAddress)) * 1.01;
    }

    updateProfitLoss(sellPrice, buyPrice, amount) {
        const profit = (sellPrice - buyPrice) * amount;
        if (profit > 0) this.profitLoss.profit += profit;
        else this.profitLoss.loss -= profit;
    }

    notifyUser(message, data) {
        if (io) io.emit('tradeHistory', { type: message, ...data, chain: 'sui', timestamp: Date.now() });
        publishTrade('snipebot/ton/trades', { type: message, ...data });
    console.log(`[NOTIFY] ${message}: ${JSON.stringify(data)}`);
}
}

module.exports = { SnipeBot };