// backend/controllers/snipebotController.js
const { SnipeBot: SnipeBotSOL } = require('../snipebots/snipebot_sol');
const { SnipeBot: SnipeBotBSC } = require('../snipebots/snipebot_bsc');
const { SnipeBot: SnipeBotBASE } = require('../snipebots/snipebot_base');
const { SnipeBot: SnipeBotARB } = require('../snipebots/snipebot_arb');
const { SnipeBot: SnipeBotNEAR } = require('../snipebots/snipebot_near');
const { SnipeBot: SnipeBotETH } = require('../snipebots/snipebot_eth');
const { SnipeBot: SnipeBotTON } = require('../snipebots/snipebot_ton');
const { SnipeBot: SnipeBotSUI } = require('../snipebots/snipebot_sui');
const { SnipeBot: SnipeBotAVAX } = require('../snipebots/snipebot_avax');
const { SnipeBot: SnipeBotBTC } = require('../snipebots/snipebot_btc');
const { SnipeBot: SnipeBotPI } = require('../snipebots/snipebot_pi');
const { SnipeBot: SnipeBotSTELLAR } = require('../snipebots/snipebot_stellar');
const { logSnipe } = require('../models/snipebot');
const DepinManager = require('../depin/depinManager');
const SentimentAnalyzerModule = require('../sentimentAnalyzer');
const MempoolSharder = require('../mempoolSharder');
const GasOptimizer = require('../gasOptimizer');
const RiskAnalyzer = require('../riskAnalyzer');
const { logger } = require('../utils/logger');

class SnipebotController {
async smartBuy(req) {
    try {
        const { chain, tokenAddress, amount, userId, key } = req.body;
        if (!chain || !tokenAddress || !amount) throw new Error('Missing required fields');

        const fee = await this.checkFeeAndKey(userId, 'smart', key);
        if (!fee.paid) throw new Error('Payment or key required');

        const bot = this.getSnipeBot(chain);
        const gasParams = await this.gasOptimizer.optimizeGas(chain, 50, await bot.getMempool(), bot);
        const result = await bot.smartBuy(tokenAddress, amount, gasParams);

        await this.logForTraining(userId, { mode: 'smartBuy', chain, tokenAddress, amount, result });
        return { status: 'Smart Buy successful', result };
    } catch (error) {
        logger.error(`Error in smartBuy: ${error.message}`);
        throw error;
    }
};
async smartSell(req) {
    try {
        const { chain, tokenAddress, percentage, userId, key } = req.body;
        if (!chain || !tokenAddress || !percentage) throw new Error('Missing required fields');

        const fee = await this.checkFeeAndKey(userId, 'smart', key);
        if (!fee.paid) throw new Error('Payment or key required');

        const bot = this.getSnipeBot(chain);
        const gasParams = await this.gasOptimizer.optimizeGas(chain, 50, await bot.getMempool(), bot);
        const result = await bot.smartSell(tokenAddress, percentage, gasParams);

        await this.logForTraining(userId, { mode: 'smartSell', chain, tokenAddress, percentage, result });
        return { status: 'Smart Sell successful', result };
    } catch (error) {
        logger.error(`Error in smartSell: ${error.message}`);
        throw error;
    }
};

async triggerPremium(req) {
    try {
        const { chain, tokenAddress, userId, key } = req.body;
        if (!chain || !tokenAddress) throw new Error('Missing required fields');

        const fee = await this.checkFeeAndKey(userId, 'premium', key);
        if (!fee.paid) throw new Error('Payment or key required');

        const bot = this.getSnipeBot(chain);
        const gasParams = await this.gasOptimizer.optimizeGas(chain, 50, await bot.getMempool(), bot);
        const smartResult = await bot.smart(tokenAddress, gasParams);
        const mevResult = await bot.monitorMempoolMEV(tokenAddress, null, gasParams);

        setTimeout(async () => {
            await bot.stopSnipe(tokenAddress);
            await bot.smartSell(tokenAddress, 100, gasParams);
            logger.info(`Premium mode auto-stopped for ${tokenAddress} on chain ${chain}`);
        }, 30 * 60 * 1000); // 30 phút

        await this.logForTraining(userId, { mode: 'premium', chain, tokenAddress, smartResult, mevResult });
        return { status: 'Premium mode started', smartResult, mevResult };
    } catch (error) {
        logger.error(`Error in triggerPremium: ${error.message}`);
        throw error;
    }
};

async triggerUltimate(req) {
    try {
        const { chain, tokenAddress, userId, key } = req.body;
        if (!chain || !tokenAddress) throw new Error('Missing required fields');

        const usage = await this.checkUsageLimit(userId, 'ultimate');
        if (usage >= 1) throw new Error('Ultimate mode usage limit reached');

        const fee = await this.checkFeeAndKey(userId, 'ultimate', key);
        if (!fee.paid) throw new Error('Payment or key required');

        const bot = this.getSnipeBot(chain);
        const gasParams = await this.gasOptimizer.optimizeGas(chain, 50, await bot.getMempool(), bot);
        const smartResult = await bot.smart(tokenAddress, gasParams);
        const mevResult = await bot.monitorMempoolMEV(tokenAddress, null, gasParams);
        const sseResult = await bot.monitorMempoolSSE(tokenAddress);

        await this.logForTraining(userId, { mode: 'ultimate', chain, tokenAddress, smartResult, mevResult, sseResult });
        await this.updateUsageLimit(userId, 'ultimate');
        return { status: 'Ultimate mode executed', smartResult, mevResult, sseResult };
    } catch (error) {
        logger.error(`Error in triggerUltimate: ${error.message}`);
        throw error;
    }
};

async checkFeeAndKey(userId, mode, key) {
    const fees = { smart: 0.01, premium: 0.05, ultimate: 0.1 };
    if (key) {
        // Giả lập kiểm tra key
        return { paid: key === 'valid-key' };
    }
    // Giả lập kiểm tra phí
    return { paid: true }; // Thay bằng logic thanh toán thực tế
};

async checkUsageLimit(userId, mode) {
    // Giả lập kiểm tra giới hạn sử dụng
    return 0; // Thay bằng logic thực tế
};

async updateUsageLimit(userId, mode) {
    // Giả lập cập nhật giới hạn sử dụng
};

async logForTraining(userId, logData) {
    try {
        await logSnipe.create({ userId, ...logData });
        // Gửi log qua gRPC để training AI
        const grpcClient = require('../grpc/grpcServer');
        grpcClient.sendLogForTraining({ userId, logData: JSON.stringify(logData) });
    } catch (error) {
        logger.error(`Error logging for training: ${error.message}`);
    }
};

    constructor() {
        this.bots = new Map();
        this.sentimentAnalyzer = new SentimentAnalyzerModule();
        this.depinManager = new DepinManager();
        this.sentimentCache = new Map();
        this.mempoolSharder = new MempoolSharder();
        this.gasOptimizer = new GasOptimizer();
        this.riskAnalyzer = new RiskAnalyzer();
        this.supportedChains = [
            'sol', 'bsc', 'base', 'arb', 'near', 'eth', 'ton', 'sui', 'avax', 'btc', 'pi', 'stellar'
        ];
    }

    async startSnipebot(chain) {
        try {
            if (!this.supportedChains.includes(chain)) {
                throw new Error(`Unsupported chain: ${chain}`);
            }

            const bot = this.bots.get(chain);
            if (!bot) throw new Error(`Bot for chain ${chain} not found`);

            if (bot.isMonitoring || bot.isMonitoringSmart) {
                logger.warn(`Snipebot on chain ${chain} is already running`);
                return;
            }
            if (this.depinManager.isMaster) {
                const adjustments = await this.riskAnalyzer.adjustStrategy();
                bot.maxSlippage = adjustments.maxSlippage;
                bot.minProfit = adjustments.minProfit;
                logger.info(`Adjusted strategy for chain ${chain}: ${JSON.stringify(adjustments)}`);
            }

            const tokens = await this.fetchTokens(chain);
            const prioritizedTokens = await this.prioritizeTokens(tokens);
            
            const mempool = await bot.getMempool();
            const shardedMempool = this.mempoolSharder.shardMempool(mempool);
            const highPriorityShard = this.mempoolSharder.getShard(3);
            
            // Lấy mempool và chia shard
            for (const token of prioritizedTokens) {
                const gasParams = await this.gasOptimizer.optimizeGas(chain, 50, highPriorityShard);
                // Gửi giao dịch qua Slave node (relay)
                if (this.depinManager.isMaster) {
                    const task = {
                        id: `relay-${Date.now()}`,
                        type: 'relay-transaction',
                        chain,
                        tokenAddress: token.address,
                        gasParams,
                    };
                    await this.depinManager.assignTasks([task]);
                } else {
                    await new Promise(resolve => setTimeout(resolve, gasParams.timingDelay));
                    await bot.monitorMempoolMEV(token.address, highPriorityShard, gasParams);
                }
            }
            logger.info(`Started sniping on chain ${chain} with prioritized tokens in shard 3`);
        } catch (error) {
            logger.error(`Error starting snipebot on chain ${chain}: ${error.message}`);
            throw error;
        }
    }

    async fetchTokens(chain) {
        // Giả lập lấy danh sách token
        return [
            { symbol: 'TOKEN1', address: '0x123' },
            { symbol: 'TOKEN2', address: '0x456' },
        ];
    }

    async prioritizeTokens(tokens) {
        try {
            const scoredTokens = [];
            for (const token of tokens) {
                let sentimentScore = this.sentimentCache.get(token.symbol);
                if (!sentimentScore) {
                    sentimentScore = await this.sentimentAnalyzer.getSentimentScore(token.symbol);
                    this.sentimentCache.set(token.symbol, sentimentScore);
                }
                await this.sentimentAnalyzer.sendSentimentToMobile(token.symbol, sentimentScore);
                scoredTokens.push({ ...token, sentimentScore, combinedScore: sentimentScore });
            }
            return scoredTokens.sort((a, b) => b.combinedScore - a.combinedScore);
        } catch (error) {
            logger.error(`Error prioritizing tokens: ${error.message}`);
            return tokens;
        }
    }

    getSnipeBot(chain) {
        return this.bots.get(chain);
    }
}
const bots = {};

const getSnipeBot = (chain) => {
    try {
        const chainMap = {
            'sol': SnipeBotSOL,
            'bsc': SnipeBotBSC,
            'base': SnipeBotBASE,
            'arb': SnipeBotARB,
            'near': SnipeBotNEAR,
            'eth': SnipeBotETH,
            'ton': SnipeBotTON,
            'sui': SnipeBotSUI,
            'avax': SnipeBotAVAX,
            'btc': SnipeBotBTC,
            'pi': SnipeBotPI,
            'stellar': SnipeBotSTELLAR,
        };

        const BotClass = chainMap[chain];
        if (!BotClass) throw new Error(`Unsupported chain: ${chain}`);

        if (!bots[chain]) {
            const privateKey = process.env.PRIVATE_KEY;
            const privateRpc = process.env[`PRIVATE_RPC_${chain.toUpperCase()}`] || null;
            if (!privateKey) {
                logger.error(`PRIVATE_KEY not found in environment variables for chain ${chain}`);
                throw new Error(`PRIVATE_KEY not found for chain ${chain}`);
            }
            if (!privateRpc) {
                logger.warn(`PRIVATE_RPC_${chain.toUpperCase()} not found, using default RPC`);
            }
            bots[chain] = new BotClass(privateKey, privateRpc);
            bots[chain].monitorMempoolSmart();
            const controller = new SnipebotController();
            controller.bots.set(chain, bots[chain]);
        }
        return bots[chain];
    } catch (error) {
        logger.error(`Error getting snipebot for chain ${chain}: ${error.message}`);
        throw error;
    }
};

const triggerManualBuy = async (req) => {
    try {
        const { chain, tokenAddress, amount, poolId } = req.body;
        if (!chain || !tokenAddress || !amount || !poolId) {
            throw new Error('Missing required fields: chain, tokenAddress, amount, poolId');
        }

        const controller = new SnipebotController();
        const bot = controller.getSnipeBot(chain);
        const gasParams = await controller.gasOptimizer.optimizeGas(chain, 50);
        const { buyAmount, buyPrice } = await bot.buyToken(tokenAddress, amount, poolId, gasParams);
        await logSnipe({
            userId: req.user?.userId || 'default-user',
            tokenAddress,
            chain,
            mode: 'manual',
            buyAmount,
            buyPrice: buyPrice || 0,
            status: 'completed',
        });
        return { status: `Manual Buy completed on ${chain.toUpperCase()}`, buyAmount, buyPrice, gasParams };
    } catch (error) {
        logger.error(`Error in triggerManualBuy: ${error.message}`);
        throw error;
    }
};

const triggerManualSell = async (req) => {
    try {
        const { chain, tokenAddress, percentage } = req.body;
        if (!chain || !tokenAddress || !percentage || percentage <= 0 || percentage > 100) {
            throw new Error('Invalid input: chain, tokenAddress, percentage (must be between 0 and 100)');
        }

        const bot = getSnipeBot(chain);
        const sellAmount = await bot.sellToken(tokenAddress, percentage);
        await logSnipe({
            userId: req.user?.userId || 'default-user',
            tokenAddress,
            chain,
            mode: 'manual-sell',
            buyAmount: 0,
            buyPrice: 0,
            status: 'completed',
        });
        return { status: `Manual Sell completed on ${chain.toUpperCase()}`, sellAmount };
    } catch (error) {
        logger.error(`Error in triggerManualSell: ${error.message}`);
        throw error;
    }
};

const triggerSnipeMEV = async (req) => {
    try {
        const { chain, tokenAddress } = req.body;
        if (!chain || !tokenAddress) {
            throw new Error('Missing required fields: chain, tokenAddress');
        }
        
        const controller = new SnipebotController();
        const bot = controller.getSnipeBot(chain);
        if (bot.isMonitoring) {
            return { status: `MEV Snipebot already running on ${chain.toUpperCase()}` };
        }

        const mempool = await bot.getMempool();
        const gasParams = await controller.gasOptimizer.optimizeGas(chain, 50);
        const mevProcess = bot.monitorMempoolMEV(tokenAddress, null, gasParams);
        mevProcess.then(async ({ tokenAddress: mevTokenAddress, buyAmount, buyPrice }) => {
            await logSnipe({
                userId: req.user?.userId || 'default-user',
                tokenAddress: mevTokenAddress || 'N/A',
                chain,
                mode: 'mev',
                buyAmount: buyAmount || 0,
                buyPrice: buyPrice || 0,
                status: 'completed',
            });
        }).catch(error => {
            logger.error(`[MEV Snipe Error] ${chain}: ${error.message}`);
        });
        return { status: `MEV Snipebot started on ${chain.toUpperCase()}`, gasParams };
    } catch (error) {
        logger.error(`Error in triggerSnipeMEV: ${error.message}`);
        throw error;
    }
};

const triggerSnipeSmart = async (req) => {
    try {
        const { chain } = req.body;
        if (!chain) {
            throw new Error('Missing required field: chain');
        }

        const bot = getSnipeBot(chain);
        if (bot.isMonitoringSmart) {
            return { status: `Smart Buy/Sell already running on ${chain.toUpperCase()}` };
        }

        bot.monitorMempoolSmart();
        return { status: `Smart Buy/Sell running on ${chain.toUpperCase()}` };
    } catch (error) {
        logger.error(`Error in triggerSnipeSmart: ${error.message}`);
        throw error;
    }
};

const getProfitLoss = async (req) => {
    try {
        const { chain } = req.params;
        if (!chain) {
            throw new Error('Missing required parameter: chain');
        }

        const bot = getSnipeBot(chain);
        return bot.profitLoss;
    } catch (error) {
        logger.error(`Error in getProfitLoss: ${error.message}`);
        throw error;
    }
};

const stopSnipe = async (req) => {
    try {
        const { chain, tokenAddress } = req.body;
        if (!chain || !tokenAddress) {
            throw new Error('Missing required fields: chain, tokenAddress');
        }

        const bot = getSnipeBot(chain);
        await bot.sellAll(tokenAddress);
        bot.stopMonitoring();
        const log = await logSnipe({
            userId: req.user?.userId || 'default-user',
            tokenAddress,
            chain,
            mode: 'stop',
            buyAmount: 0,
            buyPrice: 0,
            status: 'completed',
        });
        return { status: `Snipe stopped on ${chain.toUpperCase()}`, profitLoss: bot.profitLoss, logId: log._id };
    } catch (error) {
        logger.error(`Error in stopSnipe: ${error.message}`);
        throw error;
    }
};

const updateSettings = async (req) => {
    try {
        const { chain, minProfit, maxSlippage, tslPercentage } = req.body;
        if (!chain || minProfit == null || maxSlippage == null || tslPercentage == null) {
            throw new Error('Missing required fields: chain, minProfit, maxSlippage, tslPercentage');
        }

        const bot = getSnipeBot(chain);
        bot.minProfit = parseFloat(minProfit) || bot.minProfit;
        bot.maxSlippage = parseFloat(maxSlippage) || bot.maxSlippage;
        bot.tslPercentage = parseFloat(tslPercentage) || bot.tslPercentage;
        return { status: `Settings updated on ${chain.toUpperCase()}` };
    } catch (error) {
        logger.error(`Error in updateSettings: ${error.message}`);
        throw error;
    }
};

const getBotStatus = async (req) => {
    try {
        const { chain } = req.params;
        if (!chain) {
            throw new Error('Missing required parameter: chain');
        }

        const bot = getSnipeBot(chain);
        const isRunning = bot.isMonitoring || bot.isMonitoringSmart;
        return { status: isRunning ? 'Running' : 'Stopped' };
    } catch (error) {
        logger.error(`Error in getBotStatus: ${error.message}`);
        throw error;
    }
};

const triggerSnipeSSE = (req, res) => {
    try {
        res.setHeader('Content-Type', 'text/event-stream');
        res.setHeader('Cache-Control', 'no-cache');
        res.setHeader('Connection', 'keep-alive');

        const { chain } = req.params;
        if (!chain) {
            res.status(400).end('Missing required parameter: chain');
            return;
        }

        const bot = getSnipeBot(chain);
        const interval = setInterval(() => {
            const isRunning = bot.isMonitoring || bot.isMonitoringSmart;
            res.write(`data: ${JSON.stringify({ chain, status: isRunning ? 'Running' : 'Stopped' })}\n\n`);
        }, 5000);

        req.on('close', () => clearInterval(interval));
    } catch (error) {
        logger.error(`Error in triggerSnipeSSE: ${error.message}`);
        res.status(500).end('SSE Error');
    }
};

const getTradeFromIPFS = async (req) => {
    try {
        const { cid } = req.params;
        if (!cid) {
            throw new Error('Missing required parameter: cid');
        }

        const depinManager = new DepinManager();
        const data = await depinManager.fetchData('ton-node', cid);
        return data;
    } catch (error) {
        logger.error(`Error in getTradeFromIPFS: ${error.message}`);
        throw error;
    }
};

const updateRiskParameters = async (req) => {
    try {
        const { maxLoss, confidenceLevel } = req.body;
        if (maxLoss == null || confidenceLevel == null) {
            throw new Error('Missing required fields: maxLoss, confidenceLevel');
        }

        if (maxLoss < 0 || maxLoss > 1 || confidenceLevel < 0 || confidenceLevel > 1) {
            throw new Error('Invalid parameters: maxLoss and confidenceLevel must be between 0 and 1');
        }

        const controller = new SnipebotController();
        if (!controller.depinManager.isMaster) {
            throw new Error('Only Master node can update risk parameters');
        }

        controller.riskAnalyzer.updateRiskParameters({ maxLoss, confidenceLevel });
        return { status: 'Risk parameters updated successfully' };
    } catch (error) {
        logger.error(`Error in updateRiskParameters: ${error.message}`);
        throw error;
    }
};

const getTradeHistory = async (req) => {
    try {
        const { chain } = req.params;
        if (!chain) {
            throw new Error('Missing required parameter: chain');
        }

        const logs = await logSnipe.find({ chain, status: 'completed' }).sort({ createdAt: -1 }).limit(50);
        return logs;
    } catch (error) {
        logger.error(`Error in getTradeHistory: ${error.message}`);
        throw error;
    }
};

const getRiskData = async (req) => {
    try {
        const controller = new SnipebotController();
        if (!controller.depinManager.isMaster) {
            throw new Error('Only Master node can fetch risk data');
        }

        const varValue = await controller.riskAnalyzer.calculateVaR();
        const historicalReturns = await controller.riskAnalyzer.fetchHistoricalReturns();
        return { varValue, historicalReturns };
    } catch (error) {
        logger.error(`Error in getRiskData: ${error.message}`);
        throw error;
    }
};

const getPNL = async (req) => {
    try {
        const { userId, period } = req.query; // period: '24h', '1d', '7d'
        const now = new Date();
        let startTime;

        if (period === '24h') startTime = new Date(now.getTime() - 24 * 60 * 60 * 1000);
        else if (period === '1d') startTime = new Date(now.getTime() - 24 * 60 * 60 * 1000);
        else if (period === '7d') startTime = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000);
        else throw new Error('Invalid period');

        const trades = await logSnipe.find({
            userId,
            createdAt: { $gte: startTime },
            status: 'completed',
        });

        let totalPNL = 0;
        trades.forEach(trade => {
            if (trade.sellPrice && trade.buyPrice) {
                const profit = (trade.sellPrice - trade.buyPrice) * trade.buyAmount;
                totalPNL += profit;
            }
        });

        return { period, totalPNL, trades };
    } catch (error) {
        logger.error(`Error in getPNL: ${error.message}`);
        throw error;
    }
};



module.exports = {
    triggerSnipeSSE,
    triggerManualBuy,
    triggerManualSell,
    triggerSnipeMEV,
    triggerSnipeSmart,
    getProfitLoss,
    stopSnipe,
    updateSettings,
    getBotStatus,
    getTradeFromIPFS,
    getSnipeBot,
    updateRiskParameters,
    getTradeHistory,
    getRiskData,
    smartBuy: (req) => new SnipebotController().smartBuy(req),
    smartSell: (req) => new SnipebotController().smartSell(req),
    triggerPremium: (req) => new SnipebotController().triggerPremium(req),
    triggerUltimate: (req) => new SnipebotController().triggerUltimate(req),
    getPNL,

};