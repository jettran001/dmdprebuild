import React, { useEffect, useState } from 'react';
import axios from 'axios';

const Settings = () => {
    const [username, setUsername] = useState('');

    useEffect(() => {
        const fetchProfile = async () => {
            const res = await axios.get('/api/users/profile', {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` }
            });
            setUsername(res.data.username);
        };
        fetchProfile();
    }, []);
    
    useEffect(() => {
        const source = new EventSource(`/api/snipebot/${chain}/sse`);
        source.onmessage = (event) => setStatus(`SSE Update: ${event.data}`);
        return () => source.close();
    }, [chain]);
    const updateProfile = async () => {
        await axios.put('/api/users/profile', { username }, {
            headers: { Authorization: `Bearer ${localStorage.getItem('token')}` }
        });
        alert('Profile updated!');
    };

    return (
        <div className="settings">
            <h2>Settings</h2>
            <input type="text" value={username} onChange={(e) => setUsername(e.target.value)} placeholder="Username" />
            <button onClick={updateProfile}>Update</button>
        </div>
    );
};

export default Settings;