import React from 'react';
import { Link } from 'react-router-dom';

const NavMenu = () => {
    return (
        <nav style={{ marginLeft: '20px' }}>
            <Link to="/" style={{ marginRight: '10px' }}>Home</Link>
            <Link to="/exchange" style={{ marginRight: '10px' }}>Exchange</Link>
            <Link to="/snipebot" style={{ marginRight: '10px' }}>Snipebot</Link>
            <Link to="/stake" style={{ marginRight: '10px' }}>Stake</Link>
            <Link to="/mission" style={{ marginRight: '10px' }}>Mission</Link>
        </nav>
    );
};

export default NavMenu;