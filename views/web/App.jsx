import React from 'react';
import { BrowserRouter as Router, Route, Routes } from 'react-router-dom';
import WalletPage from './pages/WalletPage';
import HomePage from './pages/HomePage';
import ExchangePage from './pages/ExchangePage';
import SnipebotPage from './pages/SnipebotPage';
import EarnPage from './pages/EarnPage';
import MissionPage from './pages/MissionPage';
import SearchPage from './pages/SearchPage';
import TradeHistoryPage from './pages/TradeHistoryPage';
import SettingsPage from './pages/SettingsPage';
import FarmPage from './pages/FarmPage';
import Node_key from './pages/Node_key'; // Thêm Node_key


const App = () => {
    return (
        <Router>
            <Routes>
                <Route path="/" element={<HomePage />} />
                <Route path="/wallet" element={<WalletPage />} />
                <Route path="/exchange" element={<ExchangePage />} />
                <Route path="/snipebot" element={<SnipebotPage />} />
                <Route path="/earn" element={<EarnPage />} />
                <Route path="/mission" element={<MissionPage />} />
                <Route path="/search" element={<SearchPage />} />
                <Route path="/trade-history" element={<TradeHistoryPage />} />
                <Route path="/settings" element={<SettingsPage />} />
                <Route path="/farm" element={<FarmPage />} />
                <Route path="/node-key" element={<Node_key />} />
            </Routes>
        </Router>
    );
};

export default App;