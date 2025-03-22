// mobile/src/screens/WalletScreen.js
import React, { useState, useEffect } from 'react';
import { View, Text, Button } from 'react-native';
import axios from 'axios';

const WalletScreen = ({ route }) => {
    const { telegramId } = route.params;
    const [wallet, setWallet] = useState(null);

    useEffect(() => {
        axios.post('http://your-server:port/createWallet', { telegramId })
            .then(res => setWallet(res.data.wallet))
            .catch(err => console.error(err));
    }, [telegramId]);

    return (
        <View>
            <Text>Wallet Address: {wallet?.address}</Text>
            <Button
                title="Connect to Bot"
                onPress={() => axios.post('http://your-server:port/connectToBot', { telegramId, chain: 'eth', tokenAddress: '0x123' })}
            />
        </View>
    );
};

export default WalletScreen;