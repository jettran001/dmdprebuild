import React, { useEffect, useState } from 'react';
import axios from 'axios';

const TokenCard = ({ tokenAddress }) => {
    const [token, setToken] = useState(null);

    useEffect(() => {
        const fetchToken = async () => {
            const res = await axios.get(`/api/tokens/${tokenAddress}`, {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` }
            });
            setToken(res.data);
        };
        fetchToken();
    }, [tokenAddress]);

    return token ? (
        <div className="token-card">
            <h3>{token.name}</h3>
            <p>Address: {token.address}</p>
            <p>Status: {token.status}</p>
        </div>
    ) : <p>Loading...</p>;
};

export default TokenCard;