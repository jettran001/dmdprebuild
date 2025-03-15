import React, { useState } from 'react';
import { Container, Form, Button } from 'react-bootstrap';
import { apiRequest } from './utils/api';

const Staking = () => {
    const [amount, setAmount] = useState('');
    const [status, setStatus] = useState('');

    const stake = async () => {
        const data = await apiRequest('http://localhost:3000/api/stake', 'POST', {
            wallet: '0xTestWallet',
            amount
        });
        setStatus(data.status || data.error);
    };

    return (
        <Container className="mt-4">
            <h3>Staking</h3>
            <Form>
                <Form.Control value={amount} onChange={e => setAmount(e.target.value)} placeholder="Amount to stake" />
                <Button className="mt-2" onClick={stake}>Stake</Button>
            </Form>
            {status && <p>{status}</p>}
        </Container>
    );
};

export default Staking;