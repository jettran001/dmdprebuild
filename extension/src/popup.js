import { sendTransaction } from './api';

document.getElementById('sendButton').addEventListener('click', async () => {
    const transaction = { amount: 100, to: '0x123...' };
    try {
        const result = await sendTransaction(transaction);
        alert('Transaction sent: ' + JSON.stringify(result));
    } catch (error) {
        alert('Error: ' + error.message);
    }
});