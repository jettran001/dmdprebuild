const mongoose = require('mongoose');

const tokenSchema = new mongoose.Schema({
    address: { type: String, required: true, unique: true },
    name: { type: String, required: true },
    symbol: { type: String, required: true },
    decimals: { type: Number, default: 18 },
    totalSupply: { type: Number, required: true },
    auditScore: { type: Number, default: 0 },
    liquidity: { type: Number, default: 0 },
    tax: {
        buy: { type: Number, default: 0 },
        sell: { type: Number, default: 0 }
    },
    isVerified: { type: Boolean, default: false },
    createdAt: { type: Date, default: Date.now }
});

module.exports = mongoose.model('Token', tokenSchema);