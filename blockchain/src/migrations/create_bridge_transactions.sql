CREATE TABLE IF NOT EXISTS bridge_transactions (
    id SERIAL PRIMARY KEY,
    tx_hash VARCHAR(66) NOT NULL UNIQUE,
    source_chain VARCHAR(20) NOT NULL,
    target_chain VARCHAR(20) NOT NULL,
    sender VARCHAR(66) NOT NULL,
    receiver TEXT NOT NULL,
    amount TEXT NOT NULL,
    token_id BIGINT NOT NULL,
    status JSONB NOT NULL,
    timestamp BIGINT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_bridge_tx_hash ON bridge_transactions(tx_hash);
CREATE INDEX idx_bridge_sender ON bridge_transactions(sender);
CREATE INDEX idx_bridge_source_target ON bridge_transactions(source_chain, target_chain); 