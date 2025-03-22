// admin/components/Sidebar.jsx
import React from 'react';

const Sidebar = ({ setCurrentPage }) => {
    const pages = [
        'Home',
        'Depin_adminManager',
        'Pools_adminManager',
        'Promo',
        'Bot_adminManager',
        'Node_keyManager'
    ];

    return (
        <div style={{
            width: '200px',
            height: '100vh',
            backgroundColor: '#1a1a1a',
            color: 'white',
            position: 'fixed',
            paddingTop: '20px'
        }}>
            <h2 style={{ textAlign: 'center' }}>Admin Dashboard</h2>
            <ul style={{ listStyle: 'none', padding: 0 }}>
                {pages.map(page => (
                    <li
                        key={page}
                        onClick={() => setCurrentPage(page)}
                        style={{
                            padding: '10px 20px',
                            cursor: 'pointer',
                            backgroundColor: page === 'Home' ? '#333' : 'transparent'
                        }}
                    >
                        {page}
                    </li>
                ))}
            </ul>
        </div>
    );
};

export default Sidebar;