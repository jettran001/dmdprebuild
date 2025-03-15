const MiningWallet = require('../models/MiningWallet');

module.exports = {
    getMiningWallet: async (req, res) => {
        try {
            const { wallet } = req.body;
            const data = await MiningWallet.get(req.pool, wallet);
            res.json(data);
        } catch (err) {
            res.status(500).json({ error: err.message });
        }
    },
    withdrawMining: async (req, res) => {
        try {
            const { wallet, amount } = req.body;
            await MiningWallet.withdraw(req.pool, req.dmdContract, wallet, amount);
            res.json({ status: 'Withdrawn' });
        } catch (err) {
            res.status(400).json({ error: err.message });
        }
    }
};