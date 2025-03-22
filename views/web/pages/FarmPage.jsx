// views/web/pages/FarmPage.jsx
import React, { useState, useEffect } from 'react';
import axios from 'axios';
import Header from '../components/Header';
import Footer from '../components/Footer';

const FarmPage = () => {
    const userId = 'user123';
    const [farmStatus, setFarmStatus] = useState([]);
    const [stakeAmount, setStakeAmount] = useState({});
    const [lockPeriod, setLockPeriod] = useState({});
    const [tokenBalances, setTokenBalances] = useState({});
    
    const pools = [
        { name: 'DMD/BTC', apr: 20, token: 'BTC' },
        { name: 'DMD/ETH', apr: 20, token: 'ETH' },
        { name: 'DMD/BNB', apr: 20, token: 'BNB' },
        { name: 'DMD/SOL', apr: 20, token: 'SOL' },
        { name: 'DMD/BASE', apr: 20, token: 'ETH' },
        { name: 'DMD/ARB', apr: 20, token: 'ETH' },
        { name: 'DMD/XLM', apr: 10, token: 'XLM' },
        { name: 'DMD/NEAR', apr: 10, token: 'NEAR' },
        { name: 'DMD/TON', apr: 10, token: 'TON' },
        { name: 'DMD/SUI', apr: 10, token: 'SUI' },
        { name: 'DMD/AVAX', apr: 10, token: 'AVAX' },
        { name: 'DMD/PI', apr: 10, token: 'PI' },
    ];

    useEffect(() => {
        const fetchFarmStatus = async () => {
            try {
                const response = await axios.get(`/api/getFarmStatus?userId=${userId}`);
                setFarmStatus(response.data.farm);
            } catch (error) {
                console.error('Error fetching farm status:', error);
            }
        };
        const fetchTokenBalances = async () => {
            try {
                const response = await axios.get(`/api/getTokenBalances?userId=${userId}`);
                setTokenBalances(response.data);
            } catch (error) {
                console.error('Error fetching token balances:', error);
            }
        };
        fetchFarmStatus();
        fetchTokenBalances();

        // Thêm sự kiện bôi đen text
        document.addEventListener('mouseup', showTooltip);
        document.addEventListener('mousedown', hideTooltip);

        return () => {
            document.removeEventListener('mouseup', showTooltip);
            document.removeEventListener('mousedown', hideTooltip);
        };
    }, []);

    const showTooltip = () => {
        const selection = window.getSelection();
        const selectedText = selection.toString().trim();

        const existingTooltip = document.getElementById('warning-tooltip');
        if (existingTooltip) {
            existingTooltip.remove();
        }

        if (selectedText.length > 0) {
            const range = selection.getRangeAt(0);
            const rect = range.getBoundingClientRect();

            const tooltip = document.createElement('div');
            tooltip.id = 'warning-tooltip';
            tooltip.innerHTML = `⚠️ Warning: "${selectedText}" may contain sensitive information!`;
            document.body.appendChild(tooltip);

            tooltip.style.top = `${rect.top + window.scrollY - 40}px`;
            tooltip.style.left = `${rect.left + window.scrollX}px`;
        }
    };

    const hideTooltip = () => {
        const existingTooltip = document.getElementById('warning-tooltip');
        if (existingTooltip) {
            existingTooltip.remove();
        }
    };

    const handleStake = async (poolName, token) => {
        try {
            const amount = parseFloat(stakeAmount[poolName] || 0);
            const period = lockPeriod[poolName] || 7;
            if (amount > (tokenBalances[token] || 0)) {
                alert(`Insufficient ${token} balance`);
                return;
            }
            const response = await axios.post('/api/stakeFarm', { userId, poolName, amount, lockPeriod: period });
            alert(response.data.status);
            const updatedFarm = await axios.get(`/api/getFarmStatus?userId=${userId}`);
            setFarmStatus(updatedFarm.data.farm);
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    const handleUnstake = async (poolName, lockPeriod) => {
        try {
            const now = new Date();
            const pool = farmStatus.find(p => p.poolName === poolName);
            const lockEnd = new Date(pool.startTime.getTime() + lockPeriod * 24 * 60 * 60 * 1000);
            if (now < lockEnd) {
                const confirmUnstake = window.confirm('Unstaking early will incur a 2% fee. Continue?');
                if (!confirmUnstake) return;
            }
            const response = await axios.post('/api/unstakeFarm', { userId, poolName });
            alert(response.data.status);
            const updatedFarm = await axios.get(`/api/getFarmStatus?userId=${userId}`);
            setFarmStatus(updatedFarm.data.farm);
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    return (
        <div>
            <Header telegramId={userId} />
            <main style={{ padding: '20px' }}>
                <h1>Farm</h1>
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: '20px' }}>
                    {pools.map(pool => {
                        const userPool = farmStatus.find(p => p.poolName === pool.name) || { amount: 0, accumulatedReward: 0, lockPeriod: 7, startTime: new Date() };
                        return (
                            <div key={pool.name} style={{ border: '1px solid #ccc', padding: '20px', width: '300px' }}>
                                <h2>{pool.name}</h2>
                                <p>APR: {pool.apr}%</p>
                                <p>Staked: {userPool.amount} {pool.token}</p>
                                <p>Reward: {userPool.accumulatedReward} DMD</p>
                                <input
                                    type="number"
                                    placeholder={`Amount to stake (Balance: ${tokenBalances[pool.token] || 0} ${pool.token})`}
                                    onChange={(e) => setStakeAmount({ ...stakeAmount, [pool.name]: e.target.value })}
                                    style={{ width: '100%', marginBottom: '10px' }}
                                />
                                <select
                                    onChange={(e) => setLockPeriod({ ...lockPeriod, [pool.name]: parseInt(e.target.value) })}
                                    style={{ width: '100%', marginBottom: '10px' }}
                                >
                                    <option value="7">7 days (1x)</option>
                                    <option value="30">30 days (1.2x)</option>
                                    <option value="90">90 days (1.5x)</option>
                                </select>
                                <button onClick={() => handleStake(pool.name, pool.token)} style={{ marginRight: '10px' }}>
                                    Stake
                                </button>
                                {userPool.amount > 0 && (
                                    <button onClick={() => handleUnstake(pool.name, userPool.lockPeriod)}>
                                        Unstake
                                    </button>
                                )}
                            </div>
                        );
                    })}
                </div>
            </main>
            <Footer />
            <style jsx>{`
                #warning-tooltip {
                    position: absolute;
                    background-color: #ff4444;
                    color: white;
                    padding: 5px 10px;
                    border-radius: 5px;
                    font-size: 14px;
                    z-index: 1000;
                    box-shadow: 0 2px 5px rgba(0, 0, 0, 0.2);
                    max-width: 300px;
                    white-space: nowrap;
                }
            `}</style>
        </div>
    );
};

export default FarmPage;