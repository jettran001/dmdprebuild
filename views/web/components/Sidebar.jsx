import React from 'react';
import { Link } from 'react-router-dom';

const Sidebar = () => (
    <aside className="sidebar">
        <ul>
            <li><Link to="/">Home</Link></li>
            <li><Link to="/snipebot">Snipebot</Link></li>
            <li><Link to="/farming">Farming</Link></li>
            <li><Link to="/tokens">Tokens</Link></li>
            <li><Link to="/transactions">Transactions</Link></li>
            <li><Link to="/wallet">Wallet</Link></li>
            <li><Link to="/dashboard">Dashboard</Link></li>
            <li><Link to="/settings">Settings</Link></li>
        </ul>
    </aside>
);

export default Sidebar;