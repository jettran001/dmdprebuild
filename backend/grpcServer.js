const grpc = require('@grpc/grpc-js');
const protoLoader = require('@grpc/proto-loader');
const { triggerManualBuy } = require('./controllers/snipebotController');

const packageDefinition = protoLoader.loadSync('proto/snipebot.proto', {});
const snipebotProto = grpc.loadPackageDefinition(packageDefinition).snipebot;

const server = new grpc.Server();
server.addService(snipebotProto.SnipebotService.service, {
    TriggerManualBuy: (call, callback) => {
        triggerManualBuy({ params: { chain: call.request.chain }, body: call.request }, {
            json: (data) => callback(null, data)
        });
    }
});

server.bindAsync('0.0.0.0:50051', grpc.ServerCredentials.createInsecure(), () => {
    server.start();
    console.log('[gRPC] Server running on port 50051');
});