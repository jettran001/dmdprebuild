// backend/routes/wallet.js
const express = require('express');
const router = express.Router();
const walletController = require('../controllers/walletController');
const earnController = require('../controllers/earnController');
const farmingController = require('../controllers/farmingController');
const tokenSearch = require('../utils/tokenSearch');
const { verifyAdmin } = require('../../admin/auth');
const fs = require('fs').promises;
const path = require('path');
const crypto = require('crypto');
const { ethers } = require('ethers');
const { cacheMiddleware, setCache } = require('../redis');

//wallet create
router.post('/createWallet', walletController.createWallet);
router.post('/importWallet', walletController.importWallet);
router.get('/walletBalance/:telegramId/:chain', walletController.getWalletBalance);
router.post('/connectToBot', walletController.connectToBot);
router.post('/getMultiChainBalance', walletController.getMultiChainBalance);
//Farm
router.post('/joinFarmingPool', farmingController.joinPool);
router.post('/exitFarmingPool', farmingController.exitPool);
router.get('/farmingRewards/:telegramId', farmingController.getRewards);
//Earn
router.post('/stakeFarm', earnController.stakeFarm);
router.post('/unstakeFarm', earnController.unstakeFarm);
router.get('/getFarmStatus', earnController.getFarmStatus);
router.post('/signDaily', earnController.signDaily);
router.get('/getDailyStatus', earnController.getDailyStatus);
router.post('/claimMining', earnController.claimMining);
router.post('/upgradeMining', earnController.upgradeMining);
router.get('/getMiningStatus', earnController.getMiningStatus);

// Middleware kiểm tra admin
const isAdmin = (req, res, next) => {
    const token = req.headers.authorization?.split(' ')[1];
    if (!token) return res.status(401).json({ error: 'No token provided' });

    try {
        const decoded = verifyAdmin(token);
        req.admin = decoded;
        next();
    } catch (error) {
        res.status(401).json({ error: 'Unauthorized' });
    }
};
// Giả lập Oracle để lấy giá USD
const getPriceInUSD = async (token) => {
    // Giả lập giá (có thể thay bằng API Oracle như Chainlink)
    const prices = { ETH: 3000, BNB: 500, USDC: 1 };
    return prices[token] || 1;
};
// Tạo key ngẫu nhiên
const generateKey = () => {
    return crypto.randomBytes(16).toString('hex');
};


// API tạo key (cho admin và user)
router.post('/admin/bot/createKey', async (req, res) => {
    try {
        const { type, inviterId, additionalKeys, userId, walletAddress, paymentToken, chain } = req.body;
        const isAdminRequest = req.headers.authorization?.split(' ')[1] && isAdmin(req, res, () => {});

        const keysPath = path.join(__dirname, '../../admin/storage/keys.json');
        let keys = [];
        try {
            const data = await fs.readFile(keysPath, 'utf8');
            keys = JSON.parse(data);
        } catch (error) {
            keys = [];
        }

        const totalKeys = keys.length;
        const priceIncrease = Math.floor(totalKeys / 100); // Tăng $1 sau mỗi 100 key

        let totalCost = 0;
        let newKeys = [];

        if (type === 'single') {
            const costPerKey = 65 + priceIncrease;
            totalCost = costPerKey;
            newKeys.push({ key: generateKey(), type: 'single', inviterId, userId, walletAddress, createdAt: new Date() });
        } else if (type === 'team') {
            const numKeys = 5 + (additionalKeys || 0);
            const costPerKey = 50 + priceIncrease;
            totalCost = costPerKey * numKeys;
            const inviterKey = { key: generateKey(), type: 'inviter', inviterId, userId, walletAddress, createdAt: new Date() };
            newKeys.push(inviterKey);
            for (let i = 0; i < numKeys - 1; i++) {
                newKeys.push({ key: generateKey(), type: 'member', inviterId: inviterKey.key, userId, walletAddress, createdAt: new Date() });
            }
        }

        if (!isAdminRequest) {
            // Tính giá theo token thanh toán
            const usdPrice = totalCost;
            const tokenPrice = usdPrice / (await getPriceInUSD(paymentToken));
            // Giả lập kiểm tra balance và thanh toán (sẽ tích hợp smart contract sau)
            if (!walletAddress) throw new Error('Wallet address required for payment');
        }

        // Lưu key vào Log_Keys
        keys.push(...newKeys);
        await fs.writeFile(keysPath, JSON.stringify(keys, null, 2));

        // Cộng referral cho inviter (10% commission)
        if (inviterId) {
            const usersPath = path.join(__dirname, '../../admin/storage/users.json');
            let users = [];
            try {
                const data = await fs.readFile(usersPath, 'utf8');
                users = JSON.parse(data);
            } catch (error) {
                users = [];
            }
            const inviter = users.find(u => u.id === inviterId);
            if (inviter) {
                inviter.commission = (inviter.commission || 0) + totalCost * 0.1;
                await fs.writeFile(usersPath, JSON.stringify(users, null, 2));
            }
        }

        res.json({ keys: newKeys, totalCost });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});
// API lấy Log_Keys
router.get('/admin/bot/logKeys', isAdmin, async (req, res) => {
    try {
        const keysPath = path.join(__dirname, '../../admin/storage/keys.json');
        const data = await fs.readFile(keysPath, 'utf8');
        const keys = JSON.parse(data);
        res.json(keys);
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});
// API active key và lưu HWID
router.post('/node/activateKey', async (req, res) => {
    try {
        const { key, hwid, walletAddress, userId } = req.body;

        const keysPath = path.join(__dirname, '../../admin/storage/keys.json');
        const usersPath = path.join(__dirname, '../../admin/storage/users.json');

        let keys = JSON.parse(await fs.readFile(keysPath, 'utf8'));
        let users = [];
        try {
            const data = await fs.readFile(usersPath, 'utf8');
            users = JSON.parse(data);
        } catch (error) {
            users = [];
        }

        const keyData = keys.find(k => k.key === key);
        if (!keyData) throw new Error('Key not found');

        // Mã hóa HWID bằng SHA-256
        const hashedHwid = crypto.createHash('sha256').update(hwid).digest('hex');

        // Kiểm tra HWID duy nhất
        const existingHwid = users.some(u => u.hwid === hashedHwid);
        if (existingHwid) throw new Error('HWID already used');

        // Tạo hoặc cập nhật user
        let user = users.find(u => u.id === userId || u.walletAddress === walletAddress);
        if (!user) {
            user = { id: userId || crypto.randomBytes(8).toString('hex'), walletAddress, keys: [], hwid: null };
            users.push(user);
        }

        // Lưu key và HWID
        user.keys = user.keys || [];
        if (!user.keys.includes(key)) {
            user.keys.push(key);
            user.hwid = hashedHwid;
        }

        // Nếu key là member, cộng referral cho inviter
        if (keyData.type === 'member' && keyData.inviterId) {
            const inviterKey = keys.find(k => k.key === keyData.inviterId);
            const inviter = users.find(u => u.id === inviterKey.userId);
            if (inviter) {
                inviter.commission = (inviter.commission || 0) + 5; // Giả lập commission
            }
        }

        await fs.writeFile(usersPath, JSON.stringify(users, null, 2));
        await fs.writeFile(keysPath, JSON.stringify(keys, null, 2));

        res.json({ status: 'Key activated', userId: user.id });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});
// API tạo NFT từ key 
router.post('/node/createNFT', async (req, res) => {
    try {
        const { key, walletAddress, userId } = req.body;

        const keysPath = path.join(__dirname, '../../admin/storage/keys.json');
        const usersPath = path.join(__dirname, '../../admin/storage/users.json');
        const nftsPath = path.join(__dirname, '../../admin/storage/nfts.json');

        let keys = JSON.parse(await fs.readFile(keysPath, 'utf8'));
        let users = JSON.parse(await fs.readFile(usersPath, 'utf8'));
        let nfts = [];
        try {
            const data = await fs.readFile(nftsPath, 'utf8');
            nfts = JSON.parse(data);
        } catch (error) {
            nfts = [];
        }

        const keyData = keys.find(k => k.key === key);
        if (!keyData) throw new Error('Key not found');

        const user = users.find(u => u.id === userId || u.walletAddress === walletAddress);
        if (!user) throw new Error('User not found');

        const nftImage = keyData.type === 'single' ? 'Node_Solo' : 'Node_Team';
        const nft = {
            id: crypto.randomBytes(8).toString('hex'),
            userId: user.id,
            walletAddress,
            hwid: user.hwid,
            image: nftImage,
            key
        };

        // Cấp quyền Premium/Ultimate dựa trên loại key
        user.botAccess = keyData.type === 'single' ? 'Premium' : 'Ultimate';

        nfts.push(nft);
        await fs.writeFile(nftsPath, JSON.stringify(nfts, null, 2));
        await fs.writeFile(usersPath, JSON.stringify(users, null, 2));

        res.json({ status: 'NFT created', nft, botAccess: user.botAccess });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});
// API kiểm tra quyền truy cập bot của user
router.get('/user/botAccess', async (req, res) => {
    try {
        const { userId, walletAddress } = req.query;

        const usersPath = path.join(__dirname, '../../admin/storage/users.json');
        const users = JSON.parse(await fs.readFile(usersPath, 'utf8'));

        const user = users.find(u => u.id === userId || u.walletAddress === walletAddress);
        if (!user) throw new Error('User not found');

        res.json({ botAccess: user.botAccess || 'Basic' });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});

//Search
router.get('/searchToken', async (req, res) => {
    try {
        const { query } = req.query;
        if (!query) return res.status(400).json({ error: 'Missing query parameter' });

        const tokens = await tokenSearch.searchToken(query);
        res.json(tokens);
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});
router.get('/auditToken', async (req, res) => {
    try {
        const { tokenAddress, chain } = req.query;
        if (!tokenAddress || !chain) return res.status(400).json({ error: 'Missing tokenAddress or chain' });

        const auditResult = await tokenSearch.auditToken(tokenAddress, chain);
        res.json(auditResult);
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});
    // API đăng nhập admin
router.post('/admin/login', (req, res) => {
    try {
        const { username, password } = req.body;
        const token = loginAdmin(username, password);
        res.json({ token });
    } catch (error) {
        res.status(401).json({ error: error.message });
    }
});
// API lấy dữ liệu cho dashboard (chỉ admin)
router.get('/admin/dashboard', isAdmin, cacheMiddleware('dashboard'), async (req, res) => {
    try {
        const data = {
            ccu: {
                day: { pc: 1200, mobile: 800, extension: 300 },
                week: { pc: 8500, mobile: 6000, extension: 2000 }
            },
            nodes: { day: 150, week: 900 },
            heatmap: [
                { country: 'US', value: 5000 },
                { country: 'VN', value: 3000 },
                { country: 'JP', value: 2000 }
            ],
            bots: { active: 50, total: 100 },
            chains: { active: 5, total: 10 },
            tokenLogs: [
                { token: 'DMD/BTC', contractAddress: '0x123...', trades: 150 },
                { token: 'DMD/ETH', contractAddress: '0x456...', trades: 120 }
            ]
        };
        // Lưu vào cache
        await setCache(req.cacheKey, data, 3600); // Cache 1 giờ
        res.json(data);
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});
// API lấy dữ liệu Depin_adminManager
router.get('/admin/depin', isAdmin, async (req, res) => {
    try {
        // Giả lập số block đã đào
        const blocksMined = 1250;

        // Đọc danh sách node từ storage/nodes.json
        const nodesPath = path.join(__dirname, '../../admin/storage/nodes.json');
        let nodes = [];
        try {
            const data = await fs.readFile(nodesPath, 'utf8');
            nodes = JSON.parse(data);
        } catch (error) {
            // Nếu file không tồn tại, tạo file mặc định
            nodes = [
                { ip: '192.168.1.1', user: '', password: '' },
                { ip: '192.168.1.2', user: '', password: '' }
            ];
            await fs.writeFile(nodesPath, JSON.stringify(nodes, null, 2));
        }

        res.json({ blocksMined, nodes });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});
// API cập nhật user/password cho node
router.post('/admin/depin/setProxy', isAdmin, async (req, res) => {
    try {
        const { ip, user, password } = req.body;
        const nodesPath = path.join(__dirname, '../../admin/storage/nodes.json');
        const data = await fs.readFile(nodesPath, 'utf8');
        let nodes = JSON.parse(data);

        const node = nodes.find(n => n.ip === ip);
        if (node) {
            node.user = user;
            node.password = password;
        }

        await fs.writeFile(nodesPath, JSON.stringify(nodes, null, 2));
        res.json({ status: 'Proxy set successfully' });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});
// API lấy danh sách pool
router.get('/admin/pools', isAdmin, async (req, res) => {
    try {
        // Giả lập dữ liệu pool (liên kết với trang Farm)
        const pools = [
            { name: 'DMD/BTC', totalTokens: 5000, reserve: 1000 },
            { name: 'DMD/ETH', totalTokens: 3000, reserve: 500 },
            { name: 'DMD/BNB', totalTokens: 2000, reserve: 300 },
            { name: 'DMD/SOL', totalTokens: 1500, reserve: 200 },
            { name: 'DMD/BASE', totalTokens: 1000, reserve: 100 },
            { name: 'DMD/ARB', totalTokens: 800, reserve: 80 },
            { name: 'DMD/XLM', totalTokens: 600, reserve: 60 },
            { name: 'DMD/NEAR', totalTokens: 500, reserve: 50 },
            { name: 'DMD/TON', totalTokens: 400, reserve: 40 },
            { name: 'DMD/SUI', totalTokens: 300, reserve: 30 },
            { name: 'DMD/AVAX', totalTokens: 200, reserve: 20 },
            { name: 'DMD/PI', totalTokens: 100, reserve: 10 }
        ];
        res.json(pools);
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});
// API chuyển token về Reserve
router.post('/admin/pools/reserve', isAdmin, async (req, res) => {
    try {
        const { poolName, amount } = req.body;
        // Giả lập logic chuyển token về Reserve (sẽ tích hợp smart contract sau)
        res.json({ status: `Successfully reserved ${amount} tokens from ${poolName}` });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});
// API chuyển token từ Reserve về pool
router.post('/admin/pools/deposit', isAdmin, async (req, res) => {
    try {
        const { poolName, amount } = req.body;
        // Giả lập logic chuyển token từ Reserve về pool (sẽ tích hợp smart contract sau)
        res.json({ status: `Successfully deposited ${amount} tokens to ${poolName}` });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});
// API lấy danh sách nhiệm vụ chờ duyệt
router.get('/admin/promo/pendingTasks', isAdmin, async (req, res) => {
    try {
        // Đọc từ storage/mission_pending.json
        const pendingPath = path.join(__dirname, '../../admin/storage/mission_pending.json');
        let pendingTasks = [];
        try {
            const data = await fs.readFile(pendingPath, 'utf8');
            pendingTasks = JSON.parse(data);
        } catch (error) {
            pendingTasks = [
                { id: 1, name: 'Task 1', description: 'Complete a survey', reward: 10 },
                { id: 2, name: 'Task 2', description: 'Invite a friend', reward: 20 }
            ];
            await fs.writeFile(pendingPath, JSON.stringify(pendingTasks, null, 2));
        }
        res.json(pendingTasks);
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});
// API duyệt nhiệm vụ và push sang Mission
router.post('/admin/promo/confirmTask', isAdmin, async (req, res) => {
    try {
        const { taskId } = req.body;
        const pendingPath = path.join(__dirname, '../../admin/storage/mission_pending.json');
        const missionsPath = path.join(__dirname, '../../admin/storage/missions.json');

        // Đọc danh sách nhiệm vụ chờ duyệt
        const pendingData = await fs.readFile(pendingPath, 'utf8');
        let pendingTasks = JSON.parse(pendingData);
        const task = pendingTasks.find(t => t.id === taskId);
        if (!task) throw new Error('Task not found');

        // Xóa nhiệm vụ khỏi danh sách chờ duyệt
        pendingTasks = pendingTasks.filter(t => t.id !== taskId);
        await fs.writeFile(pendingPath, JSON.stringify(pendingTasks, null, 2));

        // Thêm nhiệm vụ vào danh sách Mission
        let missions = [];
        try {
            const missionsData = await fs.readFile(missionsPath, 'utf8');
            missions = JSON.parse(missionsData);
        } catch (error) {
            missions = [];
        }
        missions.push(task);
        await fs.writeFile(missionsPath, JSON.stringify(missions, null, 2));

        res.json({ status: 'Task confirmed and pushed to Mission' });
    } catch (error) {
        res.status(500).json({ error: error.message });
    }
});

module.exports = router;