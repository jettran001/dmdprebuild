// backend/mempoolSharder.js
const { logger } = require('./utils/logger');

class MempoolSharder {
    constructor() {
        this.shards = new Map();
        this.shardCount = 5; // Số shard (có thể điều chỉnh)
    }

    // Chia mempool thành các shard dựa trên giá gas
    shardMempool(transactions) {
        try {
            this.shards.clear();
            for (let i = 0; i < this.shardCount; i++) {
                this.shards.set(i, []);
            }

            transactions.forEach(tx => {
                const gasPrice = parseInt(tx.gasPrice || tx.maxFeePerGas || '0', 16);
                const shardIndex = this.getShardIndex(gasPrice);
                this.shards.get(shardIndex).push(tx);
            });

            logger.info(`Mempool sharded into ${this.shardCount} shards`);
            return this.shards;
        } catch (error) {
            logger.error(`Error sharding mempool: ${error.message}`);
            return new Map();
        }
    }

    getShardIndex(gasPrice) {
        // Chia shard dựa trên giá gas (có thể tùy chỉnh logic)
        const normalized = Math.min(Math.max(gasPrice / 1e9, 0), 100); // Chuẩn hóa gas price (Gwei)
        return Math.floor(normalized / (100 / this.shardCount)) % this.shardCount;
    }

    getShard(shardIndex) {
        return this.shards.get(shardIndex) || [];
    }
}

module.exports = MempoolSharder;