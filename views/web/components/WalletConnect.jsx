import React, { useState } from 'react';

const WalletConnect = () => {
    const [connected, setConnected] = useState(false);

    const connectWallet = () => {
        // TODO: Tích hợp Web3Modal/MetaMask
        setConnected(true);
    };

    return (
        <div className="wallet-connect">
            <button onClick={connectWallet}>{connected ? 'Connected' : 'Connect Wallet'}</button>
        </div>
    );
};

export default WalletConnect;