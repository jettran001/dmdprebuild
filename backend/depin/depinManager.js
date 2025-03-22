// backend/depin/depinManager.js
const { createLibp2pNode } = require('./libp2pNode');
const HybridCDN = require('./hybridCDN');
const DepinEdgeCache = require('./edgeCache');
const IPFSNode = require('./ipfsNode');
const { logger } = require('../utils/logger');
const RelayExtension = require('../extensions/relayExtension');



class DepinManager {
    constructor() {
        this.nodes = {};
        this.cache = new DepinEdgeCache();
        this.cdn = new HybridCDN('https://cdn.example.com');
        this.ipfs = new IPFSNode();
        this.node = null;
        this.isMaster = false;
        this.slaves = [];
        this.resourceLimits = {
            cpu: 50,
            ram: 50,
            gpu: 70,
            bandwidth: 80,
        };
        this.taskResults = new Map();
        this.relayExtension = null;
        this.init();
    }

    async init() {
        try {
            await this.cdn.init();
            await this.ipfs.init();
            await this.startNode();
            await this.electMasterSlave();
            this.relayExtension = new RelayExtension(this);
            logger.info('[DePIN Manager] Initialized');
        } catch (error) {
            logger.error(`[DePIN Manager] Initialization failed: ${error.message}`);
            throw error;
        }
    }

    async startNode() {
        try {
            this.node = await createLibp2pNode();
            this.nodes[this.node.peerId.toB58String()] = {
                node: this.node,
                type: this.getDeviceType(),
                resources: this.getHWID(),
            };
            logger.info(`[DePIN] Node ${this.getDeviceType()} initialized with ID: ${this.node.peerId.toB58String()}`);

            this.node.on('peer:discovery', (peerId) => {
                logger.info(`Discovered peer: ${peerId.toB58String()}`);
                this.measureLatency(peerId).catch(err => logger.error(`Error measuring latency: ${err.message}`));
            });

            this.node.handle('/task-result', async ({ stream }) => {
                try {
                    const result = await this.readStream(stream);
                    const { taskId, data } = JSON.parse(result);
                    this.taskResults.set(taskId, data);
                    logger.info(`Received task result for task ${taskId}: ${JSON.stringify(data)}`);
                } catch (error) {
                    logger.error(`Error handling task result: ${error.message}`);
                }
            });

            this.node.on('peer:disconnect', (peerId) => {
                this.slaves = this.slaves.filter(slave => slave.id !== peerId.toB58String());
                logger.info(`Slave disconnected: ${peerId.toB58String()}`);
            });

            // Slave node xử lý task
            if (!this.isMaster) {
                this.node.handle('/task', async ({ stream }) => {
                    try {
                        const task = JSON.parse(await this.readStream(stream));
                        const result = await this.executeTask(task);
                        const { stream: resultStream } = await this.node.dialProtocol(task.masterId, '/task-result');
                        await resultStream.write(JSON.stringify({ taskId: task.id, data: result }));
                        await resultStream.close();
                    } catch (error) {
                        logger.error(`Error executing task on Slave: ${error.message}`);
                    }
                });
            }
        } catch (error) {
            logger.error(`[DePIN] Failed to start node: ${error.message}`);
            throw error;
        }
    }

    async executeTask(task) {
        // Giả lập tính toán GPU (ví dụ: suy luận AI)
        logger.info(`Executing task ${task.id} on Slave node with GPU`);
        return { taskId: task.id, result: `Processed by GPU: ${Math.random()}` };
    }

    async measureLatency(peerId) {
        try {
            const start = Date.now();
            await this.node.ping(peerId);
            const latency = Date.now() - start;
            return { peerId: peerId.toB58String(), latency };
        } catch (error) {
            logger.error(`Error measuring latency for peer ${peerId.toB58String()}: ${error.message}`);
            return { peerId: peerId.toB58String(), latency: Infinity };
        }
    }

    async electMasterSlave() {
        try {
            const peers = await Promise.all(this.node.peerStore.peers.map(peer => this.measureLatency(peer)));
            const hwid = this.getHWID();
            const deviceInfo = {
                id: this.node.peerId.toB58String(),
                hwid,
                type: this.getDeviceType(),
                latency: peers,
            };

            await this.broadcastDeviceInfo(deviceInfo);
            const allDevices = await this.collectDeviceInfo();

            const nearbyDevices = allDevices.filter(d => d.latency.some(l => l.latency < 100));
            if (nearbyDevices.length === 0) {
                this.isMaster = true;
                this.slaves = [];
                logger.info('No nearby devices found. This node is Master.');
                return;
            }

            const strongestDevice = nearbyDevices.sort((a, b) => this.compareDeviceStrength(b.hwid, a.hwid))[0];
            if (strongestDevice.id === deviceInfo.id) {
                this.isMaster = true;
                this.slaves = nearbyDevices.filter(d => d.id !== deviceInfo.id);
                logger.info(`This node is Master. Slaves: ${this.slaves.map(s => s.id)}`);
            } else {
                this.isMaster = false;
                logger.info(`This node is Slave. Master: ${strongestDevice.id}`);
            }

            if (this.isMaster) {
                this.groupWeakDevices(nearbyDevices);
            }
        } catch (error) {
            logger.error(`Error electing Master-Slave: ${error.message}`);
            throw error;
        }
    }

    getHWID() {
        return { cpu: 50, ram: 16, gpu: 8 };
    }

    getDeviceType() {
        return process.env.DEVICE_TYPE || 'pc';
    }

    compareDeviceStrength(hwidA, hwidB) {
        return (hwidA.cpu + hwidA.ram + hwidA.gpu) - (hwidB.cpu + hwidB.ram + hwidB.gpu);
    }

    async broadcastDeviceInfo(deviceInfo) {
        try {
            for (const peer of this.node.peerStore.peers) {
                const { stream } = await this.node.dialProtocol(peer, '/device-info');
                await stream.write(JSON.stringify(deviceInfo));
                await stream.close();
            }
        } catch (error) {
            logger.error(`Error broadcasting device info: ${error.message}`);
        }
    }

    async collectDeviceInfo() {
        const devices = [];
        this.node.handle('/device-info', async ({ stream }) => {
            try {
                const data = await this.readStream(stream);
                devices.push(JSON.parse(data));
            } catch (error) {
                logger.error(`Error collecting device info: ${error.message}`);
            }
        });
        await new Promise(resolve => setTimeout(resolve, 5000));
        return devices;
    }

    groupWeakDevices(devices) {
        const weakDevices = devices.filter(d => this.compareDeviceStrength(d.hwid, { cpu: 30, ram: 8, gpu: 4 }) < 0);
        const groups = this.clusterDevices(weakDevices);
        logger.info(`Grouped weak devices: ${JSON.stringify(groups)}`);
        return groups;
    }

    clusterDevices(devices) {
        return [devices];
    }

    async assignTasks(tasks) {
        if (!this.isMaster) return;

        try {
            const groups = this.groupWeakDevices(this.slaves);
            for (const group of groups) {
                const gpuCapableSlaves = group.filter(slave => slave.hwid.gpu > 0);
                if (gpuCapableSlaves.length === 0) {
                    logger.warn(`No GPU-capable slaves in group: ${group.map(s => s.id)}`);
                    continue;
                }

                for (const task of tasks) {
                    const resourceUsage = this.calculateResourceUsage(task);
                    if (this.canAssignTask(resourceUsage)) {
                        task.masterId = this.node.peerId.toB58String();
                        await this.runTaskOnGroup(task, gpuCapableSlaves);
                    } else {
                        logger.warn(`Task ${task.id} exceeds resource limits: ${JSON.stringify(resourceUsage)}`);
                    }
                }
            }
        } catch (error) {
            logger.error(`Error assigning tasks: ${error.message}`);
        }
    }

    calculateResourceUsage(task) {
        return { cpu: 10, ram: 10, gpu: 20, bandwidth: 30 };
    }

    canAssignTask(resourceUsage) {
        return (
            resourceUsage.cpu <= this.resourceLimits.cpu &&
            resourceUsage.ram <= this.resourceLimits.ram &&
            resourceUsage.gpu <= this.resourceLimits.gpu &&
            resourceUsage.bandwidth <= this.resourceLimits.bandwidth
        );
    }

    async runTaskOnGroup(task, group) {
        try {
            logger.info(`Running task ${task.id} on group: ${group.map(d => d.id)}`);
            for (const slave of group) {
                try {
                    const { stream } = await this.node.dialProtocol(slave.id, '/task');
                    await stream.write(JSON.stringify(task));
                    await stream.close();
                } catch (error) {
                    logger.error(`Error sending task ${task.id} to slave ${slave.id}: ${error.message}`);
                }
            }
        } catch (error) {
            logger.error(`Error running task ${task.id}: ${error.message}`);
        }
    }

    async readStream(stream) {
        try {
            const chunks = [];
            for await (const chunk of stream) {
                chunks.push(chunk);
            }
            return Buffer.concat(chunks).toString();
        } catch (error) {
            logger.error(`Error reading stream: ${error.message}`);
            throw error;
        }
    }

    async getTaskResult(taskId, timeout = 30000) {
        try {
            const start = Date.now();
            while (Date.now() - start < timeout) {
                if (this.taskResults.has(taskId)) {
                    const result = this.taskResults.get(taskId);
                    this.taskResults.delete(taskId);
                    return result;
                }
                await new Promise(resolve => setTimeout(resolve, 1000));
            }
            throw new Error(`Timeout waiting for task result: ${taskId}`);
        } catch (error) {
            logger.error(`Error getting task result for ${taskId}: ${error.message}`);
            throw error;
        }
    }

    async initNode(nodeType, resources) {
        try {
            const node = await createLibp2pNode();
            this.nodes[node.peerId.toB58String()] = { node, type: nodeType, resources };
            logger.info(`[DePIN] Node ${nodeType} initialized`);
            return node;
        } catch (error) {
            logger.error(`Error initializing node ${nodeType}: ${error.message}`);
            throw error;
        }
    }

    async storeData(nodeId, data) {
        try {
            const cid = await this.ipfs.addData(data);
            await this.cacheData(nodeId, cid, data);
            return cid;
        } catch (error) {
            logger.error(`Error storing data for node ${nodeId}: ${error.message}`);
            throw error;
        }
    }

    async fetchData(nodeId, cid) {
        try {
            const cached = await this.getCachedData(nodeId, cid);
            if (cached) return cached;

            const data = await this.ipfs.getData(cid);
            await this.cacheData(nodeId, cid, data);
            return data;
        } catch (error) {
            logger.error(`Error fetching data for node ${nodeId}, CID ${cid}: ${error.message}`);
            throw error;
        }
    }

    async cacheData(nodeId, key, value) {
        try {
            this.cache.set(`${nodeId}:${key}`, value);
        } catch (error) {
            logger.error(`Error caching data for node ${nodeId}: ${error.message}`);
        }
    }

    async getCachedData(nodeId, key) {
        try {
            return this.cache.get(`${nodeId}:${key}`);
        } catch (error) {
            logger.error(`Error getting cached data for node ${nodeId}: ${error.message}`);
            return null;
        }
    }

    async fetchResource(nodeId, resourceId) {
        try {
            const cached = await this.getCachedData(nodeId, resourceId);
            if (cached) return cached;

            const data = await this.cdn.fetchData(resourceId);
            await this.cacheData(nodeId, resourceId, data);
            return data;
        } catch (error) {
            logger.error(`Error fetching resource for node ${nodeId}, resource ${resourceId}: ${error.message}`);
            throw error;
        }
    }
}

module.exports = DepinManager;