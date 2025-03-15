import React, { useState, useEffect } from 'react';
import { Container, Table } from 'react-bootstrap';
import { apiRequest } from '../utils/api';

const UserAdmin = () => {
    const [users, setUsers] = useState([]);

    useEffect(() => {
        apiRequest('http://localhost:3000/api/users')
            .then(data => setUsers(data))
            .catch(err => console.error(err));
    }, []);

    return (
        <Container className="mt-4">
            <h3>User Admin</h3>
            <Table striped bordered hover>
                <thead><tr><th>Wallet</th><th>Balance</th></tr></thead>
                <tbody>
                    {users.map(u => (
                        <tr key={u.wallet}>
                            <td>{u.wallet}</td>
                            <td>{u.balance}</td>
                        </tr>
                    ))}
                </tbody>
            </Table>
        </Container>
    );
};

export default UserAdmin;