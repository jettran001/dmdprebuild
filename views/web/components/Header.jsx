// views/web/components/Header.jsx
import React, { useState, useEffect } from 'react';
import axios from 'axios';
import { Link } from 'react-router-dom';
import NavMenu from './NavMenu';

const Header = ({ telegramId }) => {
    const [balance, setBalance] = useState(0);

    useEffect(() => {
        axios.get(`/api/walletBalance/${telegramId}/eth`)
            .then(res => setBalance(res.data.balance))
            .catch(err => console.error(err));
    }, [telegramId]);

    return (
        <header style={{ display: 'flex', justifyContent: 'space-between', padding: '10px' }}>
            <nav>
            <div style={{ display: 'flex', alignItems: 'center' }}>
                <img src="/logo.png" alt="Logo" style={{ height: '40px' }} />
                <NavMenu />
            </div>
                <Link to="/" style={{ marginRight: '20px', color: 'white' }}>Home</Link>
                <Link to="/wallet" style={{ marginRight: '20px', color: 'white' }}>Wallet</Link>
                <Link to="/exchange" style={{ marginRight: '20px', color: 'white' }}>Exchange</Link>
                <Link to="/snipebot" style={{ marginRight: '20px', color: 'white' }}>Snipebot</Link>
                <Link to="/earn" style={{ marginRight: '20px', color: 'white' }}>Earn</Link>
                <Link to="/mission" style={{ marginRight: '20px', color: 'white' }}>Mission</Link>
                <Link to="/search" style={{ marginRight: '20px', color: 'white' }}>Search</Link>
                <Link to="/trade-history" style={{ marginRight: '20px', color: 'white' }}>Trade History</Link>
                <Link to="/settings" style={{ marginRight: '20px', color: 'white' }}>Settings</Link>
                <Link to="/farm" style={{ marginRight: '20px', color: 'white' }}>Farm</Link>
                <Link to="/node-key" style={{ marginRight: '20px', color: 'white' }}>Node Key</Link>
                <span>Telegram ID: {telegramId}</span>
                <div style={{ display: 'flex', alignItems: 'center' }}>
                <span style={{ marginRight: '10px' }}>{balance} ETH</span>
                <button onClick={() => alert('Connect Wallet')}>Connect</button>
            </div>
            </nav>
               
        </header>
    );
};

export default Header;

