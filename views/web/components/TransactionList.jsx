import React, { useEffect, useState } from 'react';
import axios from 'axios';

const TransactionList = () => {
    const [transactions, setTransactions] = useState([]);

    useEffect(() => {
        const fetchTransactions = async () => {
            const res = await axios.get('/api/transactions', {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` }
            });
            setTransactions(res.data);
        };
        fetchTransactions();
    }, []);

    return (
        <div className="transaction-list">
            {transactions.map(tx => (
                <div key={tx._id}>
                    <p>Hash: {tx.txHash}</p>
                    <p>Status: {tx.status}</p>
                </div>
            ))}
        </div>
    );
};

export default TransactionList;