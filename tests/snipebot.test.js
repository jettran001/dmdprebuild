const { expect } = require('chai');
const { SnipeBot } = require('../backend/snipebots/snipebot_bsc');

describe('SnipeBot', () => {
    let bot;

    beforeEach(() => {
        bot = new SnipeBot(process.env.PRIVATE_KEY);
    });

    it('should initialize correctly', async () => {
        await bot.init();
        expect(bot.account).to.exist;
    });

    it('should buy token', async () => {
        const amount = await bot.buyToken('0xTokenAddress', 0.1);
        expect(amount).to.equal(0.1);
    });
});