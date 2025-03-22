// views/web/pages/ExchangePage.jsx
import React, { useState } from 'react';
import axios from 'axios';
import Header from '../components/Header';
import Footer from '../components/Footer';

const ExchangePage = () => {
    const telegramId = 'user123';
    const [searchQuery, setSearchQuery] = useState('');
    const [tokens, setTokens] = useState([]);
    const [selectedToken, setSelectedToken] = useState(null);
    const [amount, setAmount] = useState('');
    const [showChart, setShowChart] = useState(false);

    const handleSearch = async () => {
        try {
            const response = await axios.get(`/api/searchToken?query=${searchQuery}`);
            setTokens(response.data);
        } catch (error) {
            console.error(error);
        }
    };

    const handleBuy = async () => {
        try {
            await axios.post('/api/triggerManualBuy', {
                chain: selectedToken.chain,
                tokenAddress: selectedToken.address,
                amount,
                poolId: 'default-pool',
            });
            alert('Buy successful');
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    const handleSell = async () => {
        try {
            await axios.post('/api/triggerManualSell', {
                chain: selectedToken.chain,
                tokenAddress: selectedToken.address,
                percentage: 100,
            });
            alert('Sell successful');
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    return (
        <div>
            <Header telegramId={telegramId} />
            <main style={{ padding: '20px' }}>
                <h1>Exchange</h1>
                {/* Thanh tìm kiếm */}
                <div style={{ marginBottom: '20px' }}>
                    <input
                        type="text"
                        value={searchQuery}
                        onChange={(e) => setSearchQuery(e.target.value)}
                        placeholder="Search token..."
                        style={{ padding: '5px', width: '300px' }}
                    />
                    <button onClick={handleSearch} style={{ marginLeft: '10px', padding: '5px 10px' }}>
                        Search
                    </button>
                </div>
                <div style={{ display: 'flex', gap: '20px' }}>
                    {/* Phần Swap (UXUY) */}
                    <div style={{ flex: 1 }}>
                        <h2>Swap</h2>
                        {tokens.length > 0 && (
                            <select onChange={(e) => setSelectedToken(tokens[e.target.value])}>
                                <option value="">Select Token</option>
                                {tokens.map((token, index) => (
                                    <option key={index} value={index}>
                                        {token.symbol} ({token.chain})
                                    </option>
                                ))}
                            </select>
                        )}
                        <input
                            type="number"
                            value={amount}
                            onChange={(e) => setAmount(e.target.value)}
                            placeholder="Amount"
                            style={{ marginTop: '10px', padding: '5px', width: '100%' }}
                        />
                        <div style={{ marginTop: '10px' }}>
                            <button onClick={handleBuy} style={{ marginRight: '10px' }}>Buy</button>
                            <button onClick={handleSell}>Sell</button>
                        </div>
                    </div>
                    {/* Phần Order Book (Binance) */}
                    <div style={{ flex: 1 }}>
                        <h2>Order Book</h2>
                        <div style={{ border: '1px solid #ccc', padding: '10px' }}>
                            <p>Buy Orders: (Mock data)</p>
                            <p>Sell Orders: (Mock data)</p>
                        </div>
                        <button onClick={() => setShowChart(true)} style={{ marginTop: '10px' }}>
                            Show Chart
                        </button>
                    </div>
                </div>
                {/* Chart Popup */}
                {showChart && (
                    <div style={{
                        position: 'fixed', top: '20%', left: '20%', width: '60%', height: '60%',
                        backgroundColor: 'white', border: '1px solid #ccc', zIndex: 1000, padding: '20px'
                    }}>
                        <h3>Chart</h3>
                        <p>(Mock Chart)</p>
                        <button onClick={() => setShowChart(false)}>Close</button>
                    </div>
                )}
            </main>
            <Footer />
        </div>
    );
};

export default ExchangePage;