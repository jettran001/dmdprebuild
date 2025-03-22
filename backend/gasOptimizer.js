// backend/gasOptimizer.js
const { logger } = require('./utils/logger');

class GasOptimizer {
    constructor() {
        this.historicalGasPrices = [];
        this.simulationRuns = 1000;
    }

    async fetchHistoricalGasPrices(chain, bot) {
        try {
            const mempool = await bot.getMempool();
            const gasPrices = mempool.map(tx => parseInt(tx.gasPrice || tx.maxFeePerGas || '0', 16) / 1e9);
            this.historicalGasPrices = gasPrices.length > 0 ? gasPrices : Array.from({ length: 100 }, () => Math.random() * 100 + 20);
            return this.historicalGasPrices;
        } catch (error) {
            logger.error(`Error fetching historical gas prices: ${error.message}`);
            return Array.from({ length: 100 }, () => Math.random() * 100 + 20);
        }
    }

    // Mô phỏng lý thuyết trò chơi để tìm gas price tối ưu
    async predictOptimalGasPriceWithGameTheory(chain, bot) {
        try {
            const mempoolData = await bot.getMempool();
            if (this.historicalGasPrices.length === 0) {
                await this.fetchHistoricalGasPrices(chain, bot);
            }

            const competingGasPrices = mempoolData.map(tx => parseInt(tx.gasPrice || tx.maxFeePerGas || '0', 16) / 1e9);
            const blockTime = await bot.getAverageBlockTime(); // Giả lập: Lấy thời gian trung bình của block

            const simulations = [];
            for (let i = 0; i < this.simulationRuns; i++) {
                const randomIndex = Math.floor(Math.random() * this.historicalGasPrices.length);
                const basePrice = this.historicalGasPrices[randomIndex];
                const volatility = (Math.random() - 0.5) * 10;

                const gasPrice = basePrice + volatility;
                const successProbability = this.calculateSuccessProbability(gasPrice, competingGasPrices);
                const cost = gasPrice;
                const timingDelay = this.calculateTimingDelay(gasPrice, blockTime); // Tính thời điểm tối ưu
                const score = successProbability * 100 - cost - timingDelay;
                simulations.push({ gasPrice, successProbability, timingDelay, score });
            }

            simulations.sort((a, b) => b.score - a.score);
            const optimal = simulations[0];
            logger.info(`Optimal gas price (Game Theory) for chain ${chain}: ${optimal.gasPrice} Gwei, delay: ${optimal.timingDelay}ms`);
            return { gasPrice: optimal.gasPrice, timingDelay: optimal.timingDelay };
        } catch (error) {
            logger.error(`Error predicting gas price with Game Theory: ${error.message}`);
            return { gasPrice: 50, timingDelay: 0 };
        }
    }

    calculateSuccessProbability(gasPrice, competingGasPrices) {
        // Tính xác suất giao dịch được xác nhận trước các bot khác
        const higherCount = competingGasPrices.filter(price => price > gasPrice).length;
        return 1 - higherCount / competingGasPrices.length;
    }

    calculateTimingDelay(gasPrice, blockTime) {
        // Giả lập: Gửi giao dịch sớm hơn nếu gas price cao
        const maxDelay = blockTime * 0.5; // Tối đa delay 50% thời gian block
        const normalizedGasPrice = Math.min(Math.max(gasPrice / 100, 0), 1); // Chuẩn hóa gas price
        return maxDelay * (1 - normalizedGasPrice); // Gas price cao -> delay thấp
    }



    async optimizeGas(chain, baseGasPrice, mempoolData, bot) {
        try {
            const { gasPrice, timingDelay } = await this.predictOptimalGasPriceWithGameTheory(chain, bot);
            const optimalGasPrice = Math.max(baseGasPrice, gasPrice);
            return {
                maxFeePerGas: optimalGasPrice * 1e9,
                maxPriorityFeePerGas: (optimalGasPrice * 0.1) * 1e9,
                timingDelay,
            };
        } catch (error) {
            logger.error(`Error optimizing gas: ${error.message}`);
            return {
                maxFeePerGas: baseGasPrice * 1e9,
                maxPriorityFeePerGas: (baseGasPrice * 0.1) * 1e9,
                timingDelay: 0,
            };
        }
    }
}

module.exports = GasOptimizer;