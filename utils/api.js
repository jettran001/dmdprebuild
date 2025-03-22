import axios from 'axios';
const API_URL = 'http://localhost:3000/api'; // Thay bằng URL backend thực tế

const apiRequest = async (url, method = 'GET', data = null, headers = {}) => {
    try {
        const config = { method, url, headers };
        if (data) config.data = data;
        const response = await axios(config);
        return response.data;
    } catch (error) {
        throw new Error(error.response?.data?.message || error.message);
    }
};


export const sendTransaction = async (transaction) => {
    try {
        const response = await axios.post(`${API_URL}/sendTransaction`, transaction);
        return response.data;
    } catch (error) {
        console.error('Error sending transaction:', error);
        throw error;
    }
};

export const getUserData = async (userId) => {
    try {
        const response = await axios.get(`${API_URL}/user/${userId}`);
        return response.data;
    } catch (error) {
        console.error('Error getting user data:', error);
        throw error;
    }
};

module.exports = { apiRequest };