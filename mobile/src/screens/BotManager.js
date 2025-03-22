// mobile/src/screens/BotManager.js
import React, { useState, useEffect } from 'react';
import { View, Text, Button, FlatList } from 'react-native';
import axios from 'axios';
import { EventSourcePolyfill } from 'event-source-polyfill';

const BotManager = () => {
    const [botStatus, setBotStatus] = useState({});
    const [notifications, setNotifications] = useState([]);

    useEffect(() => {
        const chains = ['sol', 'bsc', 'eth']; // Danh sách chain
        chains.forEach(chain => {
            // Kiểm tra trạng thái ban đầu
            axios.get(`http://your-server:port/getBotStatus/${chain}`)
                .then(res => {
                    setBotStatus(prev => ({ ...prev, [chain]: res.data.status }));
                })
                .catch(err => console.error(err));

            // Kết nối WebSocket để nhận thông báo
            const source = new EventSourcePolyfill(`http://your-server:port/triggerSnipeSSE/${chain}`);
            source.onmessage = (event) => {
                const data = JSON.parse(event.data);
                setNotifications(prev => [...prev, `${chain}: ${data.status}`]);
            };
            source.onerror = () => source.close();
            return () => source.close();
        });
    }, []);

    const startBot = (chain) => {
        axios.post('http://your-server:port/triggerSnipeMEV', { chain, tokenAddress: '0x123' })
            .then(res => alert(res.data.status))
            .catch(err => alert(`Error: ${err.message}`));
    };

    const stopBot = (chain) => {
        axios.post('http://your-server:port/stopSnipe', { chain, tokenAddress: '0x123' })
            .then(res => alert(res.data.status))
            .catch(err => alert(`Error: ${err.message}`));
    };

    return (
        <View>
            <Text>Bot Manager</Text>
            {Object.keys(botStatus).map(chain => (
                <View key={chain}>
                    <Text>{chain.toUpperCase()}: {botStatus[chain]}</Text>
                    <Button title={`Start ${chain}`} onPress={() => startBot(chain)} />
                    <Button title={`Stop ${chain}`} onPress={() => stopBot(chain)} />
                </View>
            ))}
            <Text>Notifications:</Text>
            <FlatList
                data={notifications}
                renderItem={({ item }) => <Text>{item}</Text>}
                keyExtractor={(item, index) => index.toString()}
            />
        </View>
    );
};

export default BotManager;