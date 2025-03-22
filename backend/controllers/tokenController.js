const Token = require('../models/token');

const addToken = async (req, res) => {
    try {
        const { address, name, symbol, decimals, totalSupply } = req.body;
        const token = new Token({ address, name, symbol, decimals, totalSupply });
        await token.save();
        res.status(201).json({ message: 'Token added', tokenId: token._id });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

const getTokenInfo = async (req, res) => {
    try {
        const { address } = req.params;
        const token = await Token.findOne({ address });
        if (!token) return res.status(404).json({ error: 'Token not found' });
        res.json(token);
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

const updateTokenStatus = async (req, res) => {
    try {
        const { address } = req.params;
        const { auditScore, liquidity, tax, isVerified } = req.body;
        const token = await Token.findOneAndUpdate(
            { address },
            { auditScore, liquidity, tax, isVerified },
            { new: true }
        );
        if (!token) return res.status(404).json({ error: 'Token not found' });
        res.json({ message: 'Token updated', token });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

module.exports = { addToken, getTokenInfo, updateTokenStatus };