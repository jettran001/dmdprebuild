const Exchange = require('../models/exchange');

const logExchange = async (req, res) => {
    try {
        const { tokenIn, tokenOut, amountIn, amountOut, dex, chain, txHash } = req.body;
        const exchange = new Exchange({
            userId: req.user.userId,
            tokenIn,
            tokenOut,
            amountIn,
            amountOut,
            dex,
            chain,
            txHash
        });
        await exchange.save();
        res.status(201).json({ message: 'Exchange logged', exchangeId: exchange._id });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

const getExchanges = async (req, res) => {
    try {
        const exchanges = await Exchange.find({ userId: req.user.userId });
        res.json(exchanges);
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

const updateExchangeStatus = async (req, res) => {
    try {
        const { txHash } = req.params;
        const { status } = req.body;
        const exchange = await Exchange.findOneAndUpdate(
            { txHash, userId: req.user.userId },
            { status },
            { new: true }
        );
        if (!exchange) return res.status(404).json({ error: 'Exchange not found' });
        res.json({ message: 'Exchange updated', exchange });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

module.exports = { logExchange, getExchanges, updateExchangeStatus };