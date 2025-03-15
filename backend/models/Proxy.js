class Proxy {
    static async getAll(pool) {
        const { rows } = await pool.query('SELECT hwid, ip, country FROM proxies');
        return rows;
    }
}

module.exports = Proxy;