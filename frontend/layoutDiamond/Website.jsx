import React from 'react';
import { Container } from 'react-bootstrap';

const Website = ({ children }) => (
    <Container fluid className="p-3">
        <h1>Diamond Website</h1>
        {children}
    </Container>
);

export default Website;