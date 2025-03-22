const { expect } = require('chai');
const walletController = require('../backend/controllers/walletController');

describe('Wallet Controller', () => {
    it('should get wallets', async () => {
        const req = { user: { userId: 'user123' } };
        const res = { json: (data) => expect(data).to.be.an('array') };
        await walletController.getWallets(req, res);
    });
});