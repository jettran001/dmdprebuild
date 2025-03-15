module.exports = {
    snipe: async (req, res) => {
        try {
            const { wallet, token, amount, chain } = req.body;
            await new Promise(resolve => setTimeout(resolve, Math.floor(Math.random() * 1000)));
            res.json({ status: 'Sniped' });
        } catch (err) {
            res.status(400).json({ error: err.message });
        }
    }
};