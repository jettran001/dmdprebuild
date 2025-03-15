import React, { useState, useEffect } from 'react';
import { Container } from 'react-bootstrap';
import ProductCard from './ProductCard';

const Home = () => {
    const [products, setProducts] = useState([]);

    useEffect(() => {
        fetch('http://localhost:3000/api/bandwidth-storage')
            .then(res => res.json())
            .then(data => setProducts(data));
    }, []);

    return (
        <Container className="mt-4">
            <h3>Bandwidth & Storage</h3>
            <div className="d-flex flex-wrap">
                {products.map(p => <ProductCard key={p.id} product={p} />)}
            </div>
        </Container>
    );
};

export default Home;