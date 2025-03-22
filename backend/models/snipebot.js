const mongoose = require('mongoose');

const snipebotSchema = new mongoose.Schema({
    userId: { type: mongoose.Schema.Types.ObjectId, ref: 'User', required: true },
    tokenAddress: { type: String, required: true },
    chain: { type: String, enum: ['bsc', 'eth', 'sol'], required: true },
    mode: { type: String, enum: ['mev', 'smart'], required: true },
    buyAmount: { type: Number, required: true },
    buyPrice: { type: Number, required: true },
    sellPrice: { type: Number },
    profit: { type: Number, default: 0 },
    status: { type: String, enum: ['pending', 'completed', 'failed'], default: 'pending' },
    timestamp: { type: Date, default: Date.now }
});

const SnipeBot = mongoose.model('SnipeBot', snipebotSchema);

const logSnipe = async (data) => {
    const snipe = new SnipeBot(data);
    await snipe.save();
    return snipe;
};

module.exports = { SnipeBot, logSnipe };