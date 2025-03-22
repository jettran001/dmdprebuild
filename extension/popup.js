// extension/popup.js
document.addEventListener('DOMContentLoaded', () => {
    const telegramId = 'user123';
    const contentDiv = document.getElementById('content');
    const balanceSpan = document.getElementById('balance');

    fetch(`/api/walletBalance/${telegramId}/eth`)
        .then(res => res.json())
        .then(data => {
            balanceSpan.textContent = `${data.balance} ETH`;
            document.getElementById('wallet-balance').textContent = `${data.balance} ETH`;
        });

    document.getElementById('connect').addEventListener('click', () => {
        alert('Connect Wallet');
    });

    const renderExchangePage = () => {
        contentDiv.innerHTML = `
            <h1>Exchange</h1>
            <input id="searchQuery" type="text" placeholder="Search token..." style="width: 100%; padding: 5px;" />
            <button id="searchBtn" style="margin-top: 5px;">Search</button>
            <select id="tokenSelect" style="margin-top: 5px; width: 100%; padding: 5px;"></select>
            <input id="amount" type="number" placeholder="Amount" style="margin-top: 5px; width: 100%; padding: 5px;" />
            <button id="buyBtn" style="margin-top: 5px;">Buy</button>
            <button id="sellBtn" style="margin-top: 5px;">Sell</button>
            <h2>Order Book</h2>
            <div style="border: 1px solid #ccc; padding: 10px;">
                <p>Buy Orders: (Mock data)</p>
                <p>Sell Orders: (Mock data)</p>
            </div>
            <button id="chartBtn" style="margin-top: 5px;">Show Chart</button>
            <div id="chartPopup" style="display: none; position: fixed; top: 20%; left: 20%; width: 60%; height: 60%; background-color: white; border: 1px solid #ccc; z-index: 1000; padding: 20px;">
                <h3>Chart</h3>
                <p>(Mock Chart)</p>
                <button id="closeChart">Close</button>
            </div>
        `;

        document.getElementById('searchBtn').addEventListener('click', () => {
            const query = document.getElementById('searchQuery').value;
            fetch(`/api/searchToken?query=${query}`)
                .then(res => res.json())
                .then(tokens => {
                    const select = document.getElementById('tokenSelect');
                    select.innerHTML = '<option value="">Select Token</option>';
                    tokens.forEach((token, index) => {
                        select.innerHTML += `<option value="${index}">${token.symbol} (${token.chain})</option>`;
                    });
                });
        });

        document.getElementById('buyBtn').addEventListener('click', () => {
            const select = document.getElementById('tokenSelect');
            const amount = document.getElementById('amount').value;
            const token = tokens[select.value];
            fetch('/api/triggerManualBuy', {
                method: 'POST',
                body: JSON.stringify({ chain: token.chain, tokenAddress: token.address, amount, poolId: 'default-pool' }),
                headers: { 'Content-Type': 'application/json' },
            }).then(res => res.json()).then(data => alert(data.status));
        });

        document.getElementById('sellBtn').addEventListener('click', () => {
            const select = document.getElementById('tokenSelect');
            const token = tokens[select.value];
            fetch('/api/triggerManualSell', {
                method: 'POST',
                body: JSON.stringify({ chain: token.chain, tokenAddress: token.address, percentage: 100 }),
                headers: { 'Content-Type': 'application/json' },
            }).then(res => res.json()).then(data => alert(data.status));
        });

        document.getElementById('chartBtn').addEventListener('click', () => {
            document.getElementById('chartPopup').style.display = 'block';
        });

        document.getElementById('closeChart').addEventListener('click', () => {
            document.getElementById('chartPopup').style.display = 'none';
        });
    };

    document.querySelectorAll('[data-page]').forEach(link => {
        link.addEventListener('click', (e) => {
            e.preventDefault();
            const page = e.target.dataset.page;
            if (page === 'exchange') {
                renderExchangePage();
            } else {
                contentDiv.innerHTML = `<h1>${page.charAt(0).toUpperCase() + page.slice(1)}</h1>`;
                if (page === 'home') {
                    contentDiv.innerHTML += `
                        <p>Balance: <span id="wallet-balance">${balanceSpan.textContent}</span></p>
                        <h2>Transaction History</h2>
                        <div id="history"></div>
                    `;
                }
            }
        });
    });
});