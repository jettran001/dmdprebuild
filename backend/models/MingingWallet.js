class MiningWallet {
    static async get(pool, wallet) {
        const { rows } = await pool.query('SELECT balance, last_mined FROM mining_wallets WHERE wallet = $1', [wallet]);
        if (rows.length === 0) {
            await pool.query('INSERT INTO mining_wallets (wallet, balance, last_mined) VALUES ($1, 0, NOW())', [wallet]);
            return { balance: 0, lastMined: new Date().toISOString() };
        }
        return { balance: rows[0].balance, lastMined: rows[0].last_mined };
    }

    static async withdraw(pool, dmdContract, wallet, amount) {
        const { rows } = await pool.query('SELECT balance FROM mining_wallets WHERE wallet = $1', [wallet]);
        if (rows.length === 0 || rows[0].balance < amount) throw new Error('Insufficient balance');
        await pool.query('UPDATE mining_wallets SET balance = balance - $1 WHERE wallet = $2', [amount, wallet]);
        await dmdContract.methods.transfer(wallet, fastWeb3.utils.toWei(amount.toString(), 'ether')).send({ from: process.env.OWNER_ADDRESS });
    }
}

module.exports = MiningWallet;