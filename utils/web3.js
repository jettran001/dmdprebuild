const { ethers } = require('ethers');

const getProvider = (rpcUrl) => new ethers.providers.JsonRpcProvider(rpcUrl);

const getContract = (address, abi, providerOrSigner) => new ethers.Contract(address, abi, providerOrSigner);

module.exports = { getProvider, getContract };