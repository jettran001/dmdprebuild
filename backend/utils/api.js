const axios = require('axios');

const apiRequest = async (url, method, data = {}) => {
    try {
        const response = await axios({
            url,
            method,
            data,
            headers: { 'Content-Type': 'application/json' },
        });
        return response.data;
    } catch (error) {
        console.error(`API Request Error: ${url}`, error);
        return null;
    }
};

module.exports = { apiRequest };