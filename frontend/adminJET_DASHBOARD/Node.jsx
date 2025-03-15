import React, { useState, useEffect } from 'react';
import { Container, Table, Button } from 'react-bootstrap';
import { apiRequest } from '../utils/api';

const Node = () => {
    const [nodes, setNodes] = useState([]);

    useEffect(() => {
        apiRequest('http://localhost:3000/api/nodes')
            .then(data => setNodes(data))
            .catch(err => console.error(err));
    }, []);

    const delist = async (hwid) => {
        await apiRequest('http://localhost:3000/api/delist', 'POST', { hwid });
        setNodes(prev => prev.filter(n => n.hwid !== hwid));
    };

    return (
        <Container className="mt-4">
            <h3>Node Management</h3>
            <Table striped bordered hover>
                <thead><tr><th>HWID</th><th>Bandwidth</th><th>Price</th><th>Action</th></tr></thead>
                <tbody>
                    {nodes.map(n => (
                        <tr key={n.hwid}>
                            <td>{n.hwid}</td>
                            <td>{n.available_bandwidth}</td>
                            <td>{n.price}</td>
                            <td><Button variant="danger" onClick={() => delist(n.hwid)}>Delist</Button></td>
                        </tr>
                    ))}
                </tbody>
            </Table>
        </Container>
    );
};

export default Node;