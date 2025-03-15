import React, { useState, useEffect } from 'react';
import { Container, Table } from 'react-bootstrap';
import { apiRequest } from './utils/api';

const Proxy = () => {
    const [proxies, setProxies] = useState([]);

    useEffect(() => {
        apiRequest('http://localhost:3000/api/proxies')
            .then(data => setProxies(data))
            .catch(err => console.error(err));
    }, []);

    return (
        <Container className="mt-4">
            <h3>Proxies</h3>
            <Table striped bordered hover>
                <thead><tr><th>HWID</th><th>IP</th><th>Country</th></tr></thead>
                <tbody>
                    {proxies.map(p => (
                        <tr key={p.hwid}>
                            <td>{p.hwid}</td>
                            <td>{p.ip}</td>
                            <td>{p.country}</td>
                        </tr>
                    ))}
                </tbody>
            </Table>
        </Container>
    );
};

export default Proxy;