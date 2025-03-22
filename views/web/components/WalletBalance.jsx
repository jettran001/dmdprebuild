// views/web/components/WalletBalance.jsx
import React, { useState, useEffect } from 'react';
import axios from 'axios';

const WalletBalance = ({ telegramId, chain }) => {
    const [balance, setBalance] = useState(0);

    useEffect(() => {
        axios.get(`/api/walletBalance/${telegramId}/${chain}`)
            .then(res => setBalance(res.data.balance))
            .catch(err => console.error(err));
    }, [telegramId, chain]);

    return (
        <div>
            <h3>Wallet Balance ({chain.toUpperCase()})</h3>
            <p>{balance} ETH</p>
        </div>
    );
};

export default WalletBalance;