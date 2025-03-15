const Node = require('../models/Node');

module.exports = {
    getNodes: async (req, res) => {
        try {
            const nodes = await Node.getAll(req.pool);
            res.json(nodes);
        } catch (err) {
            res.status(500).json({ error: err.message });
        }
    },
    delistNode: async (req, res) => {
        try {
            const { hwid } = req.body;
            await Node.delist(req.pool, hwid);
            res.json({ status: 'Node delisted' });
        } catch (err) {
            res.status(400).json({ error: err.message });
        }
    },
    getNodeKeys: async (req, res) => {
        try {
            const nodes = await Node.getAll(req.pool);
            res.json(nodes.map(n => ({ hwid: n.hwid, key: 'key_' + n.hwid })));
        } catch (err) {
            res.status(500).json({ error: err.message });
        }
    },
    generateKey: async (req, res) => {
        try {
            const { hwid } = req.body;
            const key = await Node.generateKey(req.pool, hwid);
            res.json({ status: 'Key generated', key });
        } catch (err) {
            res.status(400).json({ error: err.message });
        }
    },
    deleteKey: async (req, res) => {
        try {
            const { hwid } = req.body;
            await Node.deleteKey(req.pool, hwid);
            res.json({ status: 'Key deleted' });
        } catch (err) {
            res.status(400).json({ error: err.message });
        }
    }
};