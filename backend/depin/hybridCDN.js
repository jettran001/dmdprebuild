const { createLibp2pNode } = require('./libp2pNode');

class HybridCDN {
    constructor(cdnUrl) {
        this.cdnUrl = cdnUrl; // Ví dụ: 'https://cdn.example.com'
        this.node = null;
    }

    async init() {
        this.node = await createLibp2pNode();
        this.node.handle('/cdn', (protocol, conn) => {
            conn.getStream().on('data', (data) => {
                console.log('[Hybrid CDN] P2P Data:', data.toString());
            });
        });
    }

    async fetchData(resourceId) {
        // Ưu tiên CDN truyền thống
        try {
            const response = await fetch(`${this.cdnUrl}/${resourceId}`);
            return await response.text();
        } catch (error) {
            console.log('[Hybrid CDN] Falling back to P2P');
            // Fallback sang P2P
            return this.fetchViaP2P(resourceId);
        }
    }

    fetchViaP2P(resourceId) {
        return new Promise((resolve) => {
            const stream = this.node.dialProtocol('/cdn');
            stream.write(resourceId);
            stream.on('data', (data) => resolve(data.toString()));
        });
    }
}

module.exports = HybridCDN;