require('dotenv').config();

const config = {
    mongoUri: process.env.MONGO_URI || 'mongodb://localhost:27017/diamond_mainnet',
    jwtSecret: process.env.JWT_SECRET || 'secret',
    port: process.env.PORT || 3000,
    privateRpc: {
        bsc: process.env.PRIVATE_RPC_BSC,
        eth: process.env.PRIVATE_RPC_ETH,
        sol: process.env.PRIVATE_RPC_SOL,
        near: process.env.PRIVATE_RPC_NEAR,
        sui: process.env.PRIVATE_RPC_SUI,
        ton: process.env.PRIVATE_RPC_TON
    }
};

module.exports = config;