import React, { useState } from 'react';
import { Container, Form, Button } from 'react-bootstrap';
import { apiRequest } from './utils/api';

const Balance = () => {
    const [wallet, setWallet] = useState('');
    const [balance, setBalance] = useState(null);

    const checkBalance = async () => {
        const data = await apiRequest('http://localhost:3000/api/balance', 'POST', { wallet });
        setBalance(data.balance);
    };

    return (
        <Container className="mt-4">
            <h3>Check Balance</h3>
            <Form inline>
                <Form.Control value={wallet} onChange={e => setWallet(e.target.value)} placeholder="Enter wallet address" />
                <Button className="ml-2" onClick={checkBalance}>Check</Button>
            </Form>
            {balance && <p>Balance: {balance} DMD</p>}
        </Container>
    );
};

export default Balance;