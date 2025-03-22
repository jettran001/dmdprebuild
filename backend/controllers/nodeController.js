const Node = require('../models/node');

const addNode = async (req, res) => {
    try {
        const { nodeId, type, rewardRate } = req.body;
        const node = new Node({ userId: req.user.userId, nodeId, type, rewardRate });
        await node.save();
        res.status(201).json({ message: 'Node added', nodeId: node._id });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

const getNodes = async (req, res) => {
    try {
        const nodes = await Node.find({ userId: req.user.userId });
        res.json(nodes);
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

const updateNodeStatus = async (req, res) => {
    try {
        const { nodeId } = req.params;
        const { status, totalReward } = req.body;
        const node = await Node.findOneAndUpdate(
            { nodeId, userId: req.user.userId },
            { status, totalReward },
            { new: true }
        );
        if (!node) return res.status(404).json({ error: 'Node not found' });
        res.json({ message: 'Node updated', node });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

module.exports = { addNode, getNodes, updateNodeStatus };