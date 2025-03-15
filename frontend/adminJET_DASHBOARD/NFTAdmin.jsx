import React, { useState } from 'react';
import { Container, Form, Button } from 'react-bootstrap';
import { apiRequest } from '../utils/api';

const NFTAdmin = () => {
    const [uri, setUri] = useState('');
    const [status, setStatus] = useState('');

    const mint = async () => {
        const data = await apiRequest('http://localhost:3000/api/mint-nft', 'POST', { uri });
        setStatus(data.status || data.error);
    };

    return (
        <Container className="mt-4">
            <h3>NFT Admin</h3>
            <Form>
                <Form.Control value={uri} onChange={e => setUri(e.target.value)} placeholder="NFT URI" />
                <Button className="mt-2" onClick={mint}>Mint NFT</Button>
            </Form>
            {status && <p>{status}</p>}
        </Container>
    );
};

export default NFTAdmin;