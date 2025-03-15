const Proxy = require('../models/Proxy');

module.exports = {
    getProxies: async (req, res) => {
        try {
            const proxies = await Proxy.getAll(req.pool);
            res.json(proxies);
        } catch (err) {
            res.status(500).json({ error: err.message });
        }
    }
};