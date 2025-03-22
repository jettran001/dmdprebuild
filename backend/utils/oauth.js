const jwt = require('jsonwebtoken');
const config = require('./config');

const generateOAuthToken = (userId) => {
    return jwt.sign({ userId }, config.jwtSecret, { expiresIn: '1h' });
};

const verifyOAuthToken = (token) => {
    return jwt.verify(token, config.jwtSecret);
};

module.exports = { generateOAuthToken, verifyOAuthToken };