import React from 'react';

const ChainSelector = ({ chain, setChain, chains }) => (
    <div>
        <label>Chain:</label>
        <select value={chain} onChange={(e) => setChain(e.target.value)}>
            {chains.map(c => <option key={c} value={c}>{c.toUpperCase()}</option>)}
        </select>
    </div>
);

export default ChainSelector;