const Product = require('../models/Product');

module.exports = {
    getProducts: async (req, res) => {
        try {
            const cached = await req.redis.get('products');
            if (cached) return res.json(JSON.parse(cached));
            const products = await Product.getAll(req.pool);
            await req.redis.setEx('products', 60, JSON.stringify(products));
            res.json(products);
        } catch (err) {
            res.status(500).json({ error: err.message });
        }
    },
    buyProduct: async (req, res) => {
        try {
            const { itemId, wallet } = req.body;
            const product = await Product.buy(req.pool, req.producer, itemId, wallet);
            res.json({ status: 'Bought', product });
        } catch (err) {
            res.status(400).json({ error: err.message });
        }
    },
    getBandwidthStorage: async (req, res) => {
        try {
            const products = await Product.getByType(req.pool, ['bandwidth', 'storage']);
            res.json(products);
        } catch (err) {
            res.status(500).json({ error: err.message });
        }
    }
};