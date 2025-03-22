const Libp2p = require('libp2p');
const TCP = require('libp2p-tcp');
const Mplex = require('libp2p-mplex');
const { Noise } = require('libp2p-noise');

async function createLibp2pNode() {
    const node = await Libp2p.create({
        addresses: { listen: ['/ip4/0.0.0.0/tcp/0'] },
        modules: {
            transport: [TCP],
            streamMuxer: [Mplex],
            connEncryption: [Noise]
        }
    });
    await node.start();
    console.log('[libp2p] Node started:', node.peerId.toB58String());
    return node;
}

module.exports = { createLibp2pNode };