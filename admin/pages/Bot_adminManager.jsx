// admin/pages/Bot_adminManager.jsx
import React, { useState, useEffect } from 'react';
import axios from 'axios';

const Bot_adminManager = () => {
    const [keys, setKeys] = useState([]);
    const [currentPage, setCurrentPage] = useState(1);
    const [singleKeyForm, setSingleKeyForm] = useState({ inviterId: '' });
    const [teamKeyForm, setTeamKeyForm] = useState({ inviterId: '', additionalKeys: 0 });
    const itemsPerPage = 5;

    useEffect(() => {
        const fetchKeys = async () => {
            try {
                const response = await axios.get('/api/admin/bot/logKeys', {
                    headers: { Authorization: `Bearer ${localStorage.getItem('adminToken')}` }
                });
                setKeys(response.data);
            } catch (error) {
                console.error('Error fetching keys:', error);
            }
        };
        fetchKeys();
    }, []);

    const indexOfLastItem = currentPage * itemsPerPage;
    const indexOfFirstItem = indexOfLastItem - itemsPerPage;
    const currentItems = keys.slice(indexOfFirstItem, indexOfLastItem);
    const totalPages = Math.ceil(keys.length / itemsPerPage);

    const handleCreateSingleKey = async () => {
        try {
            const response = await axios.post('/api/admin/bot/createKey', {
                type: 'single',
                inviterId: singleKeyForm.inviterId || null
            }, {
                headers: { Authorization: `Bearer ${localStorage.getItem('adminToken')}` }
            });
            alert(`Key created: ${response.data.keys[0].key}`);
            setKeys([...keys, ...response.data.keys]);
            setSingleKeyForm({ inviterId: '' });
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    const handleCreateTeamKey = async () => {
        try {
            const response = await axios.post('/api/admin/bot/createKey', {
                type: 'team',
                inviterId: teamKeyForm.inviterId || null,
                additionalKeys: parseInt(teamKeyForm.additionalKeys) || 0
            }, {
                headers: { Authorization: `Bearer ${localStorage.getItem('adminToken')}` }
            });
            alert(`Keys created: ${response.data.keys.map(k => k.key).join(', ')}`);
            setKeys([...keys, ...response.data.keys]);
            setTeamKeyForm({ inviterId: '', additionalKeys: 0 });
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    return (
        <div>
            <h1>Bot Admin Manager</h1>
            <div style={{ display: 'flex', gap: '20px' }}>
                {/* Card Single Key */}
                <div style={{ border: '1px solid #ccc', padding: '20px', width: '300px' }}>
                    <h3>Create Single Key</h3>
                    <input
                        type="text"
                        placeholder="Inviter ID (optional)"
                        value={singleKeyForm.inviterId}
                        onChange={(e) => setSingleKeyForm({ inviterId: e.target.value })}
                        style={{ width: '100%', marginBottom: '10px' }}
                    />
                    <button onClick={handleCreateSingleKey}>Create</button>
                </div>

                {/* Card Team Key */}
                <div style={{ border: '1px solid #ccc', padding: '20px', width: '300px' }}>
                    <h3>Create Team Key</h3>
                    <input
                        type="text"
                        placeholder="Inviter ID (optional)"
                        value={teamKeyForm.inviterId}
                        onChange={(e) => setTeamKeyForm({ ...teamKeyForm, inviterId: e.target.value })}
                        style={{ width: '100%', marginBottom: '10px' }}
                    />
                    <input
                        type="number"
                        placeholder="Additional Keys (default: 4)"
                        value={teamKeyForm.additionalKeys}
                        onChange={(e) => setTeamKeyForm({ ...teamKeyForm, additionalKeys: e.target.value })}
                        style={{ width: '100%', marginBottom: '10px' }}
                    />
                    <button onClick={handleCreateTeamKey}>Create</button>
                </div>
            </div>

            {/* Log_Keys */}
            <div style={{ marginTop: '20px' }}>
                <h3>Log_Keys</h3>
                <table style={{ width: '100%', borderCollapse: 'collapse' }}>
                    <thead>
                        <tr>
                            <th style={{ border: '1px solid #ccc', padding: '8px' }}>Key</th>
                            <th style={{ border: '1px solid #ccc', padding: '8px' }}>Type</th>
                            <th style={{ border: '1px solid #ccc', padding: '8px' }}>Inviter ID</th>
                            <th style={{ border: '1px solid #ccc', padding: '8px' }}>Created At</th>
                            <th style={{ border: '1px solid #ccc', padding: '8px' }}>Bot Access</th>
                        </tr>
                    </thead>
                    <tbody>
                        {currentItems.map((key, index) => (
                            <tr key={index}>
                                <td style={{ border: '1px solid #ccc', padding: '8px' }}>{key.key}</td>
                                <td style={{ border: '1px solid #ccc', padding: '8px' }}>{key.type}</td>
                                <td style={{ border: '1px solid #ccc', padding: '8px' }}>{key.inviterId || '-'}</td>
                                <td style={{ border: '1px solid #ccc', padding: '8px' }}>{new Date(key.createdAt).toLocaleString()}</td>
                                <td style={{ border: '1px solid #ccc', padding: '8px' }}>{key.type === 'single' ? 'Premium' : key.type === 'inviter' || key.type === 'member' ? 'Ultimate' : 'Basic'}</td>
                            </tr>
                        ))}
                    </tbody>
                </table>
                <div style={{ marginTop: '10px' }}>
                    <button
                        onClick={() => setCurrentPage(page => Math.max(page - 1, 1))}
                        disabled={currentPage === 1}
                    >
                        Previous
                    </button>
                    <span style={{ margin: '0 10px' }}>Page {currentPage} of {totalPages}</span>
                    <button
                        onClick={() => setCurrentPage(page => Math.min(page + 1, totalPages))}
                        disabled={currentPage === totalPages}
                    >
                        Next
                    </button>
                </div>
            </div>
        </div>
    );
};

export default Bot_adminManager;