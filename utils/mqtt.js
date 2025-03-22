const mqtt = require('mqtt');
const client = mqtt.connect('mqtt://localhost:1883');

client.on('connect', () => {
    console.log('[MQTT] Connected to Mosquitto');
});

const publishTrade = (topic, message) => {
    client.publish(topic, JSON.stringify(message));
};

module.exports = { publishTrade };