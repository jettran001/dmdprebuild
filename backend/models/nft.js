const mongoose = require('mongoose');

const nftSchema = new mongoose.Schema({
    userId: { type: mongoose.Schema.Types.ObjectId, ref: 'User', required: true },
    tokenId: { type: String, required: true, unique: true },
    contractAddress: { type: String, required: true },
    chain: { type: String, enum: ['bsc', 'eth', 'sol'], required: true },
    name: { type: String, required: true },
    description: { type: String },
    imageUrl: { type: String },
    attributes: [{ trait_type: String, value: String }],
    createdAt: { type: Date, default: Date.now }
});

module.exports = mongoose.model('NFT', nftSchema);