const NFT = require('../models/nft');

const addNFT = async (req, res) => {
    try {
        const { tokenId, contractAddress, chain, name, description, imageUrl, attributes } = req.body;
        const nft = new NFT({
            userId: req.user.userId,
            tokenId,
            contractAddress,
            chain,
            name,
            description,
            imageUrl,
            attributes
        });
        await nft.save();
        res.status(201).json({ message: 'NFT added', nftId: nft._id });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

const getNFTs = async (req, res) => {
    try {
        const nfts = await NFT.find({ userId: req.user.userId });
        res.json(nfts);
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

const getNFTById = async (req, res) => {
    try {
        const { tokenId } = req.params;
        const nft = await NFT.findOne({ tokenId, userId: req.user.userId });
        if (!nft) return res.status(404).json({ error: 'NFT not found' });
        res.json(nft);
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
};

module.exports = { addNFT, getNFTs, getNFTById };