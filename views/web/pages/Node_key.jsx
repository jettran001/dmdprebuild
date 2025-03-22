// views/web/pages/Node_key.jsx
import React, { useState, useEffect } from 'react';
import axios from 'axios';
import { useTranslation } from 'react-i18next'; // Hỗ trợ multi-language

const Node_key = () => {
    const { t, i18n } = useTranslation();
    const [user, setUser] = useState(null); // Giả lập user đã đăng nhập
    const [walletAddress, setWalletAddress] = useState('');
    const [singleKeyForm, setSingleKeyForm] = useState({ inviterId: '', paymentToken: 'ETH', chain: 'ETH' });
    const [teamKeyForm, setTeamKeyForm] = useState({ inviterId: '', additionalKeys: 0, paymentToken: 'ETH', chain: 'ETH' });
    const [nftForm, setNftForm] = useState({ key: '' });
    const [newKeys, setNewKeys] = useState([]);
    const [showPopup, setShowPopup] = useState(false);
    const [showNftPopup, setShowNftPopup] = useState(false);
    const [botAccess, setBotAccess] = useState('Basic');

    useEffect(() => {
        // Giả lập kiểm tra user đã đăng nhập
        const userId = localStorage.getItem('userId');
        if (userId) {
            setUser({ id: userId, walletAddress: '0x123...' });
            setWalletAddress('0x123...');
            fetchBotAccess(userId, '0x123...');
        }
    }, []);

    const fetchBotAccess = async (userId, walletAddress) => {
        try {
            const response = await axios.get('/api/user/botAccess', {
                params: { userId, walletAddress }
            });
            setBotAccess(response.data.botAccess);
        } catch (error) {
            console.error('Error fetching bot access:', error);
        }
    };

    const handleCreateKey = async (type, form) => {
        try {
            const response = await axios.post('/api/admin/bot/createKey', {
                type,
                inviterId: form.inviterId || null,
                additionalKeys: form.additionalKeys || 0,
                userId: user?.id,
                walletAddress: walletAddress || '0xGuest...',
                paymentToken: form.paymentToken,
                chain: form.chain
            });
            setNewKeys(response.data.keys);
            setShowPopup(true);

            // Nếu là guest, tạo user mới
            if (!user) {
                const newUserId = response.data.keys[0].userId || crypto.randomBytes(8).toString('hex');
                localStorage.setItem('userId', newUserId);
                setUser({ id: newUserId, walletAddress: walletAddress || '0xGuest...' });
                fetchBotAccess(newUserId, walletAddress || '0xGuest...');
            }
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    const handleCreateNFT = async () => {
        if (!nftForm.key) return;

        const confirm = window.confirm(t('create_nft_confirm')); // Hỗ trợ multi-language
        if (!confirm) return;

        try {
            const response = await axios.post('/api/node/createNFT', {
                key: nftForm.key,
                walletAddress: walletAddress || '0xGuest...',
                userId: user?.id
            });
            alert(`${response.data.status}. Bot Access: ${response.data.botAccess}`);
            setShowNftPopup(false);
            setNftForm({ key: '' });
            fetchBotAccess(user?.id, walletAddress || '0xGuest...');
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    return (
        <div style={{ textAlign: 'center', padding: '20px' }}>
            <h1>Node Key</h1>
            <p>Current Bot Access: {botAccess}</p>
            <img src="/path/to/image.jpg" alt="Node Key" style={{ width: '300px', marginBottom: '20px' }} />
            <div style={{ display: 'flex', justifyContent: 'center', gap: '20px' }}>
                {/* Card Solo */}
                <div style={{ border: '1px solid #ccc', padding: '20px', width: '300px' }}>
                    <h3>Solo Key</h3>
                    <input
                        type="text"
                        placeholder="Inviter ID (optional)"
                        value={singleKeyForm.inviterId}
                        onChange={(e) => setSingleKeyForm({ ...singleKeyForm, inviterId: e.target.value })}
                        style={{ width: '100%', marginBottom: '10px' }}
                    />
                    <select
                        value={singleKeyForm.paymentToken}
                        onChange={(e) => setSingleKeyForm({ ...singleKeyForm, paymentToken: e.target.value })}
                        style={{ width: '100%', marginBottom: '10px' }}
                    >
                        <option value="ETH">ETH</option>
                        <option value="BNB">BNB</option>
                        <option value="USDC">USDC</option>
                    </select>
                    <select
                        value={singleKeyForm.chain}
                        onChange={(e) => setSingleKeyForm({ ...singleKeyForm, chain: e.target.value })}
                        style={{ width: '100%', marginBottom: '10px' }}
                    >
                        <option value="ETH">ETH</option>
                        <option value="Base">Base</option>
                        <option value="ARB">ARB</option>
                        <option value="BSC">BSC</option>
                    </select>
                    <button onClick={() => handleCreateKey('single', singleKeyForm)}>Buy Solo Key</button>
                </div>

                {/* Card Team */}
                <div style={{ border: '1px solid #ccc', padding: '20px', width: '300px' }}>
                    <h3>Team Key</h3>
                    <input
                        type="text"
                        placeholder="Inviter ID (optional)"
                        value={teamKeyForm.inviterId}
                        onChange={(e) => setTeamKeyForm({ ...teamKeyForm, inviterId: e.target.value })}
                        style={{ width: '100%', marginBottom: '10px' }}
                    />
                    <input
                        type="number"
                        placeholder="Additional Keys (default: 4)"
                        value={teamKeyForm.additionalKeys}
                        onChange={(e) => setTeamKeyForm({ ...teamKeyForm, additionalKeys: e.target.value })}
                        style={{ width: '100%', marginBottom: '10px' }}
                    />
                    <select
                        value={teamKeyForm.paymentToken}
                        onChange={(e) => setTeamKeyForm({ ...teamKeyForm, paymentToken: e.target.value })}
                        style={{ width: '100%', marginBottom: '10px' }}
                    >
                        <option value="ETH">ETH</option>
                        <option value="BNB">BNB</option>
                        <option value="USDC">USDC</option>
                    </select>
                    <select
                        value={teamKeyForm.chain}
                        onChange={(e) => setTeamKeyForm({ ...teamKeyForm, chain: e.target.value })}
                        style={{ width: '100%', marginBottom: '10px' }}
                    >
                        <option value="ETH">ETH</option>
                        <option value="Base">Base</option>
                        <option value="ARB">ARB</option>
                        <option value="BSC">BSC</option>
                    </select>
                    <button onClick={() => handleCreateKey('team', teamKeyForm)}>Buy Team Key</button>
                </div>
            </div>

            {/* Card Create NFT */}
            <div style={{ border: '1px solid #ccc', padding: '20px', width: '300px', margin: '20px auto' }}>
                <h3>Create NFT</h3>
                <button onClick={() => setShowNftPopup(true)}>Create NFT from Key</button>
            </div>

            {/* Popup hiển thị key */}
            {showPopup && (
                <div style={{ position: 'fixed', top: '50%', left: '50%', transform: 'translate(-50%, -50%)', backgroundColor: 'white', padding: '20px', border: '1px solid #ccc', zIndex: 1000 }}>
                    <h3>Your Keys</h3>
                    {newKeys.map((key, index) => (
                        <div key={index}>
                            <p>{key.key}</p>
                            <button onClick={() => navigator.clipboard.writeText(key.key)}>Copy</button>
                        </div>
                    ))}
                    <button onClick={() => setShowPopup(false)} style={{ marginTop: '10px' }}>Close</button>
                </div>
            )}

            {/* Popup tạo NFT */}
            {showNftPopup && (
                <div style={{ position: 'fixed', top: '50%', left: '50%', transform: 'translate(-50%, -50%)', backgroundColor: 'white', padding: '20px', border: '1px solid #ccc', zIndex: 1000 }}>
                    <h3>Create NFT</h3>
                    <input
                        type="text"
                        placeholder="Enter your key"
                        value={nftForm.key}
                        onChange={(e) => setNftForm({ key: e.target.value })}
                        style={{ width: '100%', marginBottom: '10px' }}
                    />
                    <button onClick={handleCreateNFT}>Confirm</button>
                    <button onClick={() => setShowNftPopup(false)} style={{ marginLeft: '10px' }}>Cancel</button>
                </div>
            )}
        </div>
    );
};

export default Node_key;