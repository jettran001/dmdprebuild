// views/web/pages/SearchPage.jsx
import React, { useState } from 'react';
import axios from 'axios';
import Header from '../components/Header';
import Footer from '../components/Footer';

const SearchPage = () => {
    const [searchQuery, setSearchQuery] = useState('');
    const [tokens, setTokens] = useState([]);
    const [selectedToken, setSelectedToken] = useState(null);
    const [auditResult, setAuditResult] = useState(null);

    const handleSearch = async () => {
        try {
            const response = await axios.get(`/api/searchToken?query=${searchQuery}`);
            setTokens(response.data);
        } catch (error) {
            console.error(error);
        }
    };

    const handleSelectToken = async (token) => {
        setSelectedToken(token);
        try {
            const response = await axios.get(`/api/auditToken?tokenAddress=${token.address}&chain=${token.chain}`);
            setAuditResult(response.data);
        } catch (error) {
            console.error(error);
        }
    };

    return (
        <div>
            <Header telegramId="user123" />
            <main style={{ padding: '20px' }}>
                <h1>Search</h1>
                <div style={{ marginBottom: '20px' }}>
                    <input
                        type="text"
                        value={searchQuery}
                        onChange={(e) => setSearchQuery(e.target.value)}
                        placeholder="Search token..."
                        style={{ padding: '5px', width: '300px' }}
                        onKeyPress={(e) => e.key === 'Enter' && handleSearch()}
                    />
                    <button onClick={handleSearch} style={{ marginLeft: '10px', padding: '5px 10px' }}>
                        🔍
                    </button>
                </div>
                {tokens.length > 0 && (
                    <ul style={{ listStyle: 'none', padding: 0 }}>
                        {tokens.map((token, index) => (
                            <li
                                key={index}
                                onClick={() => handleSelectToken(token)}
                                style={{ padding: '10px', cursor: 'pointer', borderBottom: '1px solid #ccc' }}
                            >
                                {token.symbol} ({token.chain})
                            </li>
                        ))}
                    </ul>
                )}
                {selectedToken && auditResult && (
                    <div style={{ marginTop: '20px', border: '1px solid #ccc', padding: '20px' }}>
                        <h2>{selectedToken.symbol} Information</h2>
                        <p>Audit Score: {auditResult.auditScore}</p>
                        <p>Warnings: {auditResult.warnings.join(', ')}</p>
                        <p>Dangerous Functions: {auditResult.dangerousFunctions.join(', ')}</p>
                        <p>Profit Potential: {auditResult.profitPotential}</p>
                        <p>Risk Assessment: {auditResult.riskAssessment}</p>
                    </div>
                )}
            </main>
            <Footer />
        </div>
    );
};

export default SearchPage;