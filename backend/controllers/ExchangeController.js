module.exports = {
    swap: async (req, res) => {
        try {
            const { wallet, tokenFrom, tokenTo, amount } = req.body;
            const allowance = await req.dmdContract.methods.allowance(wallet, process.env.FARMING_ADDRESS).call();
            if (parseInt(allowance) < amount) throw new Error('Insufficient allowance');
            res.json({ status: 'Swapped' });
        } catch (err) {
            res.status(400).json({ error: err.message });
        }
    }
};