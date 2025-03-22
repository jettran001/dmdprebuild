const mongoose = require('mongoose');

const exchangeSchema = new mongoose.Schema({
    userId: { type: mongoose.Schema.Types.ObjectId, ref: 'User', required: true },
    tokenIn: { type: String, required: true },
    tokenOut: { type: String, required: true },
    amountIn: { type: Number, required: true },
    amountOut: { type: Number, required: true },
    dex: { type: String, enum: ['pancakeswap', 'uniswap', 'raydium'], required: true },
    chain: { type: String, enum: ['bsc', 'eth', 'sol'], required: true },
    txHash: { type: String, required: true, unique: true },
    status: { type: String, enum: ['pending', 'completed', 'failed'], default: 'pending' },
    timestamp: { type: Date, default: Date.now }
});

module.exports = mongoose.model('Exchange', exchangeSchema);