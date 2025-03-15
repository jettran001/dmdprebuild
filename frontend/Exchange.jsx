import React, { useState } from 'react';
import { Container, Form, Button } from 'react-bootstrap';
import { apiRequest } from './utils/api';

const Exchange = () => {
    const [tokenFrom, setTokenFrom] = useState('');
    const [tokenTo, setTokenTo] = useState('');
    const [amount, setAmount] = useState('');
    const [status, setStatus] = useState('');

    const swap = async () => {
        const data = await apiRequest('http://localhost:3000/api/exchange/swap', 'POST', {
            wallet: '0xTestWallet',
            tokenFrom,
            tokenTo,
            amount
        });
        setStatus(data.status || data.error);
    };

    return (
        <Container className="mt-4">
            <h3>Exchange</h3>
            <Form>
                <Form.Control value={tokenFrom} onChange={e => setTokenFrom(e.target.value)} placeholder="From token" />
                <Form.Control className="mt-2" value={tokenTo} onChange={e => setTokenTo(e.target.value)} placeholder="To token" />
                <Form.Control className="mt-2" value={amount} onChange={e => setAmount(e.target.value)} placeholder="Amount" />
                <Button className="mt-2" onClick={swap}>Swap</Button>
            </Form>
            {status && <p>{status}</p>}
        </Container>
    );
};

export default Exchange;