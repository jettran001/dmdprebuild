// views/web/pages/TradeHistoryPage.jsx
import React, { useState, useEffect } from 'react';
import axios from 'axios';
import Header from '../components/Header';
import Footer from '../components/Footer';

const TradeHistoryPage = () => {
    const userId = 'user123';
    const [pnlData, setPnlData] = useState({ '24h': 0, '1d': 0, '7d': 0 });
    const [selectedPeriod, setSelectedPeriod] = useState('24h');
    const [trades, setTrades] = useState([]);

    useEffect(() => {
        const fetchPNL = async () => {
            try {
                const periods = ['24h', '1d', '7d'];
                const results = {};
                for (const period of periods) {
                    const response = await axios.get(`/api/getPNL?userId=${userId}&period=${period}`);
                    results[period] = response.data.totalPNL;
                    if (period === selectedPeriod) {
                        setTrades(response.data.trades);
                    }
                }
                setPnlData(results);
            } catch (error) {
                console.error(error);
            }
        };
        fetchPNL();
    }, [selectedPeriod]);

    return (
        <div>
            <Header telegramId={userId} />
            <main style={{ padding: '20px' }}>
                <h1>Trade History</h1>
                <div style={{ marginBottom: '20px' }}>
                    <h2>PNL</h2>
                    <p>24h: {pnlData['24h']} ETH</p>
                    <p>1d: {pnlData['1d']} ETH</p>
                    <p>7d: {pnlData['7d']} ETH</p>
                </div>
                <div>
                    <select onChange={(e) => setSelectedPeriod(e.target.value)} value={selectedPeriod}>
                        <option value="24h">24h</option>
                        <option value="1d">1d</option>
                        <option value="7d">7d</option>
                    </select>
                    <h2>Trades</h2>
                    <ul style={{ listStyle: 'none', padding: 0 }}>
                        {trades.map((trade, index) => (
                            <li key={index} style={{ padding: '10px', borderBottom: '1px solid #ccc' }}>
                                {trade.mode} - {trade.tokenAddress} - {trade.buyAmount} @ {trade.buyPrice} ETH
                            </li>
                        ))}
                    </ul>
                </div>
            </main>
            <Footer />
        </div>
    );
};

export default TradeHistoryPage;