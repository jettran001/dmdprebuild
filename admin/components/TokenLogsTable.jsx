// admin/components/TokenLogsTable.jsx
import React, { useState } from 'react';

const TokenLogsTable = ({ tokenLogs }) => {
    const [currentPage, setCurrentPage] = useState(1);
    const itemsPerPage = 5;

    const indexOfLastItem = currentPage * itemsPerPage;
    const indexOfFirstItem = indexOfLastItem - itemsPerPage;
    const currentItems = tokenLogs.slice(indexOfFirstItem, indexOfLastItem);

    const totalPages = Math.ceil(tokenLogs.length / itemsPerPage);

    return (
        <div>
            <table style={{ width: '100%', borderCollapse: 'collapse' }}>
                <thead>
                    <tr>
                        <th style={{ border: '1px solid #ccc', padding: '8px' }}>Token</th>
                        <th style={{ border: '1px solid #ccc', padding: '8px' }}>Contract Address</th>
                        <th style={{ border: '1px solid #ccc', padding: '8px' }}>Trades</th>
                    </tr>
                </thead>
                <tbody>
                    {currentItems.map((log, index) => (
                        <tr key={index}>
                            <td style={{ border: '1px solid #ccc', padding: '8px' }}>{log.token}</td>
                            <td style={{ border: '1px solid #ccc', padding: '8px' }}>{log.contractAddress}</td>
                            <td style={{ border: '1px solid #ccc', padding: '8px' }}>{log.trades}</td>
                        </tr>
                    ))}
                </tbody>
            </table>
            <div style={{ marginTop: '10px' }}>
                <button
                    onClick={() => setCurrentPage(page => Math.max(page - 1, 1))}
                    disabled={currentPage === 1}
                >
                    Previous
                </button>
                <span style={{ margin: '0 10px' }}>Page {currentPage} of {totalPages}</span>
                <button
                    onClick={() => setCurrentPage(page => Math.min(page + 1, totalPages))}
                    disabled={currentPage === totalPages}
                >
                    Next
                </button>
            </div>
        </div>
    );
};

export default TokenLogsTable;