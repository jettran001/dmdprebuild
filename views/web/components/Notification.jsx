import React, { useState, useEffect } from 'react';
import io from 'socket.io-client';

const Notification = () => {
    const [message, setMessage] = useState('');

    useEffect(() => {
        const socket = io('http://localhost:3000');
        socket.on('notification', (data) => {
            setMessage(`${data.message}: ${JSON.stringify(data.data)}`);
        });
        return () => socket.disconnect();
    }, []);

    return message ? <div className="notification">{message}</div> : null;
};

export default Notification;