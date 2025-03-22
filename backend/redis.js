// backend/redis.js
const redis = require('redis');

// Kết nối Redis
const client = redis.createClient({
    url: 'redis://localhost:6379', // Nếu có mật khẩu: 'redis://:YOUR_REDIS_PASSWORD@localhost:6379'
});

client.on('error', (err) => console.log('Redis Client Error', err));

// Kết nối Redis
(async () => {
    await client.connect();
})();

// Hàm cache API result
const cacheMiddleware = (cacheKeyPrefix) => async (req, res, next) => {
    const cacheKey = `${cacheKeyPrefix}:${JSON.stringify(req.query)}:${JSON.stringify(req.body)}`;
    try {
        const cachedData = await client.get(cacheKey);
        if (cachedData) {
            return res.json(JSON.parse(cachedData));
        }
        req.cacheKey = cacheKey;
        next();
    } catch (error) {
        console.error('Redis Cache Error:', error);
        next();
    }
};

// Hàm lưu cache
const setCache = async (key, data, ttl = 3600) => {
    await client.setEx(key, ttl, JSON.stringify(data));
};

module.exports = { client, cacheMiddleware, setCache };