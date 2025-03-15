CREATE TABLE products (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255),
    price DECIMAL,
    type VARCHAR(50)
);

CREATE TABLE proxies (
    hwid VARCHAR(255) PRIMARY KEY,
    ip VARCHAR(45),
    country VARCHAR(50),
    username VARCHAR(255),
    password VARCHAR(255)
);

CREATE TABLE nodes (
    hwid VARCHAR(255) PRIMARY KEY,
    available_bandwidth BIGINT,
    price DECIMAL,
    active BOOLEAN
);

CREATE TABLE missions (
    id SERIAL PRIMARY KEY,
    description TEXT,
    reward DECIMAL,
    completed BOOLEAN DEFAULT FALSE,
    wallet VARCHAR(255)
);

CREATE TABLE mining_wallets (
    wallet VARCHAR(255) PRIMARY KEY,
    balance DECIMAL,
    last_mined TIMESTAMP
);