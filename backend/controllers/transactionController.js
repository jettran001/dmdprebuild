const Transaction = require('../models/transaction');

const logTransaction = async (req, res) => {
    try {
        const { txHash, tokenAddress, chain, type, amount, price } = req.body;
        const transaction = new Transaction({ userId: req.user.userId, txHash, tokenAddress, chain, type, amount, price });
        await transaction.save();
        res.status(201).json({ message: 'Transaction logged', txId: transaction._id });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

const getTransactions = async (req, res) => {
    try {
        const transactions = await Transaction.find({ userId: req.user.userId });
        res.json(transactions);
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

const updateTransactionStatus = async (req, res) => {
    try {
        const { txHash } = req.params;
        const { status } = req.body;
        const transaction = await Transaction.findOneAndUpdate(
            { txHash, userId: req.user.userId },
            { status },
            { new: true }
        );
        if (!transaction) return res.status(404).json({ error: 'Transaction not found' });
        res.json({ message: 'Transaction updated', transaction });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

module.exports = { logTransaction, getTransactions, updateTransactionStatus };