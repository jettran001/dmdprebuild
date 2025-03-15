class Mission {
    static async getAll(pool, wallet) {
        const { rows } = await pool.query('SELECT * FROM missions WHERE wallet = $1', [wallet]);
        return rows;
    }

    static async complete(pool, dmdContract, missionId, wallet) {
        const { rows } = await pool.query('UPDATE missions SET completed = TRUE WHERE id = $1 AND wallet = $2 RETURNING reward', [missionId, wallet]);
        if (rows.length === 0) throw new Error('Mission not found');
        const reward = rows[0].reward;
        await dmdContract.methods.transfer(wallet, fastWeb3.utils.toWei(reward.toString(), 'ether')).send({ from: process.env.OWNER_ADDRESS });
        return reward;
    }

    static async stake(farmingContract, wallet, amount) {
        const tx = await farmingContract.methods.stake(fastWeb3.utils.toWei(amount, 'ether')).send({ from: wallet });
        return tx;
    }
}

module.exports = Mission;