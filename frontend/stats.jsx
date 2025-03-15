import React, { useState, useEffect } from 'react';
import { Container } from 'react-bootstrap';
import { apiRequest } from './utils/api';

const stats = () => {
    const [statsData, setStatsData] = useState({ nodes: 0, nfts: 0, users: 0 });

    useEffect(() => {
        apiRequest('http://localhost:3000/api/stats')
            .then(data => setStatsData(data))
            .catch(err => console.error(err));
    }, []);

    return (
        <Container className="mt-4">
            <h3>System Stats</h3>
            <p>Total Active Nodes: {statsData.nodes}</p>
            <p>Total NFTs: {statsData.nfts}</p>
            <p>Total Users: {statsData.users}</p>
        </Container>
    );
};

export default stats;