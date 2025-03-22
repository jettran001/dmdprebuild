const IPFS = require('ipfs-core');
const OrbitDB = require('orbit-db');

class IPFSStorage {
    constructor() {
        this.ipfs = null;
        this.orbitdb = null;
        this.db = null;
    }

    async init() {
        this.ipfs = await IPFS.create();
        this.orbitdb = await OrbitDB.createInstance(this.ipfs);
        this.db = await this.orbitdb.keyvalue('depin-data');
        await this.db.load();
    }

    async storeData(key, value) {
        await this.db.put(key, value);
        return this.db.address.toString();
    }

    async getData(key) {
        return await this.db.get(key);
    }
}

module.exports = new IPFSStorage();