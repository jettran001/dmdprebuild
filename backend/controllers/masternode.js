class Masternode {
    async validateTransaction(transaction) {
        return transaction.amount > 0 && transaction.to.length > 0;
    }
}

module.exports = new Masternode();