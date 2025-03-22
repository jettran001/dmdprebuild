import React, { useState } from 'react';
import { View, TextInput, Button } from 'react-native';
import { sendTransaction } from '../utils/api';

const TransactionScreen = () => {
    const [amount, setAmount] = useState('');
    const [to, setTo] = useState('');

    const handleSend = async () => {
        try {
            const transaction = { amount: parseFloat(amount), to };
            const result = await sendTransaction(transaction);
            alert('Transaction sent: ' + JSON.stringify(result));
        } catch (error) {
            alert('Error: ' + error.message);
        }
    };

    return (
        <View>
            <TextInput
                placeholder="Amount"
                value={amount}
                onChangeText={setAmount}
                keyboardType="numeric"
            />
            <TextInput
                placeholder="To Address"
                value={to}
                onChangeText={setTo}
            />
            <Button title="Send" onPress={handleSend} />
        </View>
    );
};

export default TransactionScreen;