const Mission = require('../models/mission');

const addMission = async (req, res) => {
    try {
        const { missionId, title, description, reward } = req.body;
        const mission = new Mission({ userId: req.user.userId, missionId, title, description, reward });
        await mission.save();
        res.status(201).json({ message: 'Mission added', missionId: mission._id });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

const getMissions = async (req, res) => {
    try {
        const missions = await Mission.find({ userId: req.user.userId });
        res.json(missions);
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

const completeMission = async (req, res) => {
    try {
        const { missionId } = req.params;
        const mission = await Mission.findOneAndUpdate(
            { missionId, userId: req.user.userId },
            { status: 'completed', completedAt: new Date() },
            { new: true }
        );
        if (!mission) return res.status(404).json({ error: 'Mission not found' });
        res.json({ message: 'Mission completed', mission });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

module.exports = { addMission, getMissions, completeMission };