// admin/auth.js
const jwt = require('jsonwebtoken');

// Giả lập danh sách admin (có thể thay bằng database)
const admins = [
    { username: 'admin', password: 'admin123' }
];

// Tạo token khi admin đăng nhập
const loginAdmin = (username, password) => {
    const admin = admins.find(a => a.username === username && a.password === password);
    if (!admin) throw new Error('Invalid credentials');

    const token = jwt.sign({ username }, 'YOUR_SECRET_KEY', { expiresIn: '1h' });
    return token;
};

// Xác thực token
const verifyAdmin = (token) => {
    try {
        const decoded = jwt.verify(token, 'YOUR_SECRET_KEY');
        return decoded;
    } catch (error) {
        throw new Error('Unauthorized');
    }
};

module.exports = { loginAdmin, verifyAdmin };