// mobile/src/screens/TradeHistory.js
import React, { useState, useEffect } from 'react';
import { View, Text, FlatList } from 'react-native';
import axios from 'axios';

const TradeHistory = ({ route }) => {
    const { chain } = route.params;
    const [history, setHistory] = useState([]);

    useEffect(() => {
        axios.get(`http://your-server:port/getTradeHistory/${chain}`)
            .then(res => setHistory(res.data))
            .catch(err => alert(`Error: ${err.message}`));
    }, [chain]);

    return (
        <View>
            <Text>Trade History for {chain.toUpperCase()}</Text>
            <FlatList
                data={history}
                renderItem={({ item }) => (
                    <View>
                        <Text>Token: {item.tokenAddress}</Text>
                        <Text>Mode: {item.mode}</Text>
                        <Text>Buy Amount: {item.buyAmount}</Text>
                        <Text>Buy Price: {item.buyPrice}</Text>
                        <Text>Status: {item.status}</Text>
                    </View>
                )}
                keyExtractor={item => item._id}
            />
        </View>
    );
};

export default TradeHistory;

