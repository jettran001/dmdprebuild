// admin/Dashboard.jsx
import React, { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import Sidebar from './components/Sidebar';
import Home from './pages/Home';
import Depin_adminManager from './pages/Depin_adminManager';
import Pools_adminManager from './pages/Pools_adminManager';
import Promo from './pages/Promo';
import Bot_adminManager from './pages/Bot_adminManager';
import Node_keyManager from './pages/Node_keyManager';

const Dashboard = () => {
    const navigate = useNavigate();
    const [currentPage, setCurrentPage] = useState('Home');

    useEffect(() => {
        const token = localStorage.getItem('adminToken');
        if (!token) {
            navigate('/admin/login');
        }
    }, [navigate]);

    const renderPage = () => {
        switch (currentPage) {
            case 'Home':
                return <Home />;
            case 'Depin_adminManager':
                return <Depin_adminManager />;
            case 'Pools_adminManager':
                return <Pools_adminManager />;
            case 'Promo':
                return <Promo />;
            case 'Bot_adminManager':
                return <Bot_adminManager />;
            case 'Node_keyManager':
                return <Node_keyManager />;
            default:
                return <Home />;
        }
    };

    return (
        <div style={{ display: 'flex' }}>
            <Sidebar setCurrentPage={setCurrentPage} />
            <div style={{ flex: 1, padding: '20px' }}>
                {renderPage()}
            </div>
        </div>
    );
};

export default Dashboard;