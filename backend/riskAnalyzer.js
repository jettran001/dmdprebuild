// backend/riskAnalyzer.js
const { logger } = require('./utils/logger');
const { logSnipe } = require('./models/snipebot');

class RiskAnalyzer {
    constructor() {
        this.historicalReturns = [];
        this.riskParameters = {
            maxLoss: 0.1, // Tối đa 10% lỗ
            confidenceLevel: 0.95, // Mức tin cậy 95%
        };
    }

    // Thu thập dữ liệu lịch sử 
    async fetchHistoricalReturns() {
        try {
            const logs = await logSnipe.find({ status: 'completed' }).sort({ createdAt: -1 }).limit(100);
            this.historicalReturns = logs.map(log => {
                const profit = (log.buyPrice > 0 ? (log.sellPrice - log.buyPrice) / log.buyPrice : 0);
                return profit;
            });
            if (this.historicalReturns.length === 0) {
                this.historicalReturns = Array.from({ length: 100 }, () => (Math.random() - 0.5) * 0.2);
            }
            return this.historicalReturns;
        } catch (error) {
            logger.error(`Error fetching historical returns: ${error.message}`);
            return Array.from({ length: 100 }, () => (Math.random() - 0.5) * 0.2);
        }
    }

    // Tính Value at Risk (VaR)
    async calculateVaR() {
        try {
            if (this.historicalReturns.length === 0) {
                await this.fetchHistoricalReturns();
            }

            const sortedReturns = [...this.historicalReturns].sort((a, b) => a - b);
            const index = Math.floor((1 - this.riskParameters.confidenceLevel) * sortedReturns.length);
            const varValue = sortedReturns[index];
            logger.info(`Calculated VaR: ${varValue} at ${this.riskParameters.confidenceLevel * 100}% confidence`);
            return varValue;
        } catch (error) {
            logger.error(`Error calculating VaR: ${error.message}`);
            return 0;
        }
    }

    // Điều chỉnh chiến lược dựa trên rủi ro
    async adjustStrategy() {
        try {
            const varValue = await this.calculateVaR();
            const adjustments = {};

            if (varValue < -this.riskParameters.maxLoss) {
                adjustments.maxSlippage = 0.05; // Giảm slippage tối đa
                adjustments.minProfit = 0.1; // Tăng lợi nhuận tối thiểu
                logger.info('Risk too high, tightening strategy parameters');
            } else {
                adjustments.maxSlippage = 0.1; // Nới lỏng slippage
                adjustments.minProfit = 0.05; // Giảm lợi nhuận tối thiểu
                logger.info('Risk acceptable, loosening strategy parameters');
            }

            return adjustments;
        } catch (error) {
            logger.error(`Error adjusting strategy: ${error.message}`);
            return { maxSlippage: 0.1, minProfit: 0.05 };
        }
    }

    updateRiskParameters(params) {
        this.riskParameters = { ...this.riskParameters, ...params };
        logger.info(`Updated risk parameters: ${JSON.stringify(this.riskParameters)}`);
    }
}

module.exports = RiskAnalyzer;