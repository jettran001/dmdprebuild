import React, { useState, useEffect, Suspense, lazy } from 'react';
import { Navbar, Form, Button, Dropdown } from 'react-bootstrap';
import { apiRequest } from './utils/api';
const NFTCard = lazy(() => import('./NFTCard'));

const NFTMarket = () => {
    const [search, setSearch] = useState('');
    const [nfts, setNfts] = useState([]);
    const [filterChain, setFilterChain] = useState('All');
    const [status, setStatus] = useState('');

    useEffect(() => {
        apiRequest('http://localhost:3000/api/nfts')
            .then(data => setNfts(data))
            .catch(err => setStatus(err.error));
        const ws = new WebSocket('ws://localhost:8080');
        ws.onmessage = (msg) => {
            const data = JSON.parse(msg.data);
            if (data.type === 'nft_update') setNfts(prev => [...prev, data.nft]);
        };
        return () => ws.close();
    }, []);

    const handleSearch = async (e) => {
        e.preventDefault();
        const data = await apiRequest(`http://localhost:3000/api/nfts?q=${encodeURIComponent(search)}`);
        setNfts(data);
    };

    const chains = ['All', 'BSC', 'ETH', 'SOL'];

    return (
        <div>
            <Navbar bg="dark" variant="dark">
                <Navbar.Brand>Diamond NFT Marketplace</Navbar.Brand>
                <Form inline className="mx-auto" onSubmit={handleSearch}>
                    <Form.Control type="text" value={search} onChange={e => setSearch(e.target.value)} placeholder="Search NFTs..." style={{ width: '500px' }} />
                    <Button variant="outline-light" type="submit">Search</Button>
                </Form>
                <Dropdown>
                    <Dropdown.Toggle variant="success">Filter: {filterChain}</Dropdown.Toggle>
                    <Dropdown.Menu>
                        {chains.map(c => <Dropdown.Item key={c} onClick={() => setFilterChain(c)}>{c}</Dropdown.Item>)}
                    </Dropdown.Menu>
                </Dropdown>
            </Navbar>
            <div className="container mt-4">
                <h3>Explore NFTs</h3>
                <div className="d-flex flex-wrap">
                    <Suspense fallback={<div>Loading...</div>}>
                        {nfts.filter(nft => filterChain === 'All' || nft.chain === filterChain).map(nft => (
                            <NFTCard key={nft.id} nft={nft} />
                        ))}
                    </Suspense>
                </div>
                {status && <p>{status}</p>}
            </div>
        </div>
    );
};

export default NFTMarket;