// backend/models/earn.js
const mongoose = require('mongoose');

const farmPoolSchema = new mongoose.Schema({
    poolName: { type: String, required: true }, // e.g., "DMD/BNB"
    userId: { type: String, required: true },
    amount: { type: Number, default: 0 },
    lockPeriod: { type: Number, required: true }, // 7, 30, 90 days
    startTime: { type: Date, default: Date.now },
    accumulatedReward: { type: Number, default: 0 },
});

const earnSchema = new mongoose.Schema({
    userId: { type: String, required: true },
    daily: {
        lastSign: { type: Date },
        streak: { type: Number, default: 0 },
        totalDMD: { type: Number, default: 0 },
    },
    mining: {
        level: { type: Number, default: 1 },
        speed: { type: Number, default: 0.01 },
        lastClaim: { type: Date },
        accumulatedDMD: { type: Number, default: 0 },
    },
    farm: [farmPoolSchema], // Thêm farm pools
    createdAt: { type: Date, default: Date.now },
});

module.exports = mongoose.model('Earn', earnSchema);