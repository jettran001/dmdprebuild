import React, { useState } from 'react';
import { Card, Button, Modal } from 'react-bootstrap';
import { apiRequest } from './utils/api';

const NFTCard = ({ nft }) => {
    const [showModal, setShowModal] = useState(false);
    const [status, setStatus] = useState('');
    const wallet = '0xTestWallet';

    const buyNFT = async () => {
        const data = await apiRequest('http://localhost:3000/api/buy-nft', 'POST', {
            nftId: nft.id,
            wallet
        });
        setStatus(data.status || data.error);
    };

    return (
        <>
            <Card style={{ width: '18rem', margin: '10px' }}>
                <Card.Img variant="top" src={nft.uri || 'https://via.placeholder.com/150'} />
                <Card.Body>
                    <Card.Title>NFT #{nft.id}</Card.Title>
                    <Card.Text>{nft.price / 1e18} DMD</Card.Text>
                    <Button variant="primary" onClick={buyNFT}>Buy Now</Button>
                    <Button variant="outline-primary" className="ml-2" onClick={() => setShowModal(true)}>Details</Button>
                    {status && <p>{status}</p>}
                </Card.Body>
            </Card>
            <Modal show={showModal} onHide={() => setShowModal(false)}>
                <Modal.Header closeButton>
                    <Modal.Title>NFT #{nft.id}</Modal.Title>
                </Modal.Header>
                <Modal.Body>
                    <img src={nft.uri} alt="NFT" style={{ width: '100%' }} />
                    <p>Price: {nft.price / 1e18} DMD</p>
                    <p>Chain: {nft.chain || 'BSC'}</p>
                    <p>Owner: {nft.owner || '0xOwner'}</p>
                    <p>History: Sold 10/03/2025 for 0.5 DMD</p>
                </Modal.Body>
            </Modal>
        </>
    );
};

export default NFTCard;