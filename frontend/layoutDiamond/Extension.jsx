import React from 'react';
import { Container } from 'react-bootstrap';

const Extension = ({ children }) => (
    <Container className="p-1" style={{ width: '300px' }}>
        <h4>Diamond Extension</h4>
        {children}
    </Container>
);

export default Extension;