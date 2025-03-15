const Mission = require('../models/Mission');

module.exports = {
    getMissions: async (req, res) => {
        try {
            const missions = await Mission.getAll(req.pool, '0xTestWallet');
            res.json(missions);
        } catch (err) {
            res.status(500).json({ error: err.message });
        }
    },
    completeMission: async (req, res) => {
        try {
            const { missionId, wallet } = req.body;
            const reward = await Mission.complete(req.pool, req.dmdContract, missionId, wallet);
            res.json({ status: 'Mission completed, reward sent', reward });
        } catch (err) {
            res.status(400).json({ error: err.message });
        }
    },
    stake: async (req, res) => {
        try {
            const { wallet, amount } = req.body;
            const tx = await Mission.stake(req.farmingContract, wallet, amount);
            res.json({ status: 'Staked', tx });
        } catch (err) {
            res.status(400).json({ error: err.message });
        }
    }
};