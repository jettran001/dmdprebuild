// backend/server.js
const express = require('express');
const session = require('express-session');
const RedisStore = require('connect-redis')(session);
const { client } = require('./redis');
const { Server } = require('socket.io');
const http = require('http');
const mongoose = require('mongoose');
const uWS = require('uWebSockets.js');
const libp2p = require('libp2p');
const { noise } = require('@chainsafe/libp2p-noise');
const { mplex } = require('@libp2p/mplex');
const { tcp } = require('@libp2p/tcp');
const { startSFU } = require('./sfu/sfu');
const { startGrpcServer } = require('./grpc/grpcServer');
const { getSnipeBot } = require('./controllers/snipebotController');
const DepinManager = require('./depin/depinManager');
const snipebotRoutes = require('./routes/snipebot');
const apiRoutes = require('./routes/api');
const auth = require('./middleware/auth');
const errorHandler = require('./middleware/errorHandler');
const loggerMiddleware = require('./middleware/logger');
const { logger } = require('./utils/logger');
require('dotenv').config();
const ipfsStorage = require('./ipfs');

const app = express();
const server = http.createServer(app);
let io, sfuRouter;
const depinManager = new DepinManager();

// Cấu hình session với Redis
app.use(session({
    store: new RedisStore({ client }),
    secret: 'YOUR_SESSION_SECRET',
    resave: false,
    saveUninitialized: false,
    cookie: { secure: false, maxAge: 24 * 60 * 60 * 1000 } // 1 ngày
}));

let node; // Libp2p node
let isMaster = false;
let slaves = [];
let masterPeerId = null;

// Kết nối MongoDB
mongoose.connect(process.env.MONGO_URI, { useNewUrlParser: true, useUnifiedTopology: true })
    .then(() => logger.info('Connected to MongoDB'))
    .catch(err => logger.error('MongoDB connection error:', err));

// Middleware và Routes
app.use(express.json());
app.use(loggerMiddleware);
app.use('/api/snipebot', snipebotRoutes);
app.use('/api', apiRoutes);
app.use('/api', require('./routes/wallet'));
app.use(auth);
app.use(errorHandler);
// Khởi động server
startServer();
ipfsStorage.init();

async function startServer() {
    // WebSocket
    io = initSocket(server);

    // SFU (WebRTC)
    sfuRouter = await startSFU();

    // gRPC
    startGrpcServer();

    // QUIC
    uWS.App().get('/quic', (res) => {
        res.onAborted(() => {
            logger.error('[QUIC] Request aborted');
        });
        try {
            res.end('QUIC enabled');
        } catch (error) {
            logger.error('[QUIC] Error handling request:', error.message);
            res.writeStatus('500 Internal Server Error').end('Error');
        }
    }).listen(3001, (token) => {
        if (token) {
            logger.info('[QUIC] Running on 3001');
        } else {
            logger.error('[QUIC] Failed to start on port 3001');
        }
    });

    // Khởi động Libp2p node
    await startNode();
    await electMasterSlave();

    // Khởi động HTTP server
    server.listen(process.env.PORT || 3000, () => {
        logger.info(`Server running on port ${process.env.PORT || 3000}`);
    });
}

function initSocket(server) {
    const io = new Server(server);
    io.on('connection', (socket) => {
        logger.info('A user connected');
        socket.on('signal', (data) => socket.broadcast.emit('signal', data));
        socket.on('disconnect', () => logger.info('User disconnected'));

        const sendBotStatus = () => {
            const bot = getSnipeBot('ton');
            io.emit('botStatus', {
                chain: 'ton',
                status: bot?.isMonitoring ? 'Running' : 'Stopped',
            });
        };
        sendBotStatus();
        setInterval(sendBotStatus, 5000);
    });
    return io;
}

async function startNode() {
    node = await libp2p.create({
        addresses: { listen: ['/ip4/0.0.0.0/tcp/0'] },
        modules: {
            transport: [tcp()],
            streamMuxer: [mplex()],
            connEncryption: [noise()],
        },
    });

    await node.start();
    logger.info(`Libp2p node started with ID: ${node.peerId.toB58String()}`);

    // Theo dõi các node khác
    node.on('peer:discovery', (peerId) => {
        logger.info(`Discovered peer: ${peerId.toB58String()}`);
        measureLatency(peerId);
    });

    // Xử lý dữ liệu từ Slave node
    node.handle('/slave-data', async ({ stream }) => {
        const data = await readStream(stream);
        logger.info(`Received data from Slave: ${data}`);
        io.emit('slaveData', data); // Gửi dữ liệu qua WebSocket nếu cần
    });
}

async function measureLatency(peerId) {
    const start = Date.now();
    await node.ping(peerId);
    const latency = Date.now() - start;
    return { peerId: peerId.toB58String(), latency };
}

async function electMasterSlave() {
    const peers = await Promise.all(node.peerStore.peers.map(peer => measureLatency(peer)));
    const hwid = getHWID();
    const deviceInfo = { id: node.peerId.toB58String(), hwid, type: getDeviceType(), latency: peers };

    // Gửi thông tin thiết bị đến các node khác
    await broadcastDeviceInfo(deviceInfo);

    // Thu thập thông tin từ các node
    const allDevices = await collectDeviceInfo();

    // Quyết định Master-Slave
    const nearbyDevices = allDevices.filter(d => d.latency.some(l => l.latency < 100));
    const strongestDevice = nearbyDevices.sort((a, b) => compareDeviceStrength(b.hwid, a.hwid))[0];

    if (strongestDevice.id === deviceInfo.id) {
        isMaster = true;
        slaves = nearbyDevices.filter(d => d.id !== deviceInfo.id);
        logger.info(`This node is Master. Slaves: ${slaves.map(s => s.id)}`);
    } else {
        isMaster = false;
        masterPeerId = strongestDevice.id;
        logger.info(`This node is Slave. Master: ${masterPeerId}`);
        // Slave gửi dữ liệu về Master
        await sendDataToMaster({ id: deviceInfo.id, status: 'online' });
    }
}

function getHWID() {
    return { cpu: 50, ram: 16, gpu: 8 };
}

function getDeviceType() {
    return process.env.DEVICE_TYPE || 'pc';
}

function compareDeviceStrength(hwidA, hwidB) {
    return (hwidA.cpu + hwidA.ram + hwidA.gpu) - (hwidB.cpu + hwidB.ram + hwidB.gpu);
}

async function broadcastDeviceInfo(deviceInfo) {
    for (const peer of node.peerStore.peers) {
        const { stream } = await node.dialProtocol(peer, '/device-info');
        await stream.write(JSON.stringify(deviceInfo));
        await stream.close();
    }
}

async function collectDeviceInfo() {
    const devices = [];
    node.handle('/device-info', async ({ stream }) => {
        const data = await readStream(stream);
        devices.push(JSON.parse(data));
    });
    await new Promise(resolve => setTimeout(resolve, 5000));
    return devices;
}

async function sendDataToMaster(data) {
    if (!masterPeerId) return;
    const { stream } = await node.dialProtocol(masterPeerId, '/slave-data');
    await stream.write(JSON.stringify(data));
    await stream.close();
}

async function readStream(stream) {
    const chunks = [];
    for await (const chunk of stream) {
        chunks.push(chunk);
    }
    return Buffer.concat(chunks).toString();
}

module.exports = { io };