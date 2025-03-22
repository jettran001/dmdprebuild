// admin/pages/Depin_adminManager.jsx
import React, { useState, useEffect } from 'react';
import axios from 'axios';

const Depin_adminManager = () => {
    const [blocksMined, setBlocksMined] = useState(0);
    const [nodes, setNodes] = useState([]);
    const [currentPage, setCurrentPage] = useState(1);
    const [selectedNode, setSelectedNode] = useState(null);
    const [proxyForm, setProxyForm] = useState({ user: '', password: '' });
    const itemsPerPage = 5;

    useEffect(() => {
        const fetchData = async () => {
            try {
                const response = await axios.get('/api/admin/depin', {
                    headers: { Authorization: `Bearer ${localStorage.getItem('adminToken')}` }
                });
                setBlocksMined(response.data.blocksMined);
                setNodes(response.data.nodes);
            } catch (error) {
                console.error('Error fetching depin data:', error);
            }
        };
        fetchData();
    }, []);

    const indexOfLastItem = currentPage * itemsPerPage;
    const indexOfFirstItem = indexOfLastItem - itemsPerPage;
    const currentItems = nodes.slice(indexOfFirstItem, indexOfLastItem);
    const totalPages = Math.ceil(nodes.length / itemsPerPage);

    const handleSetProxy = async () => {
        if (!selectedNode) return;

        try {
            const response = await axios.post('/api/admin/depin/setProxy', {
                ip: selectedNode.ip,
                user: proxyForm.user,
                password: proxyForm.password
            }, {
                headers: { Authorization: `Bearer ${localStorage.getItem('adminToken')}` }
            });
            alert(response.data.status);
            setNodes(nodes.map(node => node.ip === selectedNode.ip ? { ...node, user: proxyForm.user, password: proxyForm.password } : node));
            setSelectedNode(null);
            setProxyForm({ user: '', password: '' });
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    return (
        <div>
            <h1>Depin Admin Manager</h1>
            <div>
                <h3>Blocks Mined</h3>
                <p>{blocksMined}</p>
            </div>
            <div>
                <h3>Nodes IP Management</h3>
                <table style={{ width: '100%', borderCollapse: 'collapse' }}>
                    <thead>
                        <tr>
                            <th style={{ border: '1px solid #ccc', padding: '8px' }}>IP</th>
                            <th style={{ border: '1px solid #ccc', padding: '8px' }}>User</th>
                            <th style={{ border: '1px solid #ccc', padding: '8px' }}>Password</th>
                            <th style={{ border: '1px solid #ccc', padding: '8px' }}>Action</th>
                        </tr>
                    </thead>
                    <tbody>
                        {currentItems.map((node, index) => (
                            <tr key={index}>
                                <td style={{ border: '1px solid #ccc', padding: '8px' }}>{node.ip}</td>
                                <td style={{ border: '1px solid #ccc', padding: '8px' }}>{node.user || '-'}</td>
                                <td style={{ border: '1px solid #ccc', padding: '8px' }}>{node.password || '-'}</td>
                                <td style={{ border: '1px solid #ccc', padding: '8px' }}>
                                    <button onClick={() => setSelectedNode(node)}>Set Proxy</button>
                                </td>
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
            {selectedNode && (
                <div style={{ marginTop: '20px', border: '1px solid #ccc', padding: '10px' }}>
                    <h3>Set Proxy for IP: {selectedNode.ip}</h3>
                    <input
                        type="text"
                        placeholder="Username"
                        value={proxyForm.user}
                        onChange={(e) => setProxyForm({ ...proxyForm, user: e.target.value })}
                        style={{ marginRight: '10px' }}
                    />
                    <input
                        type="password"
                        placeholder="Password"
                        value={proxyForm.password}
                        onChange={(e) => setProxyForm({ ...proxyForm, password: e.target.value })}
                        style={{ marginRight: '10px' }}
                    />
                    <button onClick={handleSetProxy}>Save</button>
                    <button onClick={() => setSelectedNode(null)} style={{ marginLeft: '10px' }}>Cancel</button>
                </div>
            )}
        </div>
    );
};

export default Depin_adminManager;