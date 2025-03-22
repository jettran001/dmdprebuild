// views/web/pages/EarnPage.jsx
import React, { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import axios from 'axios';
import Header from '../components/Header';
import Footer from '../components/Footer';

const EarnPage = () => {
    const userId = 'user123'; //thay bằng logic lấy từ auth
    const navigate = useNavigate();
    const [dailyStatus, setDailyStatus] = useState({ streak: 0, totalDMD: 0, lastSign: null });
    const [miningStatus, setMiningStatus] = useState({ level: 1, speed: 0.01, accumulatedDMD: 0, lastClaim: null });
    const [showDailyPopup, setShowDailyPopup] = useState(false);
    const [canSign, setCanSign] = useState(true);
    const [upgradeCost, setUpgradeCost] = useState(0);
    const [nextClaimTime, setNextClaimTime] = useState(null);

    useEffect(() => {
        const fetchStatus = async () => {
            try {
                const dailyResponse = await axios.get(`/api/getDailyStatus?userId=${userId}`);
                setDailyStatus(dailyResponse.data);
                const today = new Date().toDateString();
                const lastSignDate = dailyResponse.data.lastSign ? new Date(dailyResponse.data.lastSign).toDateString() : null;
                setCanSign(lastSignDate !== today);

                const miningResponse = await axios.get(`/api/getMiningStatus?userId=${userId}`);
                setMiningStatus(miningResponse.data);
                if (miningResponse.data.lastClaim) {
                    const claimIntervals = [1, 3, 6, 12, 24]; // Giờ
                    const interval = claimIntervals[miningResponse.data.level - 1] * 60 * 60 * 1000;
                    const nextClaim = new Date(new Date(miningResponse.data.lastClaim).getTime() + interval);
                    setNextClaimTime(nextClaim);
                }
                setUpgradeCost(miningResponse.data.upgradeCost || [1, 2, 5, 10][miningResponse.data.level - 1]);
            } catch (error) {
                console.error('Error fetching status:', error);
            }
        };
        fetchStatus();
    }, []);

    const handleSignDaily = async () => {
        try {
            const response = await axios.post('/api/signDaily', { userId });
            alert(response.data.status);
            const updatedDaily = await axios.get(`/api/getDailyStatus?userId=${userId}`);
            setDailyStatus(updatedDaily.data);
            setCanSign(false);
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    const handleClaimMining = async () => {
        try {
            const response = await axios.post('/api/claimMining', { userId });
            alert(response.data.status);
            const updatedMining = await axios.get(`/api/getMiningStatus?userId=${userId}`);
            setMiningStatus(updatedMining.data);
            const claimIntervals = [1, 3, 6, 12, 24];
            const interval = claimIntervals[updatedMining.data.level - 1] * 60 * 60 * 1000;
            setNextClaimTime(new Date(new Date().getTime() + interval));
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    const handleUpgradeMining = async () => {
        try {
            const response = await axios.post('/api/upgradeMining', { userId });
            alert(response.data.status);
            const updatedMining = await axios.get(`/api/getMiningStatus?userId=${userId}`);
            setMiningStatus(updatedMining.data);
            setUpgradeCost(updatedMining.data.upgradeCost);
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    return (
        <div>
            <Header telegramId={userId} />
            <main style={{ padding: '20px' }}>
            <h1>Earn</h1>
                <div style={{ display: 'flex', gap: '20px' }}>
                    {/* Card Daily */}
                    <div style={{ border: '1px solid #ccc', padding: '20px', width: '300px' }}>
                        <h2>Daily</h2>
                        <p>Streak: {dailyStatus.streak} days</p>
                        <p>Total DMD: {dailyStatus.totalDMD}</p>
                        <button
                            onClick={() => setShowDailyPopup(true)}
                            disabled={!canSign}
                        >
                            Sign
                        </button>
                    </div>
                    {/* Card Mining */}
                    <div style={{ border: '1px solid #ccc', padding: '20px', width: '300px' }}>
                        <h2>Mining</h2>
                        <p>Level: {miningStatus.level}</p>
                        <p>Speed: {miningStatus.speed} DMD/h</p>
                        <p>Accumulated DMD: {miningStatus.accumulatedDMD}</p>
                        <button onClick={handleClaimMining} style={{ marginRight: '10px' }}>Claim</button>
                        {miningStatus.level < 5 && (
                            <button onClick={handleUpgradeMining}>
                                Upgrade (Cost: {upgradeCost} DMD)
                            </button>
                        )}
                        {nextClaimTime && (
                            <p>Next claim in: {Math.ceil((nextClaimTime - new Date()) / (60 * 60 * 1000))} hours</p>
                        )}
                    </div>
                </div>
            </main>
            <Footer />
            {/* Popup Daily */}
            {showDailyPopup && (
                <div style={{
                    position: 'fixed', top: '20%', left: '20%', width: '300px', height: '300px',
                    backgroundColor: 'white', border: '1px solid #ccc', zIndex: 1000, padding: '20px'
                }}>
                    <h3>Daily Sign-In</h3>
                    <p>Sign in to earn {dailyStatus.streak >= 7 ? 0.1 : 0.05} DMD!</p>
                    <button onClick={handleSignDaily} style={{ marginRight: '10px' }}>Sign</button>
                    <button onClick={() => setShowDailyPopup(false)}>Close</button>
                </div>
            )}
        </div>
    );
};

export default EarnPage;