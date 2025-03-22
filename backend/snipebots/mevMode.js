class MevMode {
    constructor(bot) {
        this.bot = bot;
    }

    async frontRunMEV(tokenAddress, amount) {
        console.log(`[MEV] Frontrunning ${tokenAddress} with ${amount}`);
        await this.bot.buyToken(tokenAddress, amount);
    }
}

module.exports = MevMode;