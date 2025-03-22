// backend/aiModule.js
const tf = require('@tensorflow/tfjs-node');
const { logger } = require('./utils/logger');
const DepinManager = require('./depin/depinManager');

class AIModule {
    constructor() {
        this.model = null;
        this.depinManager = new DepinManager();
        this.initModel();
    }

    async initModel() {
        try {
            this.model = tf.sequential();
            this.model.add(tf.layers.lstm({ units: 50, inputShape: [10, 1], returnSequences: true }));
            this.model.add(tf.layers.lstm({ units: 50 }));
            this.model.add(tf.layers.dense({ units: 1 }));
            this.model.compile({ optimizer: 'adam', loss: 'meanSquaredError' });
            logger.info('AI model initialized');
        } catch (error) {
            logger.error(`Error initializing AI model: ${error.message}`);
            throw error;
        }
    }

    async trainModel(marketData) {
        try {
            if (!this.depinManager.isMaster) {
                logger.warn('Only Master node can train AI model');
                return;
            }

            // Chuẩn bị dữ liệu (giả lập: giá token trong 10 bước thời gian)
            const xs = [];
            const ys = [];
            for (let i = 0; i < marketData.length - 10; i++) {
                const input = marketData.slice(i, i + 10).map(price => [price]);
                const output = marketData[i + 10];
                xs.push(input);
                ys.push(output);
            }

            const xsTensor = tf.tensor3d(xs);
            const ysTensor = tf.tensor1d(ys);

            // Huấn luyện mô hình
            await this.model.fit(xsTensor, ysTensor, {
                epochs: 10,
                batchSize: 32,
                verbose: 1,
            });

            logger.info('AI model trained successfully');
            tf.dispose([xsTensor, ysTensor]);
        } catch (error) {
            logger.error(`Error training AI model: ${error.message}`);
            throw error;
        }
    }

    async predictPrice(historicalData) {
        try {
            if (this.depinManager.isMaster) {
                // Master node tự suy luận
                const input = tf.tensor3d([historicalData.map(price => [price])]);
                const prediction = this.model.predict(input);
                const result = prediction.dataSync()[0];
                tf.dispose([input, prediction]);
                return result;
            } else {
                // Slave node gửi task suy luận cho Master
                const task = {
                    id: `predict-${Date.now()}`,
                    type: 'ai-predict',
                    data: historicalData,
                };
                await this.depinManager.assignTasks([task]);
                return await this.depinManager.getTaskResult(task.id);
            }
        } catch (error) {
            logger.error(`Error predicting price: ${error.message}`);
            throw error;
        }
    }

    async updateModelPeriodically() {
        try {
            if (!this.depinManager.isMaster) return;

            setInterval(async () => {
                const marketData = await this.fetchMarketData();
                await this.trainModel(marketData);
            }, 24 * 60 * 60 * 1000); // Cập nhật mỗi 24 giờ
        } catch (error) {
            logger.error(`Error updating AI model: ${error.message}`);
        }
    }

    async fetchMarketData() {
        // Giả lập lấy dữ liệu thị trường
        return Array.from({ length: 100 }, () => Math.random() * 100);
    }
}

module.exports = AIModule;