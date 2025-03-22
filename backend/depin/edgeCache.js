class DepinEdgeCache {
    constructor() {
        this.cache = new Map();
    }

    set(key, value, ttl = 3600) {
        this.cache.set(key, { value, expiry: Date.now() + ttl * 1000 });
    }

    get(key) {
        const item = this.cache.get(key);
        if (!item || Date.now() > item.expiry) {
            this.cache.delete(key);
            return null;
        }
        return item.value;
    }
}

module.exports = DepinEdgeCache;