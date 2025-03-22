import axios from 'axios';

const API_URL = 'http://localhost:3000/api'; // Thay bằng URL backend thực tế

export const sendTransaction = async (transaction) => {
    try {
        const response = await axios.post(`${API_URL}/sendTransaction`, transaction);
        return response.data;
    } catch (error) {
        console.error('Error sending transaction:', error);
        throw error;
    }
};