const mongoose = require('mongoose');

const missionSchema = new mongoose.Schema({
    userId: { type: mongoose.Schema.Types.ObjectId, ref: 'User', required: true },
    missionId: { type: String, required: true, unique: true },
    title: { type: String, required: true },
    description: { type: String },
    reward: { type: Number, required: true },
    status: { type: String, enum: ['pending', 'completed', 'claimed'], default: 'pending' },
    createdAt: { type: Date, default: Date.now },
    completedAt: { type: Date }
});

module.exports = mongoose.model('Mission', missionSchema);