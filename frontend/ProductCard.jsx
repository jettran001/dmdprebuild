import React from 'react';
import { Card, Button } from 'react-bootstrap';
import { apiRequest } from './utils/api';

const ProductCard = ({ product }) => {
    const buy = async () => {
        await apiRequest('http://localhost:3000/api/buy', 'POST', {
            itemId: product.id,
            wallet: '0xTestWallet'
        });
    };

    return (
        <Card style={{ width: '18rem', margin: '10px' }}>
            <Card.Body>
                <Card.Title>{product.name}</Card.Title>
                <Card.Text>{product.price} DMD</Card.Text>
                <Button onClick={buy}>Buy</Button>
            </Card.Body>
        </Card>
    );
};

export default ProductCard;