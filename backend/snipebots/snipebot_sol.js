const { Connection, Keypair, PublicKey, Transaction, SystemProgram, sendAndConfirmTransaction } = require('@solana/web3.js');
const { TOKEN_PROGRAM_ID, getAssociatedTokenAddress, createAssociatedTokenAccountInstruction } = require('@solana/spl-token');
const { Liquidity, TokenAmount, Percent, Token, WSOL } = require('@raydium-io/raydium-sdk');
const { io } = require('../server');
const MevMode = require('./mevMode');
const DepinManager = require('../depin/depinManager');
const axios = require('axios');

const depinManager = new DepinManager();

class SnipeBot {
    constructor(privateKey, rpcUrl) {
        this.connection = new Connection(rpcUrl || 'https://api.mainnet-beta.solana.io', 'confirmed');
        this.keypair = Keypair.fromSecretKey(Buffer.from(privateKey, 'hex'));
        this.mev = new MevMode(this);
        this.nodeId = 'sol-node'; // Giả định node ID cho DePIN
        this.isMonitoring = false;
        this.isMonitoringSmart = false;
        this.profitLoss = { profit: 0, loss: 0 };
        this.minProfit = 0.01;
        this.maxSlippage = 0.05;
        this.tslPercentage = 0.05;

        // Birdeye API setup
        this.birdeyeApiKey = process.env.BIRDEYE_API_KEY || 'your-birdeye-api-key';
        this.birdeyeEndpoint = 'https://public-api.birdeye.so';
        this.birdeyeHeaders = { 'X-API-KEY': this.birdeyeApiKey };

        // Raydium setup
        this.wsolMint = WSOL.mint; // WSOL trên Solana
    }

    async buyToken(tokenAddress, amount) {
        try {
            const tokenMint = new PublicKey(tokenAddress);
            const wsolMint = new PublicKey(this.wsolMint);
            const wallet = this.keypair.publicKey;

            // Tạo Associated Token Account (ATA) cho token nếu chưa có
            const tokenAta = await getAssociatedTokenAddress(tokenMint, wallet);
            const wsolAta = await getAssociatedTokenAddress(wsolMint, wallet);

            const tokenAccountInfo = await this.connection.getAccountInfo(tokenAta);
            const wsolAccountInfo = await this.connection.getAccountInfo(wsolAta);

            let transaction = new Transaction();

            if (!wsolAccountInfo) {
                transaction.add(
                    createAssociatedTokenAccountInstruction(
                        wallet,
                        wsolAta,
                        wallet,
                        wsolMint
                    )
                );
            }

            if (!tokenAccountInfo) {
                transaction.add(
                    createAssociatedTokenAccountInstruction(
                        wallet,
                        tokenAta,
                        wallet,
                        tokenMint
                    )
                );
            }

            // Gửi WSOL vào ATA để chuẩn bị swap
            const amountInLamports = Math.floor(amount * 1e9); // 1 SOL = 1e9 lamports
            transaction.add(
                SystemProgram.transfer({
                    fromPubkey: wallet,
                    toPubkey: wsolAta,
                    lamports: amountInLamports,
                }),
                createSyncNativeInstruction(wsolAta)
            );

            // Swap trên Raydium
            const poolKeys = await this.getPoolKeys(tokenAddress); // Giả định lấy pool keys từ Raydium
            const amountIn = new TokenAmount(new Token(WSOL.mint, 9), amountInLamports, false);
            const amountOutMin = new TokenAmount(new Token(tokenMint, 9), 0); // Tính chính xác hơn nếu cần
            const slippage = new Percent(Math.floor(this.maxSlippage * 100), 100);

            const { transaction: swapTx, signers } = await Liquidity.makeSwapInstructionSimple({
                connection: this.connection,
                poolKeys,
                userKeys: {
                    tokenAccounts: [],
                    owner: wallet,
                    payer: wallet,
                },
                amountIn,
                amountOut: amountOutMin,
                fixedSide: 'in',
                makeTxVersion: 0,
                computeBudgetConfig: { microLamports: 25000 }
            });

            transaction.add(...swapTx.instructions);

            const recentBlockhash = await this.connection.getLatestBlockhash();
            transaction.recentBlockhash = recentBlockhash.blockhash;
            transaction.feePayer = wallet;

            const signature = await sendAndConfirmTransaction(
                this.connection,
                transaction,
                [this.keypair, ...signers]
            );

            const buyPrice = await this.getTokenPrice(tokenAddress);
            const tradeData = { tokenAddress, buyAmount: amount, buyPrice, timestamp: Date.now() };
            const cid = await depinManager.storeData(this.nodeId, tradeData);
            this.notifyUser('Buy', { tokenAddress, amount, buyPrice, cid });

            this.profitLoss.profit += buyPrice * amount * this.minProfit;
            return { buyAmount: amount, buyPrice };
        } catch (error) {
            console.error('[SOL Buy Error]', error.message);
            throw error;
        }
    }

    async sellToken(tokenAddress, percentage) {
        try {
            const tokenMint = new PublicKey(tokenAddress);
            const wsolMint = new PublicKey(this.wsolMint);
            const wallet = this.keypair.publicKey;

            const tokenAta = await getAssociatedTokenAddress(tokenMint, wallet);
            const wsolAta = await getAssociatedTokenAddress(wsolMint, wallet);

            const tokenAccountInfo = await this.connection.getTokenAccountBalance(tokenAta);
            const balance = tokenAccountInfo.value.uiAmount;
            const sellAmount = Math.floor(balance * percentage * 1e9); // 1 token = 1e9 (giả định 9 decimals)

            // Swap trên Raydium
            const poolKeys = await this.getPoolKeys(tokenAddress);
            const amountIn = new TokenAmount(new Token(tokenMint, 9), sellAmount, false);
            const amountOutMin = new TokenAmount(new Token(WSOL.mint, 9), 0);
            const slippage = new Percent(Math.floor(this.maxSlippage * 100), 100);

            const { transaction: swapTx, signers } = await Liquidity.makeSwapInstructionSimple({
                connection: this.connection,
                poolKeys,
                userKeys: {
                    tokenAccounts: [],
                    owner: wallet,
                    payer: wallet,
                },
                amountIn,
                amountOut: amountOutMin,
                fixedSide: 'in',
                makeTxVersion: 0,
                computeBudgetConfig: { microLamports: 25000 }
            });

            const transaction = new Transaction().add(...swapTx.instructions);
            const recentBlockhash = await this.connection.getLatestBlockhash();
            transaction.recentBlockhash = recentBlockhash.blockhash;
            transaction.feePayer = wallet;

            const signature = await sendAndConfirmTransaction(
                this.connection,
                transaction,
                [this.keypair, ...signers]
            );

            const sellPrice = await this.getTokenPrice(tokenAddress);
            const tradeData = { tokenAddress, sellAmount: (sellAmount / 1e9).toString(), sellPrice, timestamp: Date.now() };
            const cid = await depinManager.storeData(this.nodeId, tradeData);
            this.notifyUser('Sell', { tokenAddress, amount: (sellAmount / 1e9).toString(), sellPrice, cid });

            this.profitLoss.loss += sellPrice * (sellAmount / 1e9) * (1 - this.tslPercentage);
            return (sellAmount / 1e9).toString();
        } catch (error) {
            console.error('[SOL Sell Error]', error.message);
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
                console.error('[SOL MEV Error]', error.message);
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
                console.error('[SOL Smart Trade Error]', error.message);
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
            const response = await axios.get(`${this.birdeyeEndpoint}/defi/price?address=${tokenAddress}`, {
                headers: this.birdeyeHeaders
            });
            const price = response.data.data.price || 0;
            return parseFloat(price);
        } catch (error) {
            console.error('[SOL getTokenPrice Error]', error.message);
            return 0;
        }
    }

    async fetchMempoolTrades() {
        try {
            const response = await axios.get(`${this.birdeyeEndpoint}/defi/recent_trades?address=${this.wsolMint}`, {
                headers: this.birdeyeHeaders
            });
            const trades = response.data.data || [];
            return trades.map(trade => ({
                tokenAddress: trade.token_address,
                amount: parseFloat(trade.amount),
                price: parseFloat(trade.price)
            }));
        } catch (error) {
            console.error('[SOL fetchMempoolTrades Error]', error.message);
            return [];
        }
    }

    async getPoolKeys(tokenAddress) {
        // Giả định lấy pool keys từ Raydium (trong thực tế cần fetch từ Raydium API hoặc on-chain data)
        return {
            id: new PublicKey('RAYDIUM_POOL_ID'), // Thay bằng pool ID thực tế
            baseMint: new PublicKey(tokenAddress),
            quoteMint: new PublicKey(this.wsolMint),
            lpMint: new PublicKey('LP_MINT_ADDRESS'), // Thay bằng lpMint thực tế
            // Các trường khác của poolKeys
        };
    }

    notifyUser(message, data) {
        if (io) {
            io.emit('tradeHistory', { type: message, ...data, chain: 'sol', timestamp: Date.now() });
        }
        console.log(`[NOTIFY] ${message}: ${JSON.stringify(data)}`);
    }
}

// Hàm helper để sync WSOL
function createSyncNativeInstruction(account) {
    return new TransactionInstruction({
        keys: [
            { pubkey: account, isSigner: false, isWritable: true },
        ],
        programId: new PublicKey('SystemProgram'),
        data: Buffer.from([17]), // Instruction code for SyncNative
    });
}

module.exports = { SnipeBot };