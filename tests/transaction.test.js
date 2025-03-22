const { expect } = require('chai');
const transactionController = require('../backend/controllers/transactionController');

describe('Transaction Controller', () => {
    it('should get transactions', async () => {
        const req = { user: { userId: 'user123' } };
        const res = { json: (data) => expect(data).to.be.an('array') };
        await transactionController.getTransactions(req, res);
    });
});