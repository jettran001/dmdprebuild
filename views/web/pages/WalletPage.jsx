import React from 'react';
import Header from '../components/Header';
import Footer from '../components/Footer';
import WalletBalance from '../components/WalletBalance';

const WalletPage = () => {
    const telegramId = 'user123'; // Giả lập Telegram ID
    return (
        <div>
            <Header telegramId={telegramId} />
            <main style={{ padding: '20px' }}>
                <h1>Wallet</h1>
                <WalletBalance telegramId={telegramId} chain="eth" />
                <h2>Transaction History</h2>
                {/* Lịch sử giao dịch sẽ được thêm sau */}
            </main>
            <Footer />
        </div>
    );
};

export default WalletPage;