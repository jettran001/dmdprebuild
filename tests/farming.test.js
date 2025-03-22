const { expect } = require('chai');
const farmingController = require('../backend/controllers/farmingController');

describe('Farming Controller', () => {
    it('should claim reward', async () => {
        const req = { user: { userId: 'user123' } };
        const res = { json: (data) => expect(data).to.have.property('status') };
        await farmingController.claimFarmingReward(req, res);
    });
});