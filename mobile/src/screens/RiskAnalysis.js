import React, { useState, useEffect } from 'react';
import { View, Text } from 'react-native';
import axios from 'axios';
import { LineChart } from 'react-native-chart-kit';

const RiskAnalysis = () => {
    const [riskData, setRiskData] = useState({ varValue: 0, historicalReturns: [] });

    useEffect(() => {
        axios.get('http://your-server:port/getRiskData')
            .then(res => setRiskData(res.data))
            .catch(err => alert(`Error: ${err.message}`));
    }, []);

    return (
        <View>
            <Text>Risk Analysis</Text>
            <Text>VaR: {(riskData.varValue * 100).toFixed(2)}%</Text>
            <LineChart
                data={{
                    labels: Array.from({ length: riskData.historicalReturns.length }, (_, i) => i.toString()),
                    datasets: [{ data: riskData.historicalReturns.map(r => r * 100) }],
                }}
                width={300}
                height={200}
                chartConfig={{
                    backgroundColor: '#e26a00',
                    backgroundGradientFrom: '#fb8c00',
                    backgroundGradientTo: '#ffa726',
                    decimalPlaces: 2,
                    color: (opacity = 1) => `rgba(255, 255, 255, ${opacity})`,
                    labelColor: (opacity = 1) => `rgba(255, 255, 255, ${opacity})`,
                }}
                bezier
            />
        </View>
    );
};

export default RiskAnalysis;