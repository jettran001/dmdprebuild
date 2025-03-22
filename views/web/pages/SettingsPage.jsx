// views/web/pages/SettingsPage.jsx
import React, { useState } from 'react';
import axios from 'axios';
import Header from '../components/Header';
import Footer from '../components/Footer';

const SettingsPage = () => {
    const telegramId = 'user123';
    const [chains, setChains] = useState([]);
    const [balances, setBalances] = useState({});
    const [filter, setFilter] = useState('');

    const availableChains = ['eth', 'bsc', 'sol'];

    const handleFetchBalances = async () => {
        try {
            const response = await axios.post('/api/getMultiChainBalance', { telegramId, chains });
            setBalances(response.data);
        } catch (error) {
            console.error(error);
        }
    };

    return (
        <div>
            <Header telegramId={telegramId} />
            <main style={{ padding: '20px' }}>
                <h1>Settings</h1>
                <div style={{ marginBottom: '20px' }}>
                    <h2>Select Chains</h2>
                    {availableChains.map(chain => (
                        <label key={chain} style={{ marginRight: '10px' }}>
                            <input
                                type="checkbox"
                                checked={chains.includes(chain)}
                                onChange={() => {
                                    if (chains.includes(chain)) {
                                        setChains(chains.filter(c => c !== chain));
                                    } else {
                                        setChains([...chains, chain]);
                                    }
                                }}
                            />
                            {chain.toUpperCase()}
                        </label>
                    ))}
                    <button onClick={handleFetchBalances} style={{ marginLeft: '10px' }}>
                        Fetch Balances
                    </button>
                </div>
                <div style={{ marginBottom: '20px' }}>
                    <h2>Filter Tokens</h2>
                    <input
                        type="text"
                        value={filter}
                        onChange={(e) => setFilter(e.target.value)}
                        placeholder="Filter by symbol..."
                        style={{ padding: '5px', width: '200px' }}
                    />
                </div>
                <div>
                    <h2>Balances</h2>
                    {Object.keys(balances).map(chain => (
                        <div key={chain} style={{ marginBottom: '20px' }}>
                            <h3>{chain.toUpperCase()}</h3>
                            <p>Native Balance: {balances[chain].native} ETH</p>
                            <h4>Tokens</h4>
                            <ul style={{ listStyle: 'none', padding: 0 }}>
                                {balances[chain].tokens
                                    .filter(token => token.symbol.toLowerCase().includes(filter.toLowerCase()))
                                    .map((token, index) => (
                                        <li key={index}>
                                            {token.symbol}: {token.balance} (Value: {token.balance * token.price} USD)
                                        </li>
                                    ))}
                            </ul>
                        </div>
                    ))}
                </div>
            </main>
            <Footer />
        </div>
    );
};

export default SettingsPage;