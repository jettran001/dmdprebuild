const IPFS = require('ipfs-core');

class IPFSNode {
    constructor() {
        this.node = null;
    }

    async init() {
        this.node = await IPFS.create({
            repo: './ipfs-repo', // Thư mục lưu trữ dữ liệu IPFS
            config: {
                Addresses: {
                    Swarm: ['/ip4/0.0.0.0/tcp/4001'],
                    API: '/ip4/127.0.0.1/tcp/5001',
                    Gateway: '/ip4/127.0.0.1/tcp/8080'
                }
            }
        });
        console.log('[IPFS] Node started:', (await this.node.id()).id);
    }

    async addData(data) {
        const { cid } = await this.node.add(JSON.stringify(data));
        console.log('[IPFS] Data added, CID:', cid.toString());
        return cid.toString();
    }

    async getData(cid) {
        const stream = this.node.cat(cid);
        let data = '';
        for await (const chunk of stream) {
            data += chunk.toString();
        }
        return JSON.parse(data);
    }

    async stop() {
        await this.node.stop();
        console.log('[IPFS] Node stopped');
    }
}

module.exports = IPFSNode;