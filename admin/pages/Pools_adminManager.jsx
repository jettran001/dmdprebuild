// admin/pages/Pools_adminManager.jsx
import React, { useState, useEffect } from 'react';
import axios from 'axios';

const Pools_adminManager = () => {
    const [pools, setPools] = useState([]);
    const [transferForm, setTransferForm] = useState({ poolName: '', amount: '', action: '' });

    useEffect(() => {
        const fetchData = async () => {
            try {
                const response = await axios.get('/api/admin/pools', {
                    headers: { Authorization: `Bearer ${localStorage.getItem('adminToken')}` }
                });
                setPools(response.data);
            } catch (error) {
                console.error('Error fetching pools data:', error);
            }
        };
        fetchData();
    }, []);

    const handleTransfer = async () => {
        if (!transferForm.poolName || !transferForm.amount || !transferForm.action) return;

        try {
            const endpoint = transferForm.action === 'reserve' ? '/api/admin/pools/reserve' : '/api/admin/pools/deposit';
            const response = await axios.post(endpoint, {
                poolName: transferForm.poolName,
                amount: parseFloat(transferForm.amount)
            }, {
                headers: { Authorization: `Bearer ${localStorage.getItem('adminToken')}` }
            });
            alert(response.data.status);
            setTransferForm({ poolName: '', amount: '', action: '' });
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    return (
        <div>
            <h1>Pools Admin Manager</h1>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: '20px' }}>
                {pools.map(pool => (
                    <div key={pool.name} style={{ border: '1px solid #ccc', padding: '20px', width: '300px' }}>
                        <h3>{pool.name}</h3>
                        <p>Total Tokens in Pool: {pool.totalTokens}</p>
                        <p>Tokens in Reserve: {pool.reserve}</p>
                        <button
                            onClick={() => setTransferForm({ poolName: pool.name, amount: '', action: 'reserve' })}
                            style={{ marginRight: '10px' }}
                        >
                            Reserve
                        </button>
                        <button
                            onClick={() => setTransferForm({ poolName: pool.name, amount: '', action: 'deposit' })}
                        >
                            Deposit
                        </button>
                    </div>
                ))}
            </div>
            {transferForm.poolName && (
                <div style={{ marginTop: '20px', border: '1px solid #ccc', padding: '10px' }}>
                    <h3>{transferForm.action === 'reserve' ? 'Reserve Tokens' : 'Deposit Tokens'} for {transferForm.poolName}</h3>
                    <input
                        type="number"
                        placeholder="Amount"
                        value={transferForm.amount}
                        onChange={(e) => setTransferForm({ ...transferForm, amount: e.target.value })}
                        style={{ marginRight: '10px' }}
                    />
                    <button onClick={handleTransfer}>Confirm</button>
                    <button onClick={() => setTransferForm({ poolName: '', amount: '', action: '' })} style={{ marginLeft: '10px' }}>
                        Cancel
                    </button>
                </div>
            )}
        </div>
    );
};

export default Pools_adminManager;