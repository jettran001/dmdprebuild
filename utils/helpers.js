const validateAddress = (address) => /^0x[a-fA-F0-9]{40}$/.test(address);

const formatNumber = (num) => Number(num).toFixed(2);

module.exports = { validateAddress, formatNumber };