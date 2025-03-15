class User {
    static async getAll(pool) {
        return [{ wallet: '0xTestWallet', balance: 100 }]; // Giả lập
    }

    static async getBalance(dmdContract, wallet) {
        const balance = await dmdContract.methods.balanceOf(wallet).call();
        return fastWeb3.utils.fromWei(balance, 'ether');
    }
}

module.exports = User;