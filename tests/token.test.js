const { expect } = require('chai');
const tokenController = require('../backend/controllers/tokenController');

describe('Token Controller', () => {
    it('should get token', async () => {
        const req = { params: { address: '0xTokenAddress' } };
        const res = { json: (data) => expect(data).to.have.property('address') };
        await tokenController.getToken(req, res);
    });
});