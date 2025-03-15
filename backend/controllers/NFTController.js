const NFT = require('../models/NFT');

module.exports = {
    getNFTs: async (req, res) => {
        try {
            const nfts = await NFT.getAll(req.farmingContract);
            res.json(nfts);
        } catch (err) {
            res.status(500).json({ error: err.message });
        }
    },
    mintNFT: async (req, res) => {
        try {
            const { uri } = req.body;
            const tx = await NFT.mint(req.farmingContract, uri);
            res.json({ status: 'NFT minted', tx });
        } catch (err) {
            res.status(400).json({ error: err.message });
        }
    },
    buyNFT: async (req, res) => {
        try {
            const { nftId, wallet } = req.body;
            await NFT.buy(req.farmingContract, nftId, wallet);
            res.json({ status: 'NFT bought' });
        } catch (err) {
            res.status(400).json({ error: err.message });
        }
    }
};