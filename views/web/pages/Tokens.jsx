import React, { useState } from 'react';
import TokenCard from '../components/TokenCard';

const Tokens = () => {
    const [tokenAddress, setTokenAddress] = useState('');

    return (
        <div className="tokens">
            <h2>Tokens</h2>
            <input type="text" value={tokenAddress} onChange={(e) => setTokenAddress(e.target.value)} placeholder="Enter Token Address" />
            {tokenAddress && <TokenCard tokenAddress={tokenAddress} />}
        </div>
    );
};

export default Tokens;