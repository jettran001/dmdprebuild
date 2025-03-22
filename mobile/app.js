// mobile/src/App.js
import React from 'react';
import { NavigationContainer } from '@react-navigation/native';
import { createBottomTabNavigator } from '@react-navigation/bottom-tabs';
import { createStackNavigator } from '@react-navigation/stack';
import WalletScreen from './screens/WalletScreen';
import HomeScreen from './screens/HomeScreen';
import ExchangeScreen from './screens/ExchangeScreen';
import SnipebotScreen from './screens/SnipebotScreen';
import StakeScreen from './screens/StakeScreen';
import MissionScreen from './screens/MissionScreen';

const Tab = createBottomTabNavigator();
const Stack = createStackNavigator();

const TabNavigator = () => (
    <Tab.Navigator
        screenOptions={{
            tabBarStyle: { position: 'absolute', bottom: 0, justifyContent: 'center' },
        }}
    >
        <Tab.Screen name="Home" component={HomeScreen} />
        <Tab.Screen name="Exchange" component={ExchangeScreen} />
        <Tab.Screen name="Snipebot" component={SnipebotScreen} />
        <Tab.Screen name="Stake" component={StakeScreen} />
        <Tab.Screen name="Mission" component={MissionScreen} />
    </Tab.Navigator>
);

const App = () => {
    return (
        <NavigationContainer>
            <Stack.Navigator>
                <Stack.Screen
                    name="Main"
                    component={TabNavigator}
                    options={{
                        headerLeft: () => (
                            <View style={{ marginLeft: 10 }}>
                                <Button title="Menu" onPress={() => alert('Menu')} />
                            </View>
                        ),
                        headerRight: () => (
                            <View style={{ flexDirection: 'row', alignItems: 'center', marginRight: 10 }}>
                                <Text style={{ marginRight: 10 }}>0 ETH</Text>
                                <Button title="Connect" onPress={() => alert('Connect Wallet')} />
                            </View>
                        ),
                    }}
                />
                <Stack.Screen name="Wallet" component={WalletScreen} />
            </Stack.Navigator>
        </NavigationContainer>
    );
};

export default App;