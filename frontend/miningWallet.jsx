import React, { useState, useEffect } from 'react';
import { Container, Form, Button } from 'react-bootstrap';
import { apiRequest } from './utils/api';

const miningWallet = () => {
    const [walletData, setWalletData] = useState({ balance: 0, lastMined: '' });
    const [amount, setAmount] = useState('');
    const [status, setStatus] = useState('');

    useEffect(() => {
        apiRequest('http://localhost:3000/api/mining-wallet', 'POST', { wallet: '0xTestWallet' })
            .then(data => setWalletData(data))
            .catch(err => setStatus(err.error));
    }, []);

    const withdraw = async () => {
        const data = await apiRequest('http://localhost:3000/api/withdraw-mining', 'POST', {
            wallet: '0xTestWallet',
            amount
        });
        setStatus(data.status || data.error);
        setWalletData(prev => ({ ...prev, balance: prev.balance - amount }));
    };

    return (
        <Container className="mt-4">
            <h3>Mining Wallet</h3>
            <p>Balance: {walletData.balance} DMD</p>
            <p>Last Mined: {walletData.lastMined}</p>
            <Form>
                <Form.Control type="number" value={amount} onChange={e => setAmount(e.target.value)} placeholder="Amount to withdraw" />
                <Button className="mt-2" onClick={withdraw}>Withdraw</Button>
            </Form>
            {status && <p>{status}</p>}
        </Container>
    );
};

export default miningWallet;