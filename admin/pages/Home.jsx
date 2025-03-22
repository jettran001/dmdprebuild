// admin/pages/Home.jsx
import React, { useState, useEffect } from 'react';
import axios from 'axios';
import Chart from '../components/Chart';
import Heatmap from '../components/Heatmap';
import TokenLogsTable from '../components/TokenLogsTable';

const Home = () => {
    const [data, setData] = useState({
        ccu: { day: {}, week: {} },
        nodes: { day: 0, week: 0 },
        heatmap: [],
        bots: { active: 0, total: 0 },
        chains: { active: 0, total: 0 },
        tokenLogs: []
    });

    useEffect(() => {
        const fetchData = async () => {
            try {
                const response = await axios.get('/api/admin/dashboard', {
                    headers: { Authorization: `Bearer ${localStorage.getItem('adminToken')}` }
                });
                setData(response.data);
            } catch (error) {
                console.error('Error fetching dashboard data:', error);
            }
        };
        fetchData();
    }, []);

    return (
        <div>
            <h1>Dashboard - Home</h1>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: '20px' }}>
                {/* CCU */}
                <div style={{ width: '300px' }}>
                    <h3>Concurrent Users (CCU)</h3>
                    <Chart
                        title="CCU (1 Day)"
                        data={[
                            { name: 'PC', value: data.ccu.day.pc || 0 },
                            { name: 'Mobile', value: data.ccu.day.mobile || 0 },
                            { name: 'Extension', value: data.ccu.day.extension || 0 }
                        ]}
                    />
                    <Chart
                        title="CCU (7 Days)"
                        data={[
                            { name: 'PC', value: data.ccu.week.pc || 0 },
                            { name: 'Mobile', value: data.ccu.week.mobile || 0 },
                            { name: 'Extension', value: data.ccu.week.extension || 0 }
                        ]}
                    />
                </div>

                {/* Nodes Active */}
                <div style={{ width: '300px' }}>
                    <h3>Nodes Active</h3>
                    <p>1 Day: {data.nodes.day}</p>
                    <p>7 Days: {data.nodes.week}</p>
                </div>

                {/* Heatmap */}
                <div style={{ width: '300px' }}>
                    <h3>Country Heatmap</h3>
                    <Heatmap data={data.heatmap} />
                </div>

                {/* Bots Active */}
                <div style={{ width: '300px' }}>
                    <h3>Bots Active</h3>
                    <p>Active: {data.bots.active}/{data.bots.total}</p>
                </div>

                {/* Chains Active */}
                <div style={{ width: '300px' }}>
                    <h3>Chains Active</h3>
                    <p>Active: {data.chains.active}/{data.chains.total} ({(data.chains.active / data.chains.total * 100).toFixed(2)}%)</p>
                </div>

                {/* Token Logs */}
                <div style={{ width: '100%' }}>
                    <h3>Top Purchased Tokens</h3>
                    <TokenLogsTable tokenLogs={data.tokenLogs} />
                </div>
            </div>
        </div>
    );
};

export default Home;