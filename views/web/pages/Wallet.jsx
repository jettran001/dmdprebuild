import React, { useEffect, useState } from 'react';
import axios from 'axios';
import WalletConnect from '../components/WalletConnect';

const Wallet = () => {
    const [wallets, setWallets] = useState([]);

    useEffect(() => {
        const fetchWallets = async () => {
            const res = await axios.get('/api/wallets', {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` }
            });
            setWallets(res.data);
        };
        fetchWallets();
    }, []);

    return (
        <div className="wallet">
            <h2>Wallet</h2>
            <WalletConnect />
            {wallets.map(wallet => (
                <div key={wallet._id}>
                    <p>Address: {wallet.address}</p>
                    <p>Balance: {wallet.balance}</p>
                </div>
            ))}
        </div>
    );
};

export default Wallet;