import React, { useState } from 'react';
import { Container, Form, Button } from 'react-bootstrap';
import { apiRequest } from './utils/api';

const Snipebot = () => {
    const [token, setToken] = useState('');
    const [amount, setAmount] = useState('');
    const [chain, setChain] = useState('BSC');
    const [status, setStatus] = useState('');

    const snipe = async () => {
        const data = await apiRequest('http://localhost:3000/api/snipebot/snipe', 'POST', {
            wallet: '0xTestWallet',
            token,
            amount,
            chain
        });
        setStatus(data.status || data.error);
    };

    return (
        <Container className="mt-4">
            <h3>Snipebot</h3>
            <Form>
                <Form.Control value={token} onChange={e => setToken(e.target.value)} placeholder="Token address" />
                <Form.Control className="mt-2" value={amount} onChange={e => setAmount(e.target.value)} placeholder="Amount" />
                <Form.Control as="select" className="mt-2" value={chain} onChange={e => setChain(e.target.value)}>
                    <option>BSC</option>
                    <option>ETH</option>
                    <option>SOL</option>
                </Form.Control>
                <Button className="mt-2" onClick={snipe}>Snipe</Button>
            </Form>
            {status && <p>{status}</p>}
        </Container>
    );
};

export default Snipebot;