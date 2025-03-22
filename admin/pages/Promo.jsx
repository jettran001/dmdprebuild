// admin/pages/Promo.jsx
import React, { useState, useEffect } from 'react';
import axios from 'axios';

const Promo = () => {
    const [pendingTasks, setPendingTasks] = useState([]);

    useEffect(() => {
        const fetchData = async () => {
            try {
                const response = await axios.get('/api/admin/promo/pendingTasks', {
                    headers: { Authorization: `Bearer ${localStorage.getItem('adminToken')}` }
                });
                setPendingTasks(response.data);
            } catch (error) {
                console.error('Error fetching pending tasks:', error);
            }
        };
        fetchData();
    }, []);

    const handleConfirmTask = async (taskId) => {
        try {
            const response = await axios.post('/api/admin/promo/confirmTask', { taskId }, {
                headers: { Authorization: `Bearer ${localStorage.getItem('adminToken')}` }
            });
            alert(response.data.status);
            setPendingTasks(pendingTasks.filter(task => task.id !== taskId));
        } catch (error) {
            alert(`Error: ${error.message}`);
        }
    };

    return (
        <div>
            <h1>Promo - Pending Tasks</h1>
            {pendingTasks.length === 0 ? (
                <p>No pending tasks</p>
            ) : (
                <div>
                    {pendingTasks.map(task => (
                        <div key={task.id} style={{ border: '1px solid #ccc', padding: '10px', marginBottom: '10px' }}>
                            <h3>{task.name}</h3>
                            <p>Description: {task.description}</p>
                            <p>Reward: {task.reward} DMD</p>
                            <button onClick={() => handleConfirmTask(task.id)}>Confirm</button>
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
};

export default Promo;