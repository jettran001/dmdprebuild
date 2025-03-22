import React, { useEffect, useState } from 'react';
import axios from 'axios';

const Dashboard = () => {
    const [profile, setProfile] = useState(null);
    const [profitLoss, setProfitLoss] = useState({ profit: 0, loss: 0 });

    useEffect(() => {
        const fetchData = async () => {
            const profileRes = await axios.get('/api/users/profile', {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` }
            });
            const plRes = await axios.get('/api/snipebot/bsc/profitloss', {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` }
            });
            setProfile(profileRes.data);
            setProfitLoss(plRes.data);
        };
        fetchData();
    }, []);

    return profile ? (
        <div className="dashboard">
            <h2>Dashboard</h2>
            <p>Welcome, {profile.username}</p>
            <p>Profit: {profitLoss.profit} | Loss: {profitLoss.loss}</p>
        </div>
    ) : <p>Loading...</p>;
};

export default Dashboard;