// views/web/pages/SnipebotPage.jsx
import React, { useState } from 'react';
import axios from 'axios';
import Header from '../components/Header';
import Footer from '../components/Footer';

const SnipebotPage = () => {
    const userId = 'user123';
    const [tokenAddress, setTokenAddress] = useState('');
    const [amount, setAmount] = useState('');
    const [key, setKey] = useState('');
    const [showPopup, setShowPopup] = useState(null);

    const handleSmartBuy = async () => {
        try {
            const response = await axios.post('/api/smartBuy', { chain: 'eth', tokenAddress, amount, userId, key });
            alert(response.data.status);
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    const handleSmartSell = async () => {
        try {
            const response = await axios.post('/api/smartSell', { chain: 'eth', tokenAddress, percentage: 100, userId, key });
            alert(response.data.status);
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    const handlePremium = async (action) => {
        try {
            const response = await axios.post('/api/triggerPremium', { chain: 'eth', tokenAddress, userId, key });
            alert(response.data.status);
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    const handleUltimate = async () => {
        try {
            const response = await axios.post('/api/triggerUltimate', { chain: 'eth', tokenAddress, userId, key });
            alert(response.data.status);
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    return (
        <div style={{ display: 'flex' }}>
            {/* Thanh sub chuyển trang */}
            <div style={{ width: '50px', backgroundColor: '#f1f1f1', padding: '10px', position: 'fixed', height: '100vh' }}>
                {['🔍', '📈', '⚙️'].map((symbol, index) => (
                    <div
                        key={index}
                        style={{ margin: '10px 0', cursor: 'pointer' }}
                        onClick={() => setShowPopup(symbol)}
                    >
                        {symbol}
                    </div>
                ))}
            </div>
            {/* Nội dung chính */}
            <div style={{ marginLeft: '60px', flex: 1 }}>
                <Header telegramId={userId} />
                <main style={{ padding: '20px' }}>
                    <h1>Snipebot</h1>
                    <div style={{ display: 'flex', gap: '20px' }}>
                        {/* Card Smart */}
                        <div style={{ border: '1px solid #ccc', padding: '20px', width: '300px' }}>
                            <h2>Smart</h2>
                            <input
                                type="text"
                                placeholder="Token Address"
                                value={tokenAddress}
                                onChange={(e) => setTokenAddress(e.target.value)}
                                style={{ width: '100%', marginBottom: '10px' }}
                            />
                            <input
                                type="number"
                                placeholder="Amount"
                                value={amount}
                                onChange={(e) => setAmount(e.target.value)}
                                style={{ width: '100%', marginBottom: '10px' }}
                            />
                            <input
                                type="text"
                                placeholder="Key (optional)"
                                value={key}
                                onChange={(e) => setKey(e.target.value)}
                                style={{ width: '100%', marginBottom: '10px' }}
                            />
                            <button onClick={handleSmartBuy} style={{ marginRight: '10px' }}>Buy</button>
                            <button onClick={handleSmartSell}>Sell</button>
                        </div>
                        {/* Card Premium */}
                        <div style={{ border: '1px solid #ccc', padding: '20px', width: '300px' }}>
                            <h2>Premium</h2>
                            <input
                                type="text"
                                placeholder="Token Address"
                                value={tokenAddress}
                                onChange={(e) => setTokenAddress(e.target.value)}
                                style={{ width: '100%', marginBottom: '10px' }}
                            />
                            <input
                                type="text"
                                placeholder="Key (optional)"
                                value={key}
                                onChange={(e) => setKey(e.target.value)}
                                style={{ width: '100%', marginBottom: '10px' }}
                            />
                            <button onClick={() => handlePremium('start')} style={{ marginRight: '10px' }}>Start</button>
                            <button onClick={() => handlePremium('stop')}>Stop</button>
                            <p>Auto-stops after 30 minutes and sells all.</p>
                        </div>
                        {/* Card Ultimate */}
                        <div style={{ border: '1px solid #ccc', padding: '20px', width: '300px' }}>
                            <h2>Ultimate</h2>
                            <input
                                type="text"
                                placeholder="Token Address"
                                value={tokenAddress}
                                onChange={(e) => setTokenAddress(e.target.value)}
                                style={{ width: '100%', marginBottom: '10px' }}
                            />
                            <input
                                type="text"
                                placeholder="Key (optional)"
                                value={key}
                                onChange={(e) => setKey(e.target.value)}
                                style={{ width: '100%', marginBottom: '10px' }}
                            />
                            <button onClick={handleUltimate}>Execute</button>
                            <p>Limited to 1 use.</p>
                        </div>
                    </div>
                </main>
                <Footer />
            </div>
            {/* Popup */}
            {showPopup && (
                <div style={{
                    position: 'fixed', top: '20%', left: '20%', width: '200px', height: '200px',
                    backgroundColor: 'white', border: '1px solid #ccc', zIndex: 1000, padding: '20px'
                }}>
                    <h3>Popup for {showPopup}</h3>
                    <button onClick={() => setShowPopup(null)}>Close</button>
                </div>
            )}
        </div>
    );
};

export default SnipebotPage;