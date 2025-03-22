const express = require('express');
const snipebotController = require('../controllers/snipebotController');
const auth = require('../middleware/auth');
const router = express.Router();

const chains = ['bsc', 'eth', 'sol', 'base', 'arb', 'near','sui', 'ton',/*  'avax',  'btc', 'pi', 'stellar' */];

chains.forEach(chain => {
    router.post(`/${chain}/manual`, auth, (req, res) => triggerManualBuy({ ...req, params: { chain } }, res));
    router.post(`/${chain}/manual-sell`, auth, (req, res) => triggerManualSell({ ...req, params: { chain } }, res));
    router.get(`/${chain}/sse`, auth, (req, res) => triggerSnipeSSE({ ...req, params: { chain } }, res));
    router.post(`/${chain}/mev`, auth, (req, res) => triggerSnipeMEV({ ...req, params: { chain } }, res));
    router.post(`/${chain}/smart`, auth, (req, res) => triggerSnipeSmart({ ...req, params: { chain } }, res));
    router.get(`/${chain}/profitloss`, auth, (req, res) => getProfitLoss({ ...req, params: { chain } }, res));
    router.post(`/${chain}/stop`, auth, (req, res) => stopSnipe({ ...req, params: { chain } }, res));
    router.post(`/${chain}/settings`, auth, (req, res) => updateSettings({ ...req, params: { chain } }, res));
    router.get(`/${chain}/status`, auth, (req, res) => getBotStatus({ ...req, params: { chain } }, res));
    router.get(`/${chain}/ipfs/:cid`, auth, getTradeFromIPFS);
    router.post('/smartBuy', snipebotController.smartBuy);
    router.post('/smartSell', snipebotController.smartSell);
    router.post('/triggerPremium', snipebotController.triggerPremium);
    router.post('/triggerUltimate', snipebotController.triggerUltimate);
    router.get('/getPNL', snipebotController.getPNL);

});

module.exports = router;