// backend/controllers/farmManager.js
const { logger } = require('../utils/logger');
const { FarmPool } = require('../models/farmPool');
const { ethers } = require('ethers');

class FarmManager {
    async createPool(tokenA, tokenB, rewardRate) {
        try {
            const pool = new FarmPool({
                tokenA,
                tokenB,
                rewardRate,
                totalStaked: 0,
                stakes: [],
            });
            await pool.save();
            logger.info(`Created farming pool: ${tokenA}/${tokenB}`);
            return pool;
        } catch (error) {
            logger.error(`Error creating pool: ${error.message}`);
            throw error;
        }
    }

    async stake(telegramId, poolId, amount) {
        try {
            const pool = await FarmPool.findById(poolId);
            if (!pool) throw new Error('Pool not found');

            const wallet = await new (require('./walletManager'))().getWallet(telegramId);
            pool.stakes.push({ user: wallet.address, amount });
            pool.totalStaked += amount;
            await pool.save();
            logger.info(`Staked ${amount} in pool ${poolId} by ${wallet.address}`);
            return pool;
        } catch (error) {
            logger.error(`Error staking: ${error.message}`);
            throw error;
        }
    }

    async unstake(telegramId, poolId, amount) {
        try {
            const pool = await FarmPool.findById(poolId);
            if (!pool) throw new Error('Pool not found');

            const wallet = await new (require('./walletManager'))().getWallet(telegramId);
            const stake = pool.stakes.find(s => s.user === wallet.address);
            if (!stake || stake.amount < amount) throw new Error('Insufficient stake');

            stake.amount -= amount;
            pool.totalStaked -= amount;
            if (stake.amount === 0) {
                pool.stakes = pool.stakes.filter(s => s.user !== wallet.address);
            }
            await pool.save();
            logger.info(`Unstaked ${amount} from pool ${poolId} by ${wallet.address}`);
            return pool;
        } catch (error) {
            logger.error(`Error unstaking: ${error.message}`);
            throw error;
        }
    }

    async claimRewards(telegramId, poolId) {
        try {
            const pool = await FarmPool.findById(poolId);
            if (!pool) throw new Error('Pool not found');

            const wallet = await new (require('./walletManager'))().getWallet(telegramId);
            const stake = pool.stakes.find(s => s.user === wallet.address);
            if (!stake) throw new Error('No stake found');

            const rewards = stake.amount * pool.rewardRate * (Date.now() - stake.lastClaimed) / (1000 * 60 * 60 * 24); // Reward per day
            stake.lastClaimed = Date.now();
            await pool.save();
            logger.info(`Claimed ${rewards} rewards from pool ${poolId} by ${wallet.address}`);
            return rewards;
        } catch (error) {
            logger.error(`Error claiming rewards: ${error.message}`);
            throw error;
        }
    }
}

module.exports = FarmManager;