const aedes = require('aedes');
const net = require('net');
const mqtt = require('mqtt');
const cluster = require('cluster');
// Khởi tạo Aedes broker
const broker = aedes();
const server = net.createServer(broker.handle);

// Chạy broker trên port 1883
server.listen(1883, () => {
    console.log('[Aedes] MQTT Broker running on port 1883');
});

// Hàm publish
const publishTrade = (topic, message) => {
    try {
        broker.publish({
            topic,
            payload: JSON.stringify(message),
            qos: 0,
            retain: false
        });
    } catch (error) {
        console.error('[MQTT Publish Error]', error.message);
    }
};

// Hàm kết nối client (cho bot sử dụng)
const mqttClient = mqtt.connect('mqtt://localhost:1883');
mqttClient.on('connect', () => {
    console.log('[MQTT Client] Connected to broker');
});

//Tạo cluster với aedes để chạy nhiều worker, tăng khả năng mở rộng.
if (cluster.isMaster) {
    for (let i = 0; i < numCPUs; i++) {
        cluster.fork();
    }
} else {
    const broker = aedes();
    const server = require('net').createServer(broker.handle);
    server.listen(1883 + cluster.worker.id, () => {
        console.log(`[Aedes Worker ${cluster.worker.id}] Running on port ${1883 + cluster.worker.id}`);
    });
}

module.exports = { publishTrade, mqttClient };