// extensions/DiamondExtension/content.js
// Danh sách token phổ biến (có thể mở rộng hoặc lấy từ API)
const knownTokens = [
    'BTC', 'ETH', 'BNB', 'SOL', 'BASE', 'ARB', 'XLM', 'NEAR', 'TON', 'SUI', 'AVAX', 'PI',
    'DMD/BTC', 'DMD/ETH', 'DMD/BNB', 'DMD/SOL', 'DMD/BASE', 'DMD/ARB', 'DMD/XLM', 'DMD/NEAR',
    'DMD/TON', 'DMD/SUI', 'DMD/AVAX', 'DMD/PI'
];

// Các hàm hiện có trong content.js (giả sử extension đã có sẵn một số chức năng)
console.log('Diamond Extension loaded on this page');

// Thêm sự kiện bôi đen text để hiển thị tooltip
document.addEventListener('mouseup', showTokenAnalysisTooltip);

async function showTokenAnalysisTooltip() {
    const selection = window.getSelection();
    const selectedText = selection.toString().trim();

    // Xóa tooltip cũ nếu có
    const existingTooltip = document.getElementById('token-analysis-tooltip');
    if (existingTooltip) {
        existingTooltip.remove();
    }

    // Kiểm tra xem text bôi đen có phải là token không
    if (selectedText.length > 0 && knownTokens.includes(selectedText.toUpperCase())) {
        const range = selection.getRangeAt(0);
        const rect = range.getBoundingClientRect();

        // Gọi API để phân tích token
        try {
            const response = await fetch(`http://localhost:3000/api/analyzeToken?tokenPair=${selectedText}`, {
                method: 'GET',
                headers: {
                    'Content-Type': 'application/json'
                }
            });
            const data = await response.json();

            const { warningLevel, hasDangerousFunction, riskAssessment } = data;

            const tooltip = document.createElement('div');
            tooltip.id = 'token-analysis-tooltip';
            tooltip.innerHTML = `
                <strong>Token Analysis: ${selectedText}</strong><br/>
                Warning: ${warningLevel}<br/>
                Dangerous Function: ${hasDangerousFunction ? 'Yes' : 'No'}<br/>
                Risk/Potential: ${riskAssessment}
            `;
            document.body.appendChild(tooltip);

            // Định vị tooltip ngay trên vùng bôi đen
            tooltip.style.top = `${rect.top + window.scrollY - 80}px`;
            tooltip.style.left = `${rect.left + window.scrollX}px`;
        } catch (error) {
            console.error('Error analyzing token:', error);
            const tooltip = document.createElement('div');
            tooltip.id = 'token-analysis-tooltip';
            tooltip.innerHTML = `⚠️ Unable to analyze token: ${selectedText}`;
            document.body.appendChild(tooltip);

            tooltip.style.top = `${rect.top + window.scrollY - 40}px`;
            tooltip.style.left = `${rect.left + window.scrollX}px`;
        }
    }
}

// Ẩn tooltip khi không còn bôi đen
document.addEventListener('mousedown', () => {
    const existingTooltip = document.getElementById('token-analysis-tooltip');
    if (existingTooltip) {
        existingTooltip.remove();
    }
});