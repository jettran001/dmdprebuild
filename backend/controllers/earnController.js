// backend/controllers/earnController.js
const Earn = require('../models/earn');
const { logger } = require('../utils/logger');
const axios = require('axios');

class EarnController {
    async analyzeToken(tokenPair) {
        try {
            const token = tokenPair.split('/')[1] || tokenPair;

            const dexToolsResponse = await axios.get(`https://api.dextools.io/v1/token?symbol=${token}`, {
                headers: { 'Authorization': 'Bearer YOUR_DEXTOOLS_API_KEY' }
            });
            const dexToolsData = dexToolsResponse.data;

            const dexScreenerResponse = await axios.get(`https://api.dexscreener.com/latest/dex/tokens/${token}`);
            const dexScreenerData = dexScreenerResponse.data;

            let warningLevel = '🟢';
            let hasDangerousFunction = false;
            let riskAssessment = 'Low risk, good potential';

            if (dexToolsData.security) {
                const risks = dexToolsData.security.risks || [];
                if (risks.includes('honeypot') || risks.includes('hidden_owner') || risks.includes('balance_modifiable')) {
                    hasDangerousFunction = true;
                    warningLevel = '🔴';
                    riskAssessment = 'High risk: Potential scam token';
                } else if (risks.length > 0) {
                    warningLevel = '🟡';
                    riskAssessment = 'Moderate risk: Proceed with caution';
                }
            }

            const liquidity = dexScreenerData.pairs?.[0]?.liquidity?.usd || 0;
            const volume24h = dexScreenerData.pairs?.[0]?.volume?.h24 || 0;
            const priceChange24h = dexScreenerData.pairs?.[0]?.priceChange?.h24 || 0;

            if (liquidity < 10000 || volume24h < 5000) {
                riskAssessment = `${riskAssessment}. Low liquidity/volume, high volatility risk`;
                if (warningLevel === '🟢') warningLevel = '🟡';
            } else if (priceChange24h > 50) {
                riskAssessment = `${riskAssessment}. High potential: Significant price increase`;
            }

            return {
                warningLevel,
                hasDangerousFunction,
                riskAssessment
            };
        } catch (error) {
            logger.error(`Error in analyzeToken: ${error.message}`);
            return {
                warningLevel: '🟡',
                hasDangerousFunction: false,
                riskAssessment: 'Unable to analyze: API error'
            };
        }
    }

    async signDaily(req) {
        try {
            const { userId } = req.body;
            if (!userId) throw new Error('Missing userId');

            let earn = await Earn.findOne({ userId });
            if (!earn) {
                earn = new Earn({ userId });
                await earn.save();
            }

            const now = new Date();
            const today = now.toDateString();
            const lastSignDate = earn.daily.lastSign ? new Date(earn.daily.lastSign).toDateString() : null;

            if (lastSignDate === today) {
                throw new Error('You have already signed today');
            }

            const yesterday = new Date(now);
            yesterday.setDate(yesterday.getDate() - 1);
            const yesterdayDate = yesterday.toDateString();

            if (lastSignDate === yesterdayDate) {
                earn.daily.streak = (earn.daily.streak || 0) + 1;
            } else {
                earn.daily.streak = 1; // Reset streak nếu không ký liên tục
            }

            const reward = earn.daily.streak >= 7 ? 0.1 : 0.05;
            earn.daily.lastSign = now;
            earn.daily.totalDMD = (earn.daily.totalDMD || 0) + reward;
            await earn.save();

            return { status: 'Sign successful', reward, streak: earn.daily.streak };
        } catch (error) {
            throw error;
        }
    }

    async getDailyStatus(req) {
        try {
            const { userId } = req.query;
            if (!userId) throw new Error('Missing userId');

            const earn = await Earn.findOne({ userId });
            if (!earn) return { streak: 0, totalDMD: 0, lastSign: null };

            return {
                streak: earn.daily.streak || 0,
                totalDMD: earn.daily.totalDMD || 0,
                lastSign: earn.daily.lastSign,
            };
        } catch (error) {
            logger.error(`Error in getDailyStatus: ${error.message}`);
            throw error;
        }
    }

    async claimMining(req) {
        try {
            const { userId } = req.body;
            if (!userId) throw new Error('Missing userId');

            let earn = await Earn.findOne({ userId });
            if (!earn) {
                earn = new Earn({ userId });
                await earn.save();
            }

            const now = new Date();
            const lastClaim = earn.mining.lastClaim ? new Date(earn.mining.lastClaim) : null;
            const claimIntervals = [1, 3, 6, 12, 24]; // Giờ
            const interval = claimIntervals[earn.mining.level - 1] * 60 * 60 * 1000;

            if (lastClaim && now - lastClaim < interval) {
                throw new Error(`You can claim again after ${claimIntervals[earn.mining.level - 1]} hours`);
            }

            const hoursSinceLastClaim = lastClaim ? (now - lastClaim) / (60 * 60 * 1000) : 0;
            const reward = earn.mining.speed * hoursSinceLastClaim;
            earn.mining.accumulatedDMD = (earn.mining.accumulatedDMD || 0) + reward;
            earn.mining.lastClaim = now;
            await earn.save();

            return { status: 'Claim successful', reward };
        } catch (error) {
            throw error;
        }
    }

    async upgradeMining(req) {
        try {
            const { userId } = req.body;
            if (!userId) throw new Error('Missing userId');

            let earn = await Earn.findOne({ userId });
            if (!earn) {
                earn = new Earn({ userId });
                await earn.save();
            }

            if (earn.mining.level >= 5) throw new Error('Max level reached');

            const upgradeCosts = [1, 2, 5, 10]; // DMD cần để nâng cấp
            const cost = upgradeCosts[earn.mining.level - 1];
            if (earn.mining.accumulatedDMD < cost) throw new Error('Not enough DMD to upgrade');

            earn.mining.accumulatedDMD -= cost;
            earn.mining.level += 1;
            earn.mining.speed = [0.01, 0.02, 0.03, 0.04, 0.05][earn.mining.level - 1];
            await earn.save();

            return { status: 'Upgrade successful', level: earn.mining.level, speed: earn.mining.speed };
        } catch (error) {
            logger.error(`Error in upgradeMining: ${error.message}`);
            throw error;
        }
    }

    async getMiningStatus(req) {
        try {
            const { userId } = req.query;
            if (!userId) throw new Error('Missing userId');

            const earn = await Earn.findOne({ userId });
            if (!earn) return { level: 1, speed: 0.01, accumulatedDMD: 0, lastClaim: null };

            return {
                level: earn.mining.level || 1,
                speed: earn.mining.speed || 0.01,
                accumulatedDMD: earn.mining.accumulatedDMD || 0,
                lastClaim: earn.mining.lastClaim,
            };
        } catch (error) {
            logger.error(`Error in getMiningStatus: ${error.message}`);
            throw error;
        }
    }

    async stakeFarm(req) {
        try {
            const { userId, poolName, amount, lockPeriod } = req.body;
            if (!userId || !poolName || !amount || !lockPeriod) throw new Error('Missing required fields');

            let earn = await Earn.findOne({ userId });
            if (!earn) {
                earn = new Earn({ userId });
                await earn.save();
            }

            const pool = earn.farm.find(p => p.poolName === poolName);
            if (pool) {
                const reward = await this.calculateFarmReward(userId, poolName);
                pool.accumulatedReward += reward;
                pool.amount += amount;
                pool.startTime = new Date();
            } else {
                earn.farm.push({ poolName, userId, amount, lockPeriod, startTime: new Date(), accumulatedReward: 0 });
            }

            await earn.save();
            return { status: 'Stake successful', poolName, amount };
        } catch (error) {
            logger.error(`Error in stakeFarm: ${error.message}`);
            throw error;
        }
    }

    async unstakeFarm(req) {
        try {
            const { userId, poolName } = req.body;
            if (!userId || !poolName) throw new Error('Missing required fields');

            let earn = await Earn.findOne({ userId });
            if (!earn) throw new Error('User not found');

            const poolIndex = earn.farm.findIndex(p => p.poolName === poolName);
            if (poolIndex === -1) throw new Error('Pool not found');

            const pool = earn.farm[poolIndex];
            const now = new Date();
            const lockEnd = new Date(pool.startTime.getTime() + pool.lockPeriod * 24 * 60 * 60 * 1000);
            let fee = 0;
            if (now < lockEnd) {
                fee = pool.amount * 0.02; // Phí 2%
            }

            const reward = await this.calculateFarmReward(userId, poolName);
            pool.accumulatedReward += reward;
            const totalReward = pool.accumulatedReward;
            earn.daily.totalDMD = (earn.daily.totalDMD || 0) + totalReward; // Phần thưởng là DMD

            earn.farm.splice(poolIndex, 1);
            await earn.save();

            return { status: 'Unstake successful', reward: totalReward, fee };
        } catch (error) {
            logger.error(`Error in unstakeFarm: ${error.message}`);
            throw error;
        }
    }

    async calculateFarmReward(userId, poolName) {
        try {
            const earn = await Earn.findOne({ userId });
            if (!earn) throw new Error('User not found');

            const pool = earn.farm.find(p => p.poolName === poolName);
            if (!pool) return 0;

            const now = new Date();
            const timeStaked = (now - new Date(pool.startTime)) / (24 * 60 * 60 * 1000); // Ngày
            const apr = {
                'DMD/BTC': 20,
                'DMD/ETH': 20,
                'DMD/BNB': 20,
                'DMD/SOL': 20,
                'DMD/BASE': 20,
                'DMD/ARB': 20,
                'DMD/XLM': 10,
                'DMD/NEAR': 10,
                'DMD/TON': 10,
                'DMD/SUI': 10,
                'DMD/AVAX': 10,
                'DMD/PI': 10,
            }[poolName] || 10;
            const multiplier = { 7: 1, 30: 1.2, 90: 1.5 }[pool.lockPeriod] || 1;
            const reward = pool.amount * (apr / 100) * timeStaked * multiplier / 365;
            return reward; // Phần thưởng là DMD
        } catch (error) {
            logger.error(`Error in calculateFarmReward: ${error.message}`);
            return 0;
        }
    }

    async getFarmStatus(req) {
        try {
            const { userId } = req.query;
            if (!userId) throw new Error('Missing userId');

            const earn = await Earn.findOne({ userId });
            if (!earn) return { farm: [] };

            for (const pool of earn.farm) {
                pool.accumulatedReward = await this.calculateFarmReward(userId, pool.poolName);
            }
            await earn.save();

            return { farm: earn.farm };
        } catch (error) {
            logger.error(`Error in getFarmStatus: ${error.message}`);
            throw error;
        }
    }
}

module.exports = new EarnController();