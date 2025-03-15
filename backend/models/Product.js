class Product {
    static async getAll(pool) {
        const { rows } = await pool.query('SELECT * FROM products');
        return rows;
    }

    static async getByType(pool, type) {
        const { rows } = await pool.query('SELECT * FROM products WHERE type = $1', [type]);
        return rows;
    }

    static async buy(pool, producer, itemId, wallet) {
        const { rows } = await pool.query('SELECT * FROM products WHERE id = $1', [itemId]);
        const product = rows[0];
        await producer.send({
            topic: 'diamond-events',
            messages: [{ value: JSON.stringify({ type: 'product_update', productId: itemId }) }]
        });
        return product;
    }
}

module.exports = Product;