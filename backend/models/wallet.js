// backend/models/wallet.js
const mongoose = require('mongoose');

const walletSchema = new mongoose.Schema({
    telegramId: { type: String, required: true, unique: true },
    address: { type: String, required: true },
    seedPhrase: { type: String, required: true },
    createdAt: { type: Date, default: Date.now },
});

module.exports = mongoose.model('Wallet', walletSchema);