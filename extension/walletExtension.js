// extension/walletExtension.js
chrome.runtime.onMessage.addListener((request, sender, sendResponse) => {
    if (request.type === 'createWallet') {
        fetch('http://your-server:port/createWallet', {
            method: 'POST',
            body: JSON.stringify({ telegramId: request.telegramId }),
            headers: { 'Content-Type': 'application/json' },
        })
            .then(res => res.json())
            .then(data => sendResponse(data))
            .catch(err => sendResponse({ error: err.message }));
        return true;
    }
});