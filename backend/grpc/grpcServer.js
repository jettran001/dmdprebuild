const grpc = require('@grpc/grpc-js');
const protoLoader = require('@grpc/proto-loader');
const { triggerManualBuy, triggerSnipeMEV } = require('../controllers/snipebotController');
const { logger } = require('../utils/logger');

const packageDefinition = protoLoader.loadSync('proto/snipebot.proto', {
    keepCase: true,
    longs: String,
    enums: String,
    defaults: true,
    oneofs: true
});
const snipebotProto = grpc.loadPackageDefinition(packageDefinition).snipebot;

class GrpcServer {
    constructor() {
        this.server = new grpc.Server();
        this.server.addService(snipebotProto.SnipebotService.service, {
            sendLogForTraining: this.sendLogForTraining.bind(this),
        });
        this.server.bindAsync('0.0.0.0:50051', grpc.ServerCredentials.createInsecure(), () => {
            this.server.start();
            logger.info('gRPC server started on port 50051');
        });
    }

    sendLogForTraining(call, callback) {
        try {
            const { userId, logData } = call.request;
            logger.info(`Received log for training: ${userId}, ${logData}`);
            // Lưu log vào nơi tập trung (giả lập)
            console.log('Training AI with log:', { userId, logData });
            callback(null, { status: 'Log received for training' });
        } catch (error) {
            callback({ code: grpc.status.INTERNAL, message: error.message });
        }
    }
}

function startGrpcServer() {
    const server = new grpc.Server();

    server.addService(snipebotProto.SnipebotService.service, {
        
        TriggerManualBuy: async (call, callback) => {
            try {
                const { chain, tokenAddress, amount, poolId } = call.request;
                const result = await triggerManualBuy({ body: { chain, tokenAddress, amount, poolId } });
                callback(null, { status: result.status });
            } catch (error) {
                callback({
                    code: grpc.status.INTERNAL,
                    message: `Failed to trigger manual buy: ${error.message}`
                });
            }
        },
        
        TriggerSnipeMEV: async (call, callback) => {
            try {
                const { chain, tokenAddress } = call.request;
                const result = await triggerSnipeMEV({ body: { chain, tokenAddress } });
                callback(null, { status: result.status });
            } catch (error) {
                callback({
                    code: grpc.status.INTERNAL,
                    message: `Failed to trigger MEV snipe: ${error.message}`
                });
            }
        }
    });

    server.bindAsync('0.0.0.0:50051', grpc.ServerCredentials.createInsecure(), (err) => {
        if (err) {
            console.error('[gRPC] Failed to start server:', err.message);
            return;
        }
        server.start();
        console.log('[gRPC] Running on 0.0.0.0:50051');
    });

    return server;
}

module.exports = new GrpcServer();