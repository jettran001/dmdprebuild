import React, { useState, useEffect } from 'react';
import axios from 'axios';
import Header from '../components/Header';
import Footer from '../components/Footer';
import io from 'socket.io-client';
import mqtt from 'mqtt';
import SnipebotControls from '../components/SnipebotControls';
import Notification from '../components/Notification';
import Peer from 'simple-peer';
import grpc from '@grpc/grpc-js';
import protoLoader from '@grpc/proto-loader';

const packageDefinition = protoLoader.loadSync('snipebot.proto', {});
const snipebotProto = grpc.loadPackageDefinition(packageDefinition).snipebot;
const grpcClient = new snipebotProto.SnipebotService('localhost:50051', grpc.credentials.createInsecure());

const Snipebot = () => {
    const [peerData, setPeerData] = useState(null);
    const [chain, setChain] = useState('bsc');
    const [tokenAddress, setTokenAddress] = useState('');
    const [amount, setAmount] = useState('');
    const [poolId, setPoolId] = useState('');
    const [percentage, setPercentage] = useState('');
    const [profitLoss, setProfitLoss] = useState({ profit: 0, loss: 0 });
    const [status, setStatus] = useState('');
    const [botStatus, setBotStatus] = useState({});
    const [history, setHistory] = useState([]);
    const [settings, setSettings] = useState({ minProfit: 0.01, maxSlippage: 0.05, tslPercentage: 0.05 });
    
    const chains = ['arb','base','bsc', 'eth', 'sol', 'near', 'sui', 'ton'];

    useEffect(() => {
        const socket = io('http://localhost:3000');
        const peer = new Peer({ initiator: true, trickle: false });

        peer.on('signal', (data) => socket.emit('signal', data));
        socket.on('signal', (data) => peer.signal(data));
        socket.on('tradeHistory', (trade) => setHistory(prev => [...prev, trade].slice(-10)));
        peer.on('data', (data) => setPeerData(data.toString()));

        return () => {
            socket.disconnect();
            peer.destroy();
        };
    }, []);
    
    useEffect(() => {
        const mqttClient = mqtt.connect('mqtt://localhost:1883');
        mqttClient.on('connect', () => mqttClient.subscribe('snipebot/ton/trades'));
        mqttClient.on('message', (topic, message) => setHistory(prev => [...prev, JSON.parse(message)].slice(-10)));
        return () => mqttClient.end();
    }, []);

    useEffect(() => {
        const source = new EventSource(`/api/snipebot/${chain}/sse`);
        source.onmessage = (event) => setStatus(`SSE Update: ${event.data}`);
        return () => source.close();
    }, [chain]);
//websocket,MQTT,WebRTC,SSE, gRPC
    useEffect(() => {
        // WebSocket
        const socket = io('http://localhost:3000');
        socket.on('tradeHistory', (trade) => setHistory(prev => [...prev, trade].slice(-10)));

        // MQTT
        const mqttClient = mqtt.connect('mqtt://localhost:1883');
        mqttClient.on('connect', () => {
            mqttClient.subscribe('snipebot/ton/trades');
        });
        mqttClient.on('message', (topic, message) => {
            setHistory(prev => [...prev, JSON.parse(message.toString())].slice(-10));
        });
        // WebRTC
        const peer = new Peer({ initiator: true, trickle: false });
        peer.on('signal', (data) => socket.emit('signal', data));
        socket.on('signal', (data) => peer.signal(data));
        peer.on('data', (data) => setPeerData(data.toString()));

        // SSE
        const source = new EventSource(`/api/snipebot/ton/sse`);
        source.onmessage = (event) => setStatus(`SSE: ${event.data}`);

        // gRPC
        grpcClient.TriggerManualBuy({ chain: 'ton', tokenAddress: '0x123', amount: 0.1 }, (err, response) => {
            if (!err) setStatus(`gRPC: ${response.status}`);
        });

        return () => {
            socket.disconnect();
            mqttClient.end();
            peer.destroy();
            source.close();
            
        };
    }, []);

    const handleManualBuy = async (selectedChain) => {
        try {
            const res = await axios.post(`/api/snipebot/${selectedChain}/manual`, 
                { tokenAddress, amount, poolId }, 
                { headers: { Authorization: `Bearer ${localStorage.getItem('token')}` } }
            );
            setStatus(`Manual Buy: ${res.data.status}`);
        } catch (error) {
            setStatus(`Error: ${error.response?.data?.error || error.message}`);
        }
    };

    const handleManualSell = async (selectedChain) => {
        try {
            const res = await axios.post(`/api/snipebot/${selectedChain}/stop`, 
                { tokenAddress, percentage }, 
                { headers: { Authorization: `Bearer ${localStorage.getItem('token')}` } }
            );
            setStatus(`Manual Sell: ${res.data.status}`);
            setProfitLoss(res.data.profitLoss);
        } catch (error) {
            setStatus(`Error: ${error.response?.data?.error || error.message}`);
        }
    };

    const fetchFromIPFS = async (cid) => {
        try {
            const res = await axios.get(`/api/snipebot/ton/ipfs/${cid}`);
            setStatus(`IPFS Data: ${JSON.stringify(res.data)}`);
        } catch (error) {
            setStatus(`IPFS Error: ${error.message}`);
        }
    };

    const handleStartSmart = async (selectedChain) => {
        try {
            const res = await axios.post(`/api/snipebot/${selectedChain}/smart`, 
                {}, 
                { headers: { Authorization: `Bearer ${localStorage.getItem('token')}` } }
            );
            setStatus(`Smart Snipe: ${res.data.status}`);
        } catch (error) {
            setStatus(`Error: ${error.response?.data?.error || error.message}`);
        }
    };

    const handleStop = async (selectedChain) => {
        try {
            const res = await axios.post(`/api/snipebot/${selectedChain}/stop`, 
                { tokenAddress }, 
                { headers: { Authorization: `Bearer ${localStorage.getItem('token')}` } }
            );
            setStatus(`Stop Snipe: ${res.data.status}`);
            setProfitLoss(res.data.profitLoss);
        } catch (error) {
            setStatus(`Error: ${error.response?.data?.error || error.message}`);
        }
    };

    const handleGetProfitLoss = async (selectedChain) => {
        try {
            const res = await axios.get(`/api/snipebot/${selectedChain}/profitloss`, 
                { headers: { Authorization: `Bearer ${localStorage.getItem('token')}` } }
            );
            setProfitLoss(res.data);
            setStatus(`Profit/Loss updated for ${selectedChain.toUpperCase()}`);
        } catch (error) {
            setStatus(`Error: ${error.response?.data?.error || error.message}`);
        }
    };

    const handleUpdateSettings = async (selectedChain) => {
        try {
            await axios.post(`/api/snipebot/${selectedChain}/settings`, 
                settings, 
                { headers: { Authorization: `Bearer ${localStorage.getItem('token')}` } }
            );
            setStatus(`Settings updated for ${selectedChain.toUpperCase()}`);
        } catch (error) {
            setStatus(`Error: ${error.response?.data?.error || error.message}`);
        }
    };

    return (
        <div className="snipebot">
            
            <h2>Snipebot Dashboard</h2>
            <h3>Status: {status}</h3>
            <h3>Trade History</h3>            
            <h3>WebRTC Data: {peerData || 'Waiting...'}</h3>
                <h3>Trade History</h3>
                <ul>{history.map((trade, idx) => <li key={idx}>{trade.type} - {trade.tokenAddress}</li>)}</ul>
                            
            <div className="controls">
                <label>Chain:</label>
                <select value={chain} onChange={(e) => setChain(e.target.value)}>
                {chains.map(c => <option key={c} value={c}>{c.toUpperCase()}</option>)}
                </select>
                <SnipebotControls 
                    chain={chain} 
                    onManualBuy={handleManualBuy} 
                    onManualSell={handleManualSell} 
                    onStartSmart={handleStartSmart} 
                    onStop={handleStop} 
                    onGetProfitLoss={handleGetProfitLoss} 
                />

                <label>Token Address:</label>
                <input type="text" value={tokenAddress} onChange={(e) => setTokenAddress(e.target.value)} placeholder="Token Address" />

                <label>Amount (for Buy):</label>
                <input type="number" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="Amount" />

                <label>Pool ID (optional):</label>
                <input type="text" value={poolId} onChange={(e) => setPoolId(e.target.value)} placeholder="Pool ID" />

                <label>Percentage (for Sell):</label>
                <input type="number" value={percentage} onChange={(e) => setPercentage(e.target.value)} placeholder="Percentage to sell" />

                
            </div>

            <div className="dashboard">
                <h3>Bot Status</h3>
                {chains.map(c => (
                    <p key={c}>{c.toUpperCase()}: {botStatus[c] || 'Stopped'}</p>
                ))}

                <h3>Profit/Loss</h3>
                <p>Profit: {profitLoss.profit} | Loss: {profitLoss.loss}</p>
                <h3>Trade History (Last 10)</h3>
                <ul>
                    {history.map((trade, idx) => (
                        <li key={idx}>
                            {trade.type} {trade.tokenAddress} - {trade.amount} on {trade.chain}
                            {trade.cid && (
                                <button onClick={() => fetchFromIPFS(trade.cid)}>
                                    View on IPFS
                                </button>
                            )}
                        </li>
                    ))}
                </ul>
                <h3>WebRTC Data: {peerData || 'Waiting...'}</h3>
                <div className="status">
                    <h3>Status: {status}</h3>
                </div>
                <h3>Bot Settings</h3>
                <label>Min Profit:</label>
                <input type="number" value={settings.minProfit} onChange={(e) => setSettings({ ...settings, minProfit: e.target.value })} />
                <label>Max Slippage:</label>
                <input type="number" value={settings.maxSlippage} onChange={(e) => setSettings({ ...settings, maxSlippage: e.target.value })} />
                <label>TSL Percentage:</label>
                <input type="number" value={settings.tslPercentage} onChange={(e) => setSettings({ ...settings, tslPercentage: e.target.value })} />
                <button onClick={() => handleUpdateSettings(chain)}>Update Settings</button>
            </div>

            
            <Notification />
        </div>
    );
};

export default Snipebot;