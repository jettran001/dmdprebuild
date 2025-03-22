const uWS = require('uWebSockets.js');

uWS.App().get('/quic', (res) => {
    res.end('QUIC enabled');
}).listen(3001, (token) => {
    if (token) {
        console.log('[QUIC] Running on port 3001');
    } else {
        console.error('[QUIC] Failed to start on port 3001');
    }
});