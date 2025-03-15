class Node {
    static async getAll(pool) {
        const { rows } = await pool.query('SELECT * FROM nodes WHERE active = TRUE');
        return rows;
    }

    static async delist(pool, hwid) {
        await pool.query('UPDATE nodes SET active = FALSE WHERE hwid = $1', [hwid]);
    }

    static async generateKey(pool, hwid) {
        await pool.query('INSERT INTO nodes (hwid, available_bandwidth, price, active) VALUES ($1, 1000, 0.1, TRUE) ON CONFLICT (hwid) DO NOTHING', [hwid]);
        return 'key_' + hwid;
    }

    static async deleteKey(pool, hwid) {
        await pool.query('UPDATE nodes SET active = FALSE WHERE hwid = $1', [hwid]);
    }
}

module.exports = Node;