const express = require('express');
const { Pool } = require('pg');
const { MongoClient } = require('mongodb');
const { Kafka } = require('kafkajs');
const WebSocket = require('ws');
const rateLimit = require('express-rate-limit');
const fastWeb3 = require('web3');
const helmet = require('helmet');
const cors = require('cors');
const redis = require('redis');
const routes = require('./routes');
const authMiddleware = require('./middleware/auth');
const csrfProtection = require('./middleware/csrf');
const errorHandler = require('./middleware/error');

const app = express();

app.use(helmet());
app.use(cors({ origin: 'http://localhost:3000', credentials: true }));
app.use(express.json());
app.use(rateLimit({ windowMs: 1000, max: 50 }));

const pool = new Pool({ user: process.env.DB_USER, host: process.env.DB_HOST, database: process.env.DB_NAME, password: process.env.DB_PASSWORD, port: 5432 });
const mongoClient = new MongoClient(process.env.MONGO_URL);
const kafka = new Kafka({ clientId: 'diamond', brokers: [process.env.KAFKA_BROKER] });
const producer = kafka.producer();
const consumer = kafka.consumer({ groupId: 'diamond-group' });
const wss = new WebSocket.Server({ port: 8080 });
const web3 = new fastWeb3('https://bsc-dataseed.binance.org/');
const dmdContract = new web3.eth.Contract(require('./smartcontracts/DMD.json').abi, process.env.DMD_ADDRESS);
const farmingContract = new web3.eth.Contract(require('./smartcontracts/Farming.json').abi, process.env.FARMING_ADDRESS);
const redisClient = redis.createClient();

(async () => {
    await mongoClient.connect();
    const db = mongoClient.db('diamond_db');
    await producer.connect();
    await consumer.connect();
    await consumer.subscribe({ topic: 'diamond-events' });
    await consumer.run({
        eachMessage: async ({ message }) => {
            const event = JSON.parse(message.value);
            await db.collection('usage_logs').insertOne({ event });
        }
    });
    await redisClient.connect();

    wss.on('connection', (ws) => {
        ws.on('message', async (msg) => {
            const data = JSON.parse(msg);
            if (data.type === 'price_update') ws.send(JSON.stringify({ price: await farmingContract.methods.getPrice().call() }));
        });
    });

    app.use((req, res, next) => {
        req.pool = pool;
        req.mongoDb = db;
        req.producer = producer;
        req.web3 = web3;
        req.dmdContract = dmdContract;
        req.farmingContract = farmingContract;
        req.redis = redisClient;
        next();
    });

    app.get('/api/csrf-token', csrfProtection, (req, res) => {
        res.send(req.csrfToken());
    });

    app.use('/api', authMiddleware, csrfProtection, routes);
    app.use(errorHandler);

    app.listen(process.env.PORT, () => console.log(`Server running on port ${process.env.PORT}`));
})();