// mobile/src/screens/RiskSettings.js
import React, { useState } from 'react';
import { View, Text, Slider, Button } from 'react-native';
import axios from 'axios';

const RiskSettings = () => {
    const [maxLoss, setMaxLoss] = useState(0.1);
    const [confidenceLevel, setConfidenceLevel] = useState(0.95);

    const saveSettings = async () => {
        try {
            await axios.post('http://your-server:port/updateRiskParameters', {
                maxLoss,
                confidenceLevel,
            });
            alert('Risk parameters updated successfully');
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    return (
        <View>
            <Text>Max Loss: {(maxLoss * 100).toFixed(0)}%</Text>
            <Slider
                minimumValue={0}
                maximumValue={1}
                value={maxLoss}
                onValueChange={setMaxLoss}
            />
            <Text>Confidence Level: {(confidenceLevel * 100).toFixed(0)}%</Text>
            <Slider
                minimumValue={0}
                maximumValue={1}
                value={confidenceLevel}
                onValueChange={setConfidenceLevel}
            />
            <Button title="Save" onPress={saveSettings} />
        </View>
    );
};

export default RiskSettings;