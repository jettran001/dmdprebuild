const mongoose = require('mongoose');

const transactionSchema = new mongoose.Schema({
    userId: { type: mongoose.Schema.Types.ObjectId, ref: 'User', required: true },
    txHash: { type: String, required: true, unique: true },
    tokenAddress: { type: String, required: true },
    chain: { type: String, enum: ['bsc', 'eth', 'sol'], required: true },
    type: { type: String, enum: ['buy', 'sell', 'transfer'], required: true },
    amount: { type: Number, required: true },
    price: { type: Number, required: true },
    status: { type: String, enum: ['pending', 'confirmed', 'failed'], default: 'pending' },
    timestamp: { type: Date, default: Date.now }
});

module.exports = mongoose.model('Transaction', transactionSchema);