// backend/utils/tokenSearch.js
const axios = require('axios');
const { logger } = require('./logger');

class TokenSearch {
    async searchToken(query) {
        const apis = [
            { name: 'DexScreener', url: `https://api.dexscreener.com/latest/dex/search?q=${query}` },
            { name: 'DexTools', url: `https://www.dextools.io/app/api/search?query=${query}`, headers: { 'Authorization': `Bearer ${process.env.DEXTOOLS_API_KEY}` } },
            { name: 'DexView', url: `https://api.dexview.com/search?query=${query}` },
            { name: 'CoinGecko', url: `https://api.coingecko.com/api/v3/search?query=${query}` },
        ];

        for (const api of apis) {
            try {
                const response = await axios.get(api.url, { headers: api.headers || {} });
                let tokens = [];

                if (api.name === 'DexScreener') {
                    tokens = response.data.pairs.map(pair => ({
                        symbol: pair.baseToken.symbol,
                        address: pair.baseToken.address,
                        chain: pair.chainId,
                    }));
                } else if (api.name === 'DexTools') {
                    tokens = response.data.results.map(result => ({
                        symbol: result.symbol,
                        address: result.address,
                        chain: result.chain,
                    }));
                } else if (api.name === 'DexView') {
                    tokens = response.data.tokens.map(token => ({
                        symbol: token.symbol,
                        address: token.contractAddress,
                        chain: token.chain,
                    }));
                } else if (api.name === 'CoinGecko') {
                    tokens = response.data.coins.map(coin => ({
                        symbol: coin.symbol,
                        address: coin.contract_address || 'N/A',
                        chain: 'eth',
                    }));
                }

                if (tokens.length > 0) {
                    logger.info(`Fetched tokens from ${api.name}`);
                    return tokens.slice(0, 10);
                }
            } catch (error) {
                logger.error(`Error fetching tokens from ${api.name}: ${error.message}`);
                continue;
            }
        }

        return [];
    }

    async auditToken(tokenAddress, chain) {
        try {
            // Giả lập audit (thay bằng API thực tế như RugCheck, TokenSniffer)
            const auditScore = Math.random() * 100;
            const warnings = [];
            const dangerousFunctions = [];

            if (auditScore < 30) {
                warnings.push('🔴 High Risk');
                dangerousFunctions.push('Honeypot detected');
            } else if (auditScore < 70) {
                warnings.push('🟡 Medium Risk');
                dangerousFunctions.push('High buy/sell tax');
            } else {
                warnings.push('🟢 Low Risk');
            }

            const profitPotential = auditScore > 50 ? 'High' : 'Low';
            const riskAssessment = auditScore < 50 ? 'High risk of loss' : 'Moderate risk';

            return {
                auditScore,
                warnings,
                dangerousFunctions,
                profitPotential,
                riskAssessment,
            };
        } catch (error) {
            logger.error(`Error auditing token: ${error.message}`);
            return { auditScore: 0, warnings: [], dangerousFunctions: [], profitPotential: 'Unknown', riskAssessment: 'Unknown' };
        }
    }
}

module.exports = new TokenSearch();