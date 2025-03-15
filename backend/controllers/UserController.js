const User = require('../models/User');

module.exports = {
    getUsers: async (req, res) => {
        try {
            const users = await User.getAll(req.pool);
            res.json(users);
        } catch (err) {
            res.status(500).json({ error: err.message });
        }
    },
    getBalance: async (req, res) => {
        try {
            const { wallet } = req.body;
            const balance = await User.getBalance(req.dmdContract, wallet);
            res.json({ balance });
        } catch (err) {
            res.status(500).json({ error: err.message });
        }
    }
};