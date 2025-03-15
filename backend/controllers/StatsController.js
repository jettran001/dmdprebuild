const Node = require('../models/Node');
const Mission = require('../models/Mission');
const NFT = require('../models/NFT');

module.exports = {
    getStats: async (req, res) => {
        try {
            const [nodes, nfts, users] = await Promise.all([
                Node.getAll(req.pool).then(nodes => nodes.length),
                NFT.getAll(req.farmingContract).then(nfts => nfts.length),
                req.pool.query('SELECT COUNT(DISTINCT wallet) FROM missions').then(r => r.rows[0].count)
            ]);
            res.json({ nodes, nfts, users });
        } catch (err) {
            res.status(500).json({ error: err.message });
        }
    }
};