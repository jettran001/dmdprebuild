const API_URL = 'http://localhost:3000/api';

export const sendTransaction = async (transaction) => {
    try {
        const response = await fetch(`${API_URL}/sendTransaction`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(transaction)
        });
        return await response.json();
    } catch (error) {
        console.error('Error sending transaction:', error);
        throw error;
    }
};