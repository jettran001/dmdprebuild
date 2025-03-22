import React from 'react';
import Header from '../components/Header';
import Footer from '../components/Footer';
import Sidebar from '../components/Sidebar';
import { sendTransaction } from '../utils/api';

const Home = () => {
    <div className="home">
        <Header />
        <Sidebar />
        <main>
            <h2>Welcome to Diamond Mainnet</h2>
            <p>Your decentralized finance hub.</p>
        </main>
        <Footer />
    </div>
    const [transaction, setTransaction] = useState({ amount: 0, to: '' });

    const handleSend = async () => {
        try {
            const result = await sendTransaction(transaction);
            alert('Transaction sent: ' + JSON.stringify(result));
        } catch (error) {
            alert('Error: ' + error.message);
        }
    };

    return (
        <div>
            <h1>Send Transaction</h1>
            <input
                type="number"
                value={transaction.amount}
                onChange={(e) => setTransaction({ ...transaction, amount: e.target.value })}
                placeholder="Amount"
            />
            <input
                type="text"
                value={transaction.to}
                onChange={(e) => setTransaction({ ...transaction, to: e.target.value })}
                placeholder="To Address"
            />
            <button onClick={handleSend}>Send</button>
        </div>
    );
};

export default Home;

