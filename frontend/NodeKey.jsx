import React, { useState, useEffect } from 'react';
import { Container, Table, Button, Form } from 'react-bootstrap';
import { apiRequest } from './utils/api';

const NodeKey = () => {
    const [keys, setKeys] = useState([]);
    const [hwid, setHwid] = useState('');
    const [status, setStatus] = useState('');

    useEffect(() => {
        apiRequest('http://localhost:3000/api/node-keys', 'POST')
            .then(data => setKeys(data))
            .catch(err => setStatus(err.error));
    }, []);

    const generateKey = async () => {
        const data = await apiRequest('http://localhost:3000/api/generate-key', 'POST', { hwid });
        setStatus(data.status || data.error);
        setKeys(prev => [...prev, { hwid, key: data.key }]);
    };

    const deleteKey = async (hwid) => {
        const data = await apiRequest('http://localhost:3000/api/delete-key', 'POST', { hwid });
        setStatus(data.status || data.error);
        setKeys(prev => prev.filter(k => k.hwid !== hwid));
    };

    return (
        <Container className="mt-4">
            <h3>Node Key Management</h3>
            <Form inline className="mb-3">
                <Form.Control value={hwid} onChange={e => setHwid(e.target.value)} placeholder="Node HWID" />
                <Button className="ml-2" onClick={generateKey}>Generate Key</Button>
            </Form>
            <Table striped bordered hover>
                <thead><tr><th>HWID</th><th>Key</th><th>Action</th></tr></thead>
                <tbody>
                    {keys.map(k => (
                        <tr key={k.hwid}>
                            <td>{k.hwid}</td>
                            <td>{k.key}</td>
                            <td><Button variant="danger" onClick={() => deleteKey(k.hwid)}>Delete</Button></td>
                        </tr>
                    ))}
                </tbody>
            </Table>
            {status && <p>{status}</p>}
        </Container>
    );
};

export default NodeKey;