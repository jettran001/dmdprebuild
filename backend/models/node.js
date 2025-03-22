const mongoose = require('mongoose');

const nodeSchema = new mongoose.Schema({
    userId: { type: mongoose.Schema.Types.ObjectId, ref: 'User', required: true },
    nodeId: { type: String, required: true, unique: true },
    type: { type: String, enum: ['mining', 'staking'], required: true },
    status: { type: String, enum: ['active', 'inactive'], default: 'active' },
    rewardRate: { type: Number, required: true },
    totalReward: { type: Number, default: 0 },
    createdAt: { type: Date, default: Date.now }
});

module.exports = mongoose.model('Node', nodeSchema);