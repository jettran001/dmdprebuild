import React from 'react';
import { Container } from 'react-bootstrap';

const MiniApp = ({ children }) => (
    <Container className="p-2" style={{ maxWidth: '400px' }}>
        <h3>Diamond MiniApp</h3>
        {children}
    </Container>
);

export default MiniApp;