const express = require('express');
const router = express.Router();
const ProductController = require('../controllers/ProductController');
const ProxyController = require('../controllers/ProxyController');
const NodeController = require('../controllers/NodeController');
const MissionController = require('../controllers/MissionController');
const MiningWalletController = require('../controllers/MiningWalletController');
const NFTController = require('../controllers/NFTController');
const UserController = require('../controllers/UserController');
const ExchangeController = require('../controllers/ExchangeController');
const SnipebotController = require('../controllers/SnipebotController');
const StatsController = require('../controllers/StatsController');
const roleMiddleware = require('../middleware/role');
const sanitizeInput = require('../middleware/sanitize');

router.get('/products', ProductController.getProducts);
router.get('/bandwidth-storage', ProductController.getBandwidthStorage);
router.get('/nfts', NFTController.getNFTs);

router.post('/buy', sanitizeInput(['itemId', 'wallet']), ProductController.buyProduct);
router.get('/proxies', ProxyController.getProxies);
router.get('/missions', MissionController.getMissions);
router.post('/complete-mission', sanitizeInput(['missionId', 'wallet']), MissionController.completeMission);
router.post('/mining-wallet', sanitizeInput(['wallet']), MiningWalletController.getMiningWallet);
router.post('/withdraw-mining', sanitizeInput(['wallet', 'amount']), MiningWalletController.withdrawMining);
router.post('/balance', sanitizeInput(['wallet']), UserController.getBalance);
router.post('/exchange/swap', sanitizeInput(['wallet', 'tokenFrom', 'tokenTo', 'amount']), ExchangeController.swap);
router.post('/snipebot/snipe', sanitizeInput(['wallet', 'token', 'amount', 'chain']), SnipebotController.snipe);
router.post('/stake', sanitizeInput(['wallet', 'amount']), MissionController.stake);
router.post('/node-keys', NodeController.getNodeKeys);
router.post('/generate-key', sanitizeInput(['hwid']), NodeController.generateKey);
router.post('/delete-key', sanitizeInput(['hwid']), NodeController.deleteKey);

router.get('/nodes', roleMiddleware(['admin']), NodeController.getNodes);
router.post('/delist', roleMiddleware(['admin']), sanitizeInput(['hwid']), NodeController.delistNode);
router.post('/mint-nft', roleMiddleware(['admin']), sanitizeInput(['uri']), NFTController.mintNFT);
router.post('/buy-nft', sanitizeInput(['nftId', 'wallet']), NFTController.buyNFT);
router.get('/users', roleMiddleware(['admin']), UserController.getUsers);
router.get('/stats', roleMiddleware(['admin']), StatsController.getStats);

module.exports = router;