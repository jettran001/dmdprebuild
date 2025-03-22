import React from 'react';
import ChainSelector from '../components/ChainSelector';


<ChainSelector chain={chain} setChain={setChain} chains={chains} />

const SnipebotControls = ({ chain, onManualBuy, onManualSell, onStartSmart, onStop, onGetProfitLoss }) => (
    <div className="snipebot-controls">
        <button onClick={() => onManualBuy(chain)}>Manual Buy</button>
        <button onClick={() => onManualSell(chain)}>Manual Sell</button>
        <button onClick={() => onStartSmart(chain)}>Start Smart Snipe</button>
        <button onClick={() => onStop(chain)}>Stop Snipe</button>
        <button onClick={() => onGetProfitLoss(chain)}>Get Profit/Loss</button>
    </div>
);

export default SnipebotControls;