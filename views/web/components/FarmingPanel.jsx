import React, { useEffect, useState } from 'react';
import axios from 'axios';

const FarmingPanel = () => {
    const [farmingStatus, setFarmingStatus] = useState(null);

    useEffect(() => {
        const fetchStatus = async () => {
            const res = await axios.get('/api/farming', {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` }
            });
            setFarmingStatus(res.data);
        };
        fetchStatus();
    }, []);

    const claimReward = async () => {
        await axios.post('/api/farming/claim', {}, {
            headers: { Authorization: `Bearer ${localStorage.getItem('token')}` }
        });
        alert('Reward claimed!');
    };

    return farmingStatus ? (
        <div className="farming-panel">
            <p>Staked: {farmingStatus.stakedAmount}</p>
            <p>Reward: {farmingStatus.reward}</p>
            <button onClick={claimReward}>Claim Reward</button>
        </div>
    ) : <p>Loading...</p>;
};

export default FarmingPanel;