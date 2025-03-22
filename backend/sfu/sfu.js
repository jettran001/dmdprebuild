const mediasoup = require('mediasoup');

let worker;
let router;

async function startSFU() {
    worker = await mediasoup.createWorker();
    router = await worker.createRouter({ mediaCodecs: [] });
    console.log('[SFU] Running');
    return router;
}

module.exports = { startSFU };