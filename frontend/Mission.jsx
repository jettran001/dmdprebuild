import React, { useState, useEffect } from 'react';
import { Container, Table, Button } from 'react-bootstrap';
import { apiRequest } from './utils/api';

const Mission = () => {
    const [missions, setMissions] = useState([]);
    const [status, setStatus] = useState('');

    useEffect(() => {
        apiRequest('http://localhost:3000/api/missions')
            .then(data => setMissions(data))
            .catch(err => setStatus(err.error));
    }, []);

    const completeMission = async (id) => {
        const data = await apiRequest('http://localhost:3000/api/complete-mission', 'POST', {
            missionId: id,
            wallet: '0xTestWallet'
        });
        setStatus(data.status || data.error);
        setMissions(prev => prev.map(m => m.id === id ? { ...m, completed: true } : m));
    };

    return (
        <Container className="mt-4">
            <h3>Missions</h3>
            <Table striped bordered hover>
                <thead><tr><th>ID</th><th>Description</th><th>Reward (DMD)</th><th>Action</th></tr></thead>
                <tbody>
                    {missions.map(m => (
                        <tr key={m.id}>
                            <td>{m.id}</td>
                            <td>{m.description}</td>
                            <td>{m.reward}</td>
                            <td>
                                {m.completed ? 'Completed' : <Button onClick={() => completeMission(m.id)}>Complete</Button>}
                            </td>
                        </tr>
                    ))}
                </tbody>
            </Table>
            {status && <p>{status}</p>}
        </Container>
    );
};

export default Mission;