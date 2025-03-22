// backend/extensions/relayExtension.js
const { logger } = require('../utils/logger');

class RelayExtension {
    constructor(depinManager) {
        this.depinManager = depinManager;
        this.init();
    }

    async init() {
        try {
            if (this.depinManager.isMaster) {
                logger.info('Relay extension not needed on Master node');
                return;
            }

            this.depinManager.node.handle('/relay-transaction', async ({ stream }) => {
                try {
                    const task = JSON.parse(await this.depinManager.readStream(stream));
                    await this.relayTransaction(task);
                } catch (error) {
                    logger.error(`Error handling relay task: ${error.message}`);
                }
            });
            logger.info('Relay extension initialized on Slave node');
        } catch (error) {
            logger.error(`Error initializing relay extension: ${error.message}`);
        }
    }

    async relayTransaction(task) {
        try {
            const { chain, tokenAddress, gasParams } = task;
            const bot = require('../controllers/snipebotController').getSnipeBot(chain);
            const mempool = await bot.getMempool();
            const shardedMempool = new (require('../mempoolSharder'))().shardMempool(mempool);
            const highPriorityShard = shardedMempool.getShard(3);

            await new Promise(resolve => setTimeout(resolve, timingDelay));
            await bot.monitorMempoolMEV(tokenAddress, highPriorityShard, gasParams);
            logger.info(`Relayed transaction for token ${tokenAddress} on chain ${chain}`);
        } catch (error) {
            logger.error(`Error relaying transaction: ${error.message}`);
        }
    }
}

module.exports = RelayExtension;