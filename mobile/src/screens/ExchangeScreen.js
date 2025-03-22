// mobile/src/screens/ExchangeScreen.js
import React, { useState } from 'react';
import { View, Text, TextInput, Button, Picker, Modal } from 'react-native';
import axios from 'axios';

const ExchangeScreen = () => {
    const telegramId = 'user123';
    const [searchQuery, setSearchQuery] = useState('');
    const [tokens, setTokens] = useState([]);
    const [selectedToken, setSelectedToken] = useState(null);
    const [amount, setAmount] = useState('');
    const [showChart, setShowChart] = useState(false);

    const handleSearch = async () => {
        try {
            const response = await axios.get(`http://your-server:port/searchToken?query=${searchQuery}`);
            setTokens(response.data);
        } catch (error) {
            console.error(error);
        }
    };

    const handleBuy = async () => {
        try {
            await axios.post('http://your-server:port/triggerManualBuy', {
                chain: selectedToken.chain,
                tokenAddress: selectedToken.address,
                amount,
                poolId: 'default-pool',
            });
            alert('Buy successful');
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    const handleSell = async () => {
        try {
            await axios.post('http://your-server:port/triggerManualSell', {
                chain: selectedToken.chain,
                tokenAddress: selectedToken.address,
                percentage: 100,
            });
            alert('Sell successful');
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    return (
        <View style={{ padding: 20 }}>
            <Text style={{ fontSize: 24, marginBottom: 20 }}>Exchange</Text>
            {/* Thanh tìm kiếm */}
            <View style={{ flexDirection: 'row', marginBottom: 20 }}>
                <TextInput
                    style={{ flex: 1, borderWidth: 1, padding: 5 }}
                    value={searchQuery}
                    onChangeText={setSearchQuery}
                    placeholder="Search token..."
                />
                <Button title="Search" onPress={handleSearch} />
            </View>
            {/* Phần Swap */}
            <View>
                <Text style={{ fontSize: 18, marginBottom: 10 }}>Swap</Text>
                {tokens.length > 0 && (
                    <Picker
                        selectedValue={selectedToken}
                        onValueChange={(itemValue) => setSelectedToken(tokens[itemValue])}
                    >
                        <Picker.Item label="Select Token" value={null} />
                        {tokens.map((token, index) => (
                            <Picker.Item
                                key={index}
                                label={`${token.symbol} (${token.chain})`}
                                value={index}
                            />
                        ))}
                    </Picker>
                )}
                <TextInput
                    style={{ borderWidth: 1, padding: 5, marginTop: 10 }}
                    value={amount}
                    onChangeText={setAmount}
                    placeholder="Amount"
                    keyboardType="numeric"
                />
                <View style={{ flexDirection: 'row', marginTop: 10 }}>
                    <Button title="Buy" onPress={handleBuy} />
                    <Button title="Sell" onPress={handleSell} style={{ marginLeft: 10 }} />
                </View>
            </View>
            {/* Phần Order Book */}
            <View style={{ marginTop: 20 }}>
                <Text style={{ fontSize: 18, marginBottom: 10 }}>Order Book</Text>
                <View style={{ borderWidth: 1, padding: 10 }}>
                    <Text>Buy Orders: (Mock data)</Text>
                    <Text>Sell Orders: (Mock data)</Text>
                </View>
                <Button title="Show Chart" onPress={() => setShowChart(true)} />
            </View>
            {/* Chart Popup */}
            <Modal visible={showChart} animationType="slide">
                <View style={{ padding: 20 }}>
                    <Text style={{ fontSize: 18, marginBottom: 20 }}>Chart</Text>
                    <Text>(Mock Chart)</Text>
                    <Button title="Close" onPress={() => setShowChart(false)} />
                </View>
            </Modal>
        </View>
    );
};

export default ExchangeScreen;