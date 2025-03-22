const express = require('express');
const router = express.Router();
const masternode = require('../controllers/masternode');

const {
    registerUser, loginUser, getUserProfile
} = require('../controllers/userController');
const {
    addToken, getTokenInfo, updateTokenStatus
} = require('../controllers/tokenController');
const {
    startFarming, claimFarmingReward, getFarmingStatus
} = require('../controllers/farmingController');
const {
    logTransaction, getTransactions, updateTransactionStatus
} = require('../controllers/transactionController');
const {
    addWallet, getWallets, updateWalletBalance
} = require('../controllers/walletController');
const {
    addNFT, getNFTs, getNFTById
} = require('../controllers/nftController');
const {
    logExchange, getExchanges, updateExchangeStatus
} = require('../controllers/exchangeController');
const {
    addNode, getNodes, updateNodeStatus
} = require('../controllers/nodeController');
const {
    addMission, getMissions, completeMission
} = require('../controllers/missionController');
const auth = require('../middleware/auth');


// User routes
router.post('/users/register', registerUser);
router.post('/users/login', loginUser);
router.get('/users/profile', auth, getUserProfile);

// Token routes
router.post('/tokens', auth, addToken);
router.get('/tokens/:address', auth, getTokenInfo);
router.put('/tokens/:address', auth, updateTokenStatus);

// Farming routes
router.post('/farming', auth, startFarming);
router.post('/farming/:farmingId/claim', auth, claimFarmingReward);
router.get('/farming', auth, getFarmingStatus);

// Transaction routes
router.post('/transactions', auth, logTransaction);
router.get('/transactions', auth, getTransactions);
router.put('/transactions/:txHash', auth, updateTransactionStatus);

// Wallet routes
router.post('/wallets', auth, addWallet);
router.get('/wallets', auth, getWallets);
router.put('/wallets/:walletId', auth, updateWalletBalance);

// NFT routes
router.post('/nfts', auth, addNFT);
router.get('/nfts', auth, getNFTs);
router.get('/nfts/:tokenId', auth, getNFTById);

// Exchange routes
router.post('/exchanges', auth, logExchange);
router.get('/exchanges', auth, getExchanges);
router.put('/exchanges/:txHash', auth, updateExchangeStatus);

// Node routes
router.post('/nodes', auth, addNode);
router.get('/nodes', auth, getNodes);
router.put('/nodes/:nodeId', auth, updateNodeStatus);

// Mission routes
router.post('/missions', auth, addMission);
router.get('/missions', auth, getMissions);
router.put('/missions/:missionId', auth, completeMission);

router.post('/sendTransaction', async (req, res) => {
    try {
        const transaction = req.body;
        const isValid = await masternode.validateTransaction(transaction);
        if (isValid) {
            res.json({ status: 'Transaction processed' });
        } else {
            res.status(400).json({ error: 'Invalid transaction' });
        }
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});

router.get('/user/:userId', async (req, res) => {
    try {
        const userId = req.params.userId;
        res.json({ userId, name: 'John Doe', balance: 1000 });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});

module.exports = router;