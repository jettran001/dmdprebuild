/// Transfer DMD token on Solana
pub async fn transfer_to_solana(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
    info!("Starting DMD transfer on Solana: {} -> {}, amount: {}", 
        private_key_to_public_address(private_key)?, to_address, amount);
    
    // Check input parameters
    if private_key.is_empty() {
        return Err(anyhow!("Private key cannot be empty"));
    }
    
    if to_address.is_empty() {
        return Err(anyhow!("Recipient address cannot be empty"));
    }
    
    // Check Solana address
    const BASE58_CHARS: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    if !to_address.chars().all(|c| BASE58_CHARS.contains(c)) || to_address.len() < 32 || to_address.len() > 44 {
        return Err(anyhow!("Invalid Solana address: {}", to_address));
    }
    
    // Get sender's address from private key
    let sender_address = private_key_to_public_address(private_key)?;
    
    // Get Solana provider configuration from cache or create new
    let solana_config = self.get_chain_config(DmdChain::Solana)?;
    
    // Initialize SolanaContractProvider
    let solana_provider = SolanaContractProvider::new(Some(SolanaContractConfig {
        rpc_url: solana_config.rpc_url.clone(),
        token_mint: solana_config.token_address.clone(),
        timeout_ms: solana_config.timeout_ms,
        ..Default::default()
    })).await?;
    
    // Check sender's DMD token balance before transfer
    let sender_balance = solana_provider.get_token_balance(&sender_address).await?;
    if sender_balance < amount {
        return Err(anyhow!("Insufficient DMD token balance for transfer. Available: {}, Required: {}", 
            sender_balance, amount));
    }
    
    // Check sender's SOL balance for transaction fees
    let sol_balance = solana_provider.get_sol_balance(&sender_address).await?;
    let min_sol_required = solana_provider.get_minimum_sol_for_transaction().await?;
    if sol_balance < min_sol_required {
        return Err(anyhow!("Insufficient SOL for transaction fees. Available: {}, Required: {}", 
            sol_balance, min_sol_required));
    }
    
    // Convert token amount from U256 (ethers) to u64 (Solana)
    let amount_u64 = match u64::try_from(amount) {
        Ok(amt) => amt,
        Err(_) => return Err(anyhow!("Token amount too large for Solana")),
    };
    
    // Call transfer function of SolanaContractProvider
    let tx_hash = solana_provider.transfer(private_key, to_address, amount_u64).await?;
    
    info!("Solana transaction successful: {}", tx_hash);
    Ok(tx_hash)
}

/// Transfer DMD token on NEAR
pub async fn transfer_to_near(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
    info!("Starting DMD transfer on NEAR: {} -> {}, amount: {}", 
        mask_private_key(private_key), to_address, amount);
    
    // Check input parameters
    if private_key.is_empty() {
        return Err(anyhow!("Private key cannot be empty"));
    }
    
    if to_address.is_empty() {
        return Err(anyhow!("Recipient address cannot be empty"));
    }
    
    // Check NEAR address
    if !to_address.ends_with(".near") && !to_address.ends_with(".testnet") && !to_address.contains(".") {
        warn!("NEAR address does not follow standard format: {}", to_address);
    }
    
    // Get NEAR provider configuration from cache or create new
    let near_config = self.get_chain_config(DmdChain::Near)?;
    
    // Initialize NearContractProvider
    let near_provider = NearContractProvider::new(Some(NearContractConfig {
        rpc_url: near_config.rpc_url.clone(),
        contract_id: near_config.token_address.clone(),
        network_id: if near_config.rpc_url.contains("testnet") { "testnet" } else { "mainnet" }.to_string(),
        timeout_ms: near_config.timeout_ms,
    })).await?;
    
    // Call transfer function of NearContractProvider
    let tx_hash = near_provider.transfer(private_key, to_address, amount).await?;
    
    info!("NEAR transaction successful: {}", tx_hash);
    Ok(tx_hash)
}

/// Transfer DMD token on Avalanche
pub async fn transfer_to_avalanche(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
    info!("Starting DMD transfer on Avalanche: {} -> {}, amount: {}", 
        mask_private_key(private_key), to_address, amount);
    
    // Check input parameters
    if private_key.is_empty() {
        return Err(anyhow!("Private key cannot be empty"));
    }
    
    if to_address.is_empty() {
        return Err(anyhow!("Recipient address cannot be empty"));
    }
    
    // Check Avalanche address
    if !to_address.starts_with("0x") || to_address.len() != 42 {
        return Err(anyhow!("Invalid Avalanche address: {}", to_address));
    }
    
    // Get Avalanche chain configuration
    let avalanche_config = self.get_chain_config(DmdChain::Avalanche)?;
    
    // Create HTTP provider for Avalanche
    let provider = Provider::<Http>::try_from(&avalanche_config.rpc_url)?;
    let provider = Arc::new(provider);

    // Create wallet from private key
    let wallet = LocalWallet::from_str(private_key.trim_start_matches("0x"))?;
    let from_address = wallet.address();
    
    // DMD token contract address on Avalanche
    let contract_address = Address::from_str(&avalanche_config.token_address)?;
    
    // Create client middleware
    let client = SignerMiddleware::new(provider, wallet);
    
    // Create payload for transfer(address,uint256) function
    let function_signature = "transfer(address,uint256)";
    let function_selector = &ethers::utils::keccak256(function_signature.as_bytes())[0..4];
    
    let mut data = Vec::from(function_selector);
    
    // Encode recipient address (padded to 32 bytes)
    let to = Address::from_str(to_address)?;
    let mut to_bytes = [0u8; 32];
    to_bytes[12..].copy_from_slice(&to.as_bytes());
    data.extend_from_slice(&to_bytes);
    
    // Encode token amount (padded to 32 bytes)
    let amount_bytes = amount.to_be_bytes();
    data.extend_from_slice(&amount_bytes);
    
    // Create and send transaction
    let tx = TransactionRequest::new()
        .to(contract_address)
        .data(data)
        .from(from_address);
    
    let pending_tx = client.send_transaction(tx, None).await?;
    let receipt = pending_tx.await?
        .ok_or_else(|| anyhow!("No receipt received"))?;
    
    let tx_hash = format!("0x{:x}", receipt.transaction_hash);
    info!("Avalanche transaction successful: {}", tx_hash);
    
    Ok(tx_hash)
}

/// Transfer DMD token on Polygon
pub async fn transfer_to_polygon(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
    info!("Starting DMD transfer on Polygon: {} -> {}, amount: {}", 
        mask_private_key(private_key), to_address, amount);
    
    // Check input parameters
    if private_key.is_empty() {
        return Err(anyhow!("Private key cannot be empty"));
    }
    
    if to_address.is_empty() {
        return Err(anyhow!("Recipient address cannot be empty"));
    }
    
    // Check Polygon address
    if !to_address.starts_with("0x") || to_address.len() != 42 {
        return Err(anyhow!("Invalid Polygon address: {}", to_address));
    }
    
    // Get Polygon chain configuration
    let polygon_config = self.get_chain_config(DmdChain::Polygon)?;
    
    // Create HTTP provider for Polygon
    let provider = Provider::<Http>::try_from(&polygon_config.rpc_url)?;
    let provider = Arc::new(provider);
    
    // Create wallet from private key
    let wallet = LocalWallet::from_str(private_key.trim_start_matches("0x"))?;
    let from_address = wallet.address();
    
    // DMD token contract address on Polygon
    let contract_address = Address::from_str(&polygon_config.token_address)?;
    
    // Check sender's token balance
    let contract = Contract::new(
        contract_address,
        include_bytes!("../abi/erc20_abi.json").to_vec(),
        provider.clone()
    );
    
    let balance: U256 = contract.method("balanceOf", from_address)?.call().await?;
    if balance < amount {
        return Err(anyhow!("Insufficient token balance: {} < {}", balance, amount));
    }
    
    // Create client middleware
    let client = SignerMiddleware::new(provider.clone(), wallet);
    
    // Check current gas price to optimize transaction fee
    let gas_price = client.get_gas_price().await?;
    let max_priority_fee = U256::from(30_000_000_000u64); // 30 Gwei
    
    // Get current nonce for the sender
    let nonce = provider.get_transaction_count(from_address, None).await?;
    
    // Create payload for transfer(address,uint256) function
    let function_signature = "transfer(address,uint256)";
    let function_selector = &ethers::utils::keccak256(function_signature.as_bytes())[0..4];
    
    let mut data = Vec::from(function_selector);
    
    // Encode recipient address (padded to 32 bytes)
    let to = Address::from_str(to_address)?;
    let mut to_bytes = [0u8; 32];
    to_bytes[12..].copy_from_slice(&to.as_bytes());
    data.extend_from_slice(&to_bytes);
    
    // Encode token amount (padded to 32 bytes)
    let amount_bytes = amount.to_be_bytes();
    data.extend_from_slice(&amount_bytes);
    
    // Create and send transaction with custom gas parameters and explicit nonce
    let tx = TransactionRequest::new()
        .to(contract_address)
        .data(data)
        .gas_price(gas_price)
        .nonce(nonce)
        .from(from_address);
    
    // Try to send the transaction, with retry mechanism for nonce conflicts
    let mut current_attempt = 0;
    const MAX_ATTEMPTS: usize = 3;
    let mut pending_tx = None;
    
    while current_attempt < MAX_ATTEMPTS {
        match client.send_transaction(tx.clone(), None).await {
            Ok(tx) => {
                pending_tx = Some(tx);
                break;
            },
            Err(err) => {
                let err_string = err.to_string().to_lowercase();
                
                // Check if error is related to nonce
                if err_string.contains("nonce too low") || 
                   err_string.contains("transaction underpriced") || 
                   err_string.contains("already known") {
                    
                    current_attempt += 1;
                    
                    if current_attempt < MAX_ATTEMPTS {
                        // Get updated nonce
                        let new_nonce = provider.get_transaction_count(from_address, None).await?;
                        
                        // Update gas price slightly to avoid "transaction underpriced" error
                        let new_gas_price = gas_price + gas_price / 10; // Increase by 10%
                        
                        // Create new transaction with updated nonce and gas price
                        let updated_tx = tx.clone()
                            .nonce(new_nonce)
                            .gas_price(new_gas_price);
                        
                        info!("Retrying with updated nonce {} and gas price {}", new_nonce, new_gas_price);
                        
                        // Small delay before retry
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        
                        // Update tx for next iteration
                        let tx = updated_tx;
                    } else {
                        return Err(anyhow!("Failed to send transaction after {} attempts: {}", MAX_ATTEMPTS, err));
                    }
                } else {
                    // For other errors, no retry
                    return Err(anyhow!("Transaction error: {}", err));
                }
            }
        }
    }
    
    let pending_tx = pending_tx.ok_or_else(|| anyhow!("Failed to send transaction"))?;
    
    // Wait for transaction with timeout
    let receipt = match tokio::time::timeout(
        tokio::time::Duration::from_secs(180), // 3 minute timeout
        pending_tx.await
    ).await {
        Ok(result) => match result {
            Ok(Some(receipt)) => receipt,
            Ok(None) => return Err(anyhow!("No receipt received")),
            Err(err) => return Err(anyhow!("Error waiting for receipt: {}", err)),
        },
        Err(_) => {
            // Timeout occurred, return transaction hash for user to check later
            info!("Timeout waiting for transaction confirmation. Transaction hash: 0x{:x}", 
                pending_tx.tx_hash());
            return Ok(format!("0x{:x} (pending)", pending_tx.tx_hash()));
        }
    };
    
    // Check transaction status
    if receipt.status != Some(1.into()) {
        return Err(anyhow!("Transaction failed: 0x{:x}", receipt.transaction_hash));
    }
    
    let tx_hash = format!("0x{:x}", receipt.transaction_hash);
    info!("Polygon transaction successful: {}", tx_hash);
    
    Ok(tx_hash)
}

/// Transfer DMD token on Arbitrum
pub async fn transfer_to_arbitrum(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
    info!("Starting DMD transfer on Arbitrum: {} -> {}, amount: {}", 
        mask_private_key(private_key), to_address, amount);
    
    // Check input parameters
    if private_key.is_empty() {
        return Err(anyhow!("Private key cannot be empty"));
    }
    
    if to_address.is_empty() {
        return Err(anyhow!("Recipient address cannot be empty"));
    }
    
    // Check Arbitrum address
    if !to_address.starts_with("0x") || to_address.len() != 42 {
        return Err(anyhow!("Invalid Arbitrum address: {}", to_address));
    }
    
    // Get Arbitrum chain configuration
    let arbitrum_config = self.get_chain_config(DmdChain::Arbitrum)?;
    
    // Create HTTP provider for Arbitrum
    let provider = Provider::<Http>::try_from(&arbitrum_config.rpc_url)?;
    let provider = Arc::new(provider);
    
    // Create wallet from private key
    let wallet = LocalWallet::from_str(private_key.trim_start_matches("0x"))?;
    let from_address = wallet.address();
    
    // DMD token contract address on Arbitrum
    let contract_address = Address::from_str(&arbitrum_config.token_address)?;
    
    // Create client middleware
    let client = SignerMiddleware::new(provider.clone(), wallet);
    
    // Get current gas price
    let gas_price = provider.get_gas_price().await?;
    
    // Create payload for transfer(address,uint256) function
    let function_signature = "transfer(address,uint256)";
    let function_selector = &ethers::utils::keccak256(function_signature.as_bytes())[0..4];
    
    let mut data = Vec::from(function_selector);
    
    // Encode recipient address (padded to 32 bytes)
    let to = Address::from_str(to_address)?;
    let mut to_bytes = [0u8; 32];
    to_bytes[12..].copy_from_slice(&to.as_bytes());
    data.extend_from_slice(&to_bytes);
    
    // Encode token amount (padded to 32 bytes)
    let amount_bytes = amount.to_be_bytes();
    data.extend_from_slice(&amount_bytes);
    
    // Create and send transaction
    let tx = TransactionRequest::new()
        .to(contract_address)
        .data(data)
        .gas_price(gas_price)
        .from(from_address);
    
    let pending_tx = client.send_transaction(tx, None).await?;
    let receipt = pending_tx.await?
        .ok_or_else(|| anyhow!("No receipt received"))?;
    
    let tx_hash = format!("0x{:x}", receipt.transaction_hash);
    info!("Arbitrum transaction successful: {}", tx_hash);
    
    Ok(tx_hash)
}

/// Transfer DMD token on Optimism
pub async fn transfer_to_optimism(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
    info!("Starting DMD transfer on Optimism: {} -> {}, amount: {}", 
        mask_private_key(private_key), to_address, amount);
    
    // Check input parameters
    if private_key.is_empty() {
        return Err(anyhow!("Private key cannot be empty"));
    }
    
    if to_address.is_empty() {
        return Err(anyhow!("Recipient address cannot be empty"));
    }
    
    // Check Optimism address
    if !to_address.starts_with("0x") || to_address.len() != 42 {
        return Err(anyhow!("Invalid Optimism address: {}", to_address));
    }
    
    // Get Optimism chain configuration
    let optimism_config = self.get_chain_config(DmdChain::Optimism)?;
    
    // Create HTTP provider for Optimism
    let provider = Provider::<Http>::try_from(&optimism_config.rpc_url)?;
    let provider = Arc::new(provider);
    
    // Create wallet from private key
    let wallet = LocalWallet::from_str(private_key.trim_start_matches("0x"))?;
    let from_address = wallet.address();
    
    // DMD token contract address on Optimism
    let contract_address = Address::from_str(&optimism_config.token_address)?;
    
    // Create client middleware
    let client = SignerMiddleware::new(provider.clone(), wallet);
    
    // Get current gas price
    let gas_price = provider.get_gas_price().await?;
    
    // Create payload for transfer(address,uint256) function
    let function_signature = "transfer(address,uint256)";
    let function_selector = &ethers::utils::keccak256(function_signature.as_bytes())[0..4];
    
    let mut data = Vec::from(function_selector);
    
    // Encode recipient address (padded to 32 bytes)
    let to = Address::from_str(to_address)?;
    let mut to_bytes = [0u8; 32];
    to_bytes[12..].copy_from_slice(&to.as_bytes());
    data.extend_from_slice(&to_bytes);
    
    // Encode token amount (padded to 32 bytes)
    let amount_bytes = amount.to_be_bytes();
    data.extend_from_slice(&amount_bytes);
    
    // Create and send transaction
    let tx = TransactionRequest::new()
        .to(contract_address)
        .data(data)
        .gas_price(gas_price)
        .from(from_address);
    
    let pending_tx = client.send_transaction(tx, None).await?;
    let receipt = pending_tx.await?
        .ok_or_else(|| anyhow!("No receipt received"))?;
    
    let tx_hash = format!("0x{:x}", receipt.transaction_hash);
    info!("Optimism transaction successful: {}", tx_hash);
    
    Ok(tx_hash)
}

/// Transfer DMD token on Fantom
pub async fn transfer_to_fantom(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
    info!("Starting DMD transfer on Fantom: {} -> {}, amount: {}", 
        mask_private_key(private_key), to_address, amount);
    
    // Check input parameters
    if private_key.is_empty() {
        return Err(anyhow!("Private key cannot be empty"));
    }
    
    if to_address.is_empty() {
        return Err(anyhow!("Recipient address cannot be empty"));
    }
    
    // Check Fantom address
    if !to_address.starts_with("0x") || to_address.len() != 42 {
        return Err(anyhow!("Invalid Fantom address: {}", to_address));
    }
    
    // Get Fantom chain configuration
    let fantom_config = self.get_chain_config(DmdChain::Fantom)?;
    
    // Create HTTP provider for Fantom
    let provider = Provider::<Http>::try_from(&fantom_config.rpc_url)?;
    let provider = Arc::new(provider);
    
    // Create wallet from private key
    let wallet = LocalWallet::from_str(private_key.trim_start_matches("0x"))?;
    let from_address = wallet.address();

    // DMD token contract address on Fantom
    let contract_address = Address::from_str(&fantom_config.token_address)?;
    
    // Create client middleware
    let client = SignerMiddleware::new(provider.clone(), wallet);
    
    // Get current gas price
    let gas_price = provider.get_gas_price().await?;
    
    // Create payload for transfer(address,uint256) function
    let function_signature = "transfer(address,uint256)";
    let function_selector = &ethers::utils::keccak256(function_signature.as_bytes())[0..4];
    
    let mut data = Vec::from(function_selector);
    
    // Encode recipient address (padded to 32 bytes)
    let to = Address::from_str(to_address)?;
    let mut to_bytes = [0u8; 32];
    to_bytes[12..].copy_from_slice(&to.as_bytes());
    data.extend_from_slice(&to_bytes);
    
    // Encode token amount (padded to 32 bytes)
    let amount_bytes = amount.to_be_bytes();
    data.extend_from_slice(&amount_bytes);
    
    // Create and send transaction
    let tx = TransactionRequest::new()
        .to(contract_address)
        .data(data)
        .gas_price(gas_price)
        .from(from_address);
    
    let pending_tx = client.send_transaction(tx, None).await?;
    let receipt = pending_tx.await?
        .ok_or_else(|| anyhow!("No receipt received"))?;
    
    let tx_hash = format!("0x{:x}", receipt.transaction_hash);
    info!("Fantom transaction successful: {}", tx_hash);
    
    Ok(tx_hash)
}

/// Extract public address from private key
fn private_key_to_public_address(private_key: &str) -> Result<String> {
    // Process private key
    let private_key = private_key.trim_start_matches("0x");
    
    // Create wallet from private key
    let wallet = LocalWallet::from_str(private_key)?;
    let address = wallet.address();
    
    Ok(format!("0x{:x}", address))
}

/// Mask private key when logging
fn mask_private_key(private_key: &str) -> String {
    if private_key.len() <= 10 {
        return "[private_key_too_short]".to_string();
    }
    
    let prefix = &private_key[0..4];
    let suffix = &private_key[private_key.len() - 4..];
    format!("{}...{}", prefix, suffix)
}

// Add necessary imports
use std::sync::Mutex;
use lazy_static::lazy_static;
use std::collections::HashMap;

// Create structure to track ongoing bridge transactions
lazy_static! {
    static ref ACTIVE_BRIDGE_MUTEX: Mutex<()> = Mutex::new(());
    static ref PENDING_BRIDGES: Mutex<HashMap<String, (DmdChain, DmdChain, u64)>> = Mutex::new(HashMap::new());
}

/// Bridge tokens from current blockchain to target blockchain
pub async fn bridge_tokens(&self, private_key: &str, to_address: &str, to_chain: DmdChain, amount: U256) -> Result<String> {
    // Retry parameters
    const MAX_RETRIES: usize = 3;
    const INITIAL_BACKOFF_MS: u64 = 1000;
    const MAX_BACKOFF_MS: u64 = 10000;
    
    // Check input parameters
    if private_key.is_empty() {
        return Err(anyhow!("Private key cannot be empty"));
    }
    
    if to_address.is_empty() {
        return Err(anyhow!("Recipient address cannot be empty"));
    }
    
    // Check if address is compatible with target chain
    match to_chain {
        DmdChain::Near => {
            if !to_address.ends_with(".near") && !to_address.ends_with(".testnet") && !to_address.contains(".") {
                warn!("NEAR address does not follow standard format: {}", to_address);
            }
        },
        DmdChain::Solana => {
            const BASE58_CHARS: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
            if !to_address.chars().all(|c| BASE58_CHARS.contains(c)) || to_address.len() < 32 || to_address.len() > 44 {
                return Err(anyhow!("Invalid Solana address: {}", to_address));
            }
        },
        _ => {
            // EVM-compatible blockchain
            if !to_address.starts_with("0x") || to_address.len() != 42 {
                return Err(anyhow!("Invalid EVM address: {}", to_address));
            }
        }
    }
    
    // Get configuration from current chain
    let from_chain = self.current_chain;
    let from_config = self.get_chain_config(from_chain)?;
    
    // Create unique key for each bridge transaction to avoid race condition
    let address_short = if to_address.len() > 10 {
        to_address[0..10].to_string()
    } else {
        to_address.to_string()
    };
    
    // Get sender's address
    let sender_address = private_key_to_public_address(private_key)?;
    
    // Create unique transaction ID for monitoring
    let bridge_key = format!("{}_to_{}_{}_{}",
        from_chain.name(),
        to_chain.name(),
        address_short,
        sender_address
    );
    
    // Use mutex to ensure no race condition when multiple users bridge at the same time
    // Lock mutex in a scope to release immediately after checking
    {
        let mutex_guard = ACTIVE_BRIDGE_MUTEX.lock().unwrap();
        let mut pending_bridges = PENDING_BRIDGES.lock().unwrap();
        
        // Check if there is already a bridge transaction with this key being processed
        if let Some((fc, tc, timestamp)) = pending_bridges.get(&bridge_key) {
            let elapsed_secs = (chrono::Utc::now().timestamp() as u64) - *timestamp;
            if elapsed_secs < 300 { // 5 minutes
                return Err(anyhow!("Bridge transaction from {} to {} for this address is already processing ({} seconds left)",
                    fc.name(), tc.name(), 300 - elapsed_secs));
            } else {
                // If more than 5 minutes, delete old transaction and allow new transaction
                pending_bridges.remove(&bridge_key);
            }
        }
        
        // Add new transaction to processing list
        pending_bridges.insert(bridge_key.clone(), (from_chain, to_chain, chrono::Utc::now().timestamp() as u64));
    }
    
    // Log transaction information
    info!(
        "Starting bridge DMD tokens: {} -> {}, from {} to {}, amount: {}",
        from_chain.name(), to_chain.name(), mask_private_key(private_key), to_address, amount
    );
    
    // Create bridge transaction record in database
    let bridge_record_id = self.create_bridge_transaction_record(
        &bridge_key,
        &sender_address,
        to_address,
        from_chain,
        to_chain,
        amount,
        TransactionStatus::Pending
    ).await?;
    
    // Loop with retry mechanism
    let mut last_error = None;
    let mut backoff_ms = INITIAL_BACKOFF_MS;
    
    // Flag to mark processing status
    let mut bridge_success = false;
    let mut bridge_tx_hash = String::new();
    
    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            info!("Retry bridge attempt {}/{}, wait {}ms...", attempt + 1, MAX_RETRIES, backoff_ms);
            tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;
            
            // Update bridge record status
            self.update_bridge_transaction_status(
                &bridge_record_id,
                TransactionStatus::Pending,
                Some(format!("Retry attempt {}/{}", attempt + 1, MAX_RETRIES))
            ).await?;
            
            // Increase waiting time exponentially (exponential backoff)
            // Increase waiting time exponentially (exponential backoff)
            backoff_ms = std::cmp::min(backoff_ms * 2, MAX_BACKOFF_MS);
        }
        
        let result = match (from_chain, to_chain) {
            // Bridge from BSC to other blockchains
            (DmdChain::Bsc, DmdChain::Near) => {
                match self.bridge_from_bsc_to_near(private_key, to_address, amount).await {
                    Ok(tx_hash) => Ok(tx_hash),
                    Err(e) => {
                        error!("Bridge from BSC to NEAR (attempt {}/{}): {}", attempt + 1, MAX_RETRIES, e);
                        Err(e)
                    }
                }
            },
            (DmdChain::Bsc, DmdChain::Solana) => {
                match self.bridge_from_bsc_to_solana(private_key, to_address, amount).await {
                    Ok(tx_hash) => Ok(tx_hash),
                    Err(e) => {
                        error!("Bridge from BSC to Solana (attempt {}/{}): {}", attempt + 1, MAX_RETRIES, e);
                        Err(e)
                    }
                }
            },
            (DmdChain::Bsc, chain) if chain.is_evm_compatible() => {
                match self.bridge_from_bsc_to_evm(private_key, to_address, to_chain, amount).await {
                    Ok(tx_hash) => Ok(tx_hash),
                    Err(e) => {
                        error!("Bridge from BSC to {} (attempt {}/{}): {}", to_chain.name(), attempt + 1, MAX_RETRIES, e);
                        Err(e)
                    }
                }
            },
            
            // Bridge from NEAR to other blockchains
            (DmdChain::Near, DmdChain::Bsc) => {
                match self.bridge_from_near_to_bsc(private_key, to_address, amount).await {
                    Ok(tx_hash) => Ok(tx_hash),
                    Err(e) => {
                        error!("Bridge from NEAR to BSC (attempt {}/{}): {}", attempt + 1, MAX_RETRIES, e);
                        Err(e)
                    }
                }
            },
            
            // Other bridge cases
            _ => {
                let msg = format!("Bridge from {} to {} not supported", from_chain.name(), to_chain.name());
                error!("{}", msg);
                Err(anyhow!(msg))
            }
        };
        
        match result {
            Ok(tx_hash) => {
                info!("Bridge tokens successful from {} to {}, TX: {}", from_chain.name(), to_chain.name(), tx_hash);
                
                // Save transaction hash
                bridge_success = true;
                bridge_tx_hash = tx_hash.clone();
                
                // Create transaction monitor to track status
                self.monitor_bridge_transaction(tx_hash.clone(), from_chain, to_chain).await;
                
                break;
            },
            Err(e) => {
                // Classify error to decide whether to retry
                let should_retry = match e.to_string().to_lowercase() {
                    s if s.contains("timeout") || s.contains("timed out") => {
                        warn!("Bridge timeout, will retry");
                        true
                    },
                    s if s.contains("nonce too low") || s.contains("already known") => {
                        error!("Invalid nonce, will not retry");
                        false
                    },
                    s if s.contains("insufficient funds") || s.contains("not enough") => {
                        error!("Insufficient token/fee, will not retry");
                        false
                    },
                    s if s.contains("rate limit") || s.contains("too many requests") => {
                        warn!("Rate limit, will retry after backoff");
                        true
                    },
                    s if s.contains("connection") || s.contains("network") || s.contains("unavailable") => {
                        warn!("Connection/network error, will retry");
                        true
                    },
                    _ => {
                        warn!("Unknown error, will retry: {}", e);
                        true
                    }
                };
                
                if !should_retry || attempt == MAX_RETRIES - 1 {
                    last_error = Some(e);
                    break;
                }
                
                last_error = Some(e);
            }
        }
    }
    
    // Remove transaction from processing list
    {
        let mut pending_bridges = PENDING_BRIDGES.lock().unwrap();
        pending_bridges.remove(&bridge_key);
    }
    
    // Return result
    if bridge_success {
        Ok(bridge_tx_hash)
    } else {
        // If all attempts failed
        let err_msg = match last_error {
            Some(e) => format!("Bridge tokens failed after {} attempts: {}", MAX_RETRIES, e),
            None => format!("Bridge tokens failed after {} attempts with unknown error", MAX_RETRIES),
        };
        
        error!("{}", err_msg);
        Err(anyhow!(err_msg))
    }
}

/// Check if chain is EVM-compatible type
impl DmdChain {
    fn is_evm_compatible(&self) -> bool {
        match self {
            DmdChain::Ethereum | 
            DmdChain::Bsc | 
            DmdChain::Avalanche | 
            DmdChain::Polygon | 
            DmdChain::Arbitrum | 
            DmdChain::Optimism | 
            DmdChain::Fantom => true,
            _ => false,
        }
    }
    
    fn name(&self) -> &'static str {
        match self {
            DmdChain::Ethereum => "Ethereum",
            DmdChain::Bsc => "BSC",
            DmdChain::Solana => "Solana",
            DmdChain::Near => "NEAR",
            DmdChain::Avalanche => "Avalanche",
            DmdChain::Polygon => "Polygon",
            DmdChain::Arbitrum => "Arbitrum",
            DmdChain::Optimism => "Optimism",
            DmdChain::Fantom => "Fantom",
        }
    }
}

/// Track status of bridge transaction
async fn monitor_bridge_transaction(&self, tx_hash: String, from_chain: DmdChain, to_chain: DmdChain, record_id: String) -> anyhow::Result<()> {
    // Create background task to track bridge transaction
    tokio::spawn(async move {
        // Initial delay before checking
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        
        // Monitoring configuration
        const MAX_CHECKS: usize = 60; // Maximum number of status checks
        const CHECK_INTERVAL_SEC: u64 = 20; // Initially check every 20 seconds
        const MAX_CHECK_INTERVAL_SEC: u64 = 300; // Maximum check every 5 minutes
        const MAX_FAILURES: usize = 5; // Maximum consecutive failures before recovery
        
        let mut check_interval = CHECK_INTERVAL_SEC;
        let mut next_status_log = 0;
        let mut consecutive_failures = 0;
        
        // Get chain configuration with fallback options
        let mut from_rpc_urls = vec![
            get_chain_config(from_chain).map(|c| c.rpc_url.clone()).unwrap_or_default()
        ];
        
        // Add fallback RPC URLs based on chain type
        match from_chain {
            DmdChain::Bsc => {
                from_rpc_urls.extend_from_slice(&[
                    "https://bsc-dataseed1.binance.org".to_string(),
                    "https://bsc-dataseed2.binance.org".to_string()
                ]);
            },
            DmdChain::Ethereum => {
                from_rpc_urls.extend_from_slice(&[
                    "https://mainnet.infura.io/v3/9aa3d95b3bc440fa88ea12eaa4456161".to_string(),
                    "https://rpc.ankr.com/eth".to_string()
                ]);
            },
            DmdChain::Polygon => {
                from_rpc_urls.extend_from_slice(&[
                    "https://polygon-rpc.com".to_string(),
                    "https://rpc-mainnet.matic.network".to_string()
                ]);
            },
            DmdChain::Optimism => {
                from_rpc_urls.extend_from_slice(&[
                    "https://mainnet.optimism.io".to_string()
                ]);
            },
            DmdChain::Arbitrum => {
                from_rpc_urls.extend_from_slice(&[
                    "https://arb1.arbitrum.io/rpc".to_string()
                ]);
            },
            DmdChain::Avalanche => {
                from_rpc_urls.extend_from_slice(&[
                    "https://api.avax.network/ext/bc/C/rpc".to_string()
                ]);
            },
            DmdChain::Fantom => {
                from_rpc_urls.extend_from_slice(&[
                    "https://rpc.ftm.tools".to_string()
                ]);
            },
            _ => {}
        };
        
        let mut current_rpc_index = 0;
        
        // Also prepare fallback RPC URLs for target chain
        let mut to_rpc_urls = vec![
            get_chain_config(to_chain).map(|c| c.rpc_url.clone()).unwrap_or_default()
        ];
        
        // Add fallback RPC URLs based on target chain type
        match to_chain {
            DmdChain::Bsc => {
                to_rpc_urls.extend_from_slice(&[
                    "https://bsc-dataseed1.binance.org".to_string(),
                    "https://bsc-dataseed2.binance.org".to_string()
                ]);
            },
            DmdChain::Ethereum => {
                to_rpc_urls.extend_from_slice(&[
                    "https://mainnet.infura.io/v3/9aa3d95b3bc440fa88ea12eaa4456161".to_string(),
                    "https://rpc.ankr.com/eth".to_string()
                ]);
            },
            DmdChain::Polygon => {
                to_rpc_urls.extend_from_slice(&[
                    "https://polygon-rpc.com".to_string(),
                    "https://rpc-mainnet.matic.network".to_string()
                ]);
            },
            _ => {}
        };
        
        let mut current_to_rpc_index = 0;
        
        info!("Starting bridge transaction tracking {} (chain {} -> {})", tx_hash, from_chain.name(), to_chain.name());
        
        // Update record status to monitoring
        if let Err(err) = update_bridge_transaction_status(
            &record_id,
            TransactionStatus::Monitoring,
            Some("Started monitoring".to_string())
        ).await {
            error!("Failed to update transaction status: {}", err);
        }
        
        // Current transaction status
        let mut transaction_status = TransactionStatus::Pending;
        let mut current_check = 0;
        let start_time = tokio::time::Instant::now();
        
        // Store information for successful bridge
        let mut target_tx_hash: Option<String> = None;
        
        // Counter for bridge check attempts
        let mut bridge_check_retries = 0;
        const MAX_BRIDGE_RETRIES: usize = 10;
        
        // Loop to check transaction status
        loop {
            current_check += 1;
            
            // Get current RPC URL
            let current_rpc = &from_rpc_urls[current_rpc_index];
            
            // Check transaction status from source chain
            let check_result = match from_chain {
                DmdChain::Bsc => {
                    verify_bsc_transaction_status(&tx_hash, current_rpc).await
                },
                DmdChain::Near => {
                    verify_near_transaction_status(&tx_hash, current_rpc).await
                },
                DmdChain::Solana => {
                    verify_solana_transaction_status(&tx_hash, current_rpc).await
                },
                _ => {
                    // Other EVM-compatible blockchain
                    verify_evm_transaction_status(&tx_hash, current_rpc).await
                }
            };
            
            match check_result {
                Ok(status) => {
                    transaction_status = status;
                    consecutive_failures = 0; // Reset failure counter on success
                },
                        Err(e) => {
                    consecutive_failures += 1;
                    warn!("Check {} failed for transaction {}: {}", current_check, tx_hash, e);
                    
                    // Implement recovery mechanism when multiple consecutive checks fail
                    if consecutive_failures >= MAX_FAILURES {
                        // Try switching to alternative RPC endpoint
                        if current_rpc_index + 1 < from_rpc_urls.len() {
                            current_rpc_index += 1;
                            info!("Switching to fallback RPC for {}: {}", from_chain.name(), from_rpc_urls[current_rpc_index]);
                            consecutive_failures = 0; // Reset counter after switching
                        } else {
                            // If all fallback RPCs have been tried, go back to the first one
                            if from_rpc_urls.len() > 1 {
                                current_rpc_index = 0;
                                info!("Tried all fallback RPCs, returning to primary RPC");
                                consecutive_failures = 0;
                            }
                        }
                    }
                }
            };
            
            // Check if transaction has been confirmed on source chain
            if transaction_status == TransactionStatus::Confirmed {
                info!("Transaction {} confirmed on source chain {}!", tx_hash, from_chain.name());
                
                // Update record status
                if let Err(err) = update_bridge_transaction_status(
                    &record_id,
                    TransactionStatus::Confirmed,
                    Some("Transaction confirmed on source chain".to_string())
                ).await {
                    error!("Failed to update transaction status: {}", err);
                }
                
                // Wait for LayerZero bridge to process, estimate waiting time
                let estimated_bridge_time = match (from_chain, to_chain) {
                    (DmdChain::Bsc, DmdChain::Near) => 180, // ~3 minutes
                    (DmdChain::Bsc, DmdChain::Solana) => 240, // ~4 minutes
                    (DmdChain::Bsc, DmdChain::Polygon) => 160, // ~2.5 minutes
                    (DmdChain::Polygon, DmdChain::Bsc) => 160, // ~2.5 minutes
                    (DmdChain::Ethereum, _) => 300, // ~5 minutes
                    (_, DmdChain::Ethereum) => 300, // ~5 minutes
                    _ => 300, // ~5 minutes for other cases
                };
                
                info!("Waiting for bridge completion (estimated time: {} seconds)...", estimated_bridge_time);
                
                // Wait for bridge completion with periodic checks
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                
                // Check bridge completion with retry and recovery
                let mut bridge_check_count = 0;
                let mut bridge_recovery_attempts = 0;
                const MAX_BRIDGE_CHECKS: usize = 10;
                const MAX_BRIDGE_RECOVERY: usize = 3;
                
                while bridge_check_count < MAX_BRIDGE_CHECKS {
                    bridge_check_count += 1;
                    
                    // Get current target chain RPC URL
                    let current_to_rpc = &to_rpc_urls[current_to_rpc_index];
                    
                    // Check bridge events completed on target chain
                    let check_result = match to_chain {
                        DmdChain::Bsc => {
                            match check_bsc_bridge_completion(tx_hash.clone(), current_to_rpc).await {
                                result @ (true, _) => result,
                                _ => {
                                    // Try alternative method on failure
                                    check_bsc_bridge_completion_alt(tx_hash.clone(), current_to_rpc).await
                                }
                            }
                        },
                        DmdChain::Near => {
                            match check_near_bridge_completion(tx_hash.clone(), current_to_rpc).await {
                                result @ (true, _) => result,
                                _ => {
                                    // Try alternative method on failure
                                    check_near_bridge_completion_alt(tx_hash.clone(), current_to_rpc).await
                                }
                            }
                        },
                        DmdChain::Polygon => {
                            match check_polygon_bridge_completion(tx_hash.clone(), current_to_rpc).await {
                                result @ (true, _) => result,
                                _ => {
                                    // Try alternative method on failure
                                    check_polygon_bridge_completion_alt(tx_hash.clone(), current_to_rpc).await
                                }
                            }
                        },
                        DmdChain::Solana => {
                            match check_solana_bridge_completion(tx_hash.clone(), current_to_rpc).await {
                                result @ (true, _) => result,
                                _ => {
                                    // Try alternative method on failure
                                    check_solana_bridge_completion_alt(tx_hash.clone(), current_to_rpc).await
                                }
                            }
                        },
                        _ => {
                            // Default to standard EVM implementation with fallback
                            match check_evm_bridge_completion(tx_hash.clone(), current_to_rpc).await {
                                result @ (true, _) => result,
                                _ => (false, None)
                            }
                        },
                    };
                    
                    if check_result.0 {
                        // Bridge completed successfully
                        target_tx_hash = check_result.1.clone();
                        
                        info!("Bridge from {} to {} completed! Target tx: {}", 
                            from_chain.name(), to_chain.name(), 
                            target_tx_hash.clone().unwrap_or_else(|| "unknown".to_string()));
                        
                        // Update record status
                        if let Err(err) = update_bridge_transaction_status(
                            &record_id,
                            TransactionStatus::Completed,
                            target_tx_hash.clone().map(|tx| format!("Target tx: {}", tx))
                        ).await {
                            error!("Failed to update transaction status: {}", err);
                        }
                        
                        // Notify bridge success
                        notify_bridge_success(tx_hash.clone(), from_chain, to_chain, target_tx_hash.clone()).await;
                        break;
                    } else if bridge_check_count >= MAX_BRIDGE_CHECKS {
                        // If all bridge checks failed
                        warn!("Bridge completion not confirmed after {} checks", MAX_BRIDGE_CHECKS);
                        
                        // Try recovery procedure: switch RPC endpoint first
                        if current_to_rpc_index + 1 < to_rpc_urls.len() {
                            current_to_rpc_index += 1;
                            info!("Switching to fallback RPC for target chain {}: {}", 
                                to_chain.name(), to_rpc_urls[current_to_rpc_index]);
                            
                            // Reset bridge check count to try again with new RPC endpoint
                            bridge_check_count = 0;
                            continue;
                        }
                        
                        // All RPC endpoints tried, now try manual verification
                        if bridge_recovery_attempts < MAX_BRIDGE_RECOVERY {
                            bridge_recovery_attempts += 1;
                            info!("Attempting manual bridge verification (attempt {}/{})", 
                                bridge_recovery_attempts, MAX_BRIDGE_RECOVERY);
                            
                            // Manual verification logic:
                            // 1. Look for specific bridge events on target chain
                            let manual_check = verify_bridge_completion_manual(
                                &tx_hash, 
                                from_chain, 
                                to_chain, 
                                &to_rpc_urls[0]
                            ).await;
                            
                            match manual_check {
                                Ok((true, Some(tx))) => {
                                    // Manual verification successful
                                    target_tx_hash = Some(tx);
                                    info!("Manual bridge verification successful! Target tx: {}", target_tx_hash.clone().unwrap());
                                    
                                    // Update record status
                                    if let Err(err) = update_bridge_transaction_status(
                                        &record_id,
                                        TransactionStatus::Completed,
                                        target_tx_hash.clone().map(|tx| format!("Target tx (manual): {}", tx))
                                    ).await {
                                        error!("Failed to update transaction status: {}", err);
                                    }
                                    
                                    // Notify bridge success
                                    notify_bridge_success(tx_hash.clone(), from_chain, to_chain, target_tx_hash.clone()).await;
                                    break;
                                },
                                Ok((true, None)) => {
                                    // Confirmed bridge but no target tx hash
                                    info!("Manual bridge verification confirmed completion but couldn't determine target tx hash");
                                    
                                    // Update record status
                                    if let Err(err) = update_bridge_transaction_status(
                                        &record_id,
                                        TransactionStatus::Completed,
                                        Some("Bridge completed (manual verification)".to_string())
                                    ).await {
                                        error!("Failed to update transaction status: {}", err);
                                    }
                                    
                                    notify_bridge_success(tx_hash.clone(), from_chain, to_chain, None).await;
                                    break;
                                },
                                Ok((false, _)) => {
                                    // Manual verification couldn't confirm bridge
                                    warn!("Manual bridge verification attempt {} could not confirm bridge completion", 
                                        bridge_recovery_attempts);
                                    
                                    // Try a longer delay before next attempt
                                    tokio::time::sleep(tokio::time::Duration::from_secs(90)).await;
                                    
                                    // Reset bridge check count to try again
                                    bridge_check_count = 0;
                                },
                                Err(e) => {
                                    // Error in manual verification
                                    error!("Manual bridge verification failed: {}", e);
                                    
                                    // Try again with delay
                                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                                    
                                    if bridge_recovery_attempts >= MAX_BRIDGE_RECOVERY {
                                        warn!("Bridge recovery failed after {} attempts", MAX_BRIDGE_RECOVERY);
                                        
                                        // Update record status
                                        if let Err(err) = update_bridge_transaction_status(
                                            &record_id,
                                            TransactionStatus::Inconclusive,
                                            Some("Bridge state uncertain, manual verification required".to_string())
                                        ).await {
                                            error!("Failed to update transaction status: {}", err);
                                        }
                                        
                                        break;
                                    }
                                }
                            }
                        } else {
                            warn!("Bridge recovery failed after {} attempts", MAX_BRIDGE_RECOVERY);
                            
                            // Update record status
                            if let Err(err) = update_bridge_transaction_status(
                                &record_id,
                                TransactionStatus::Inconclusive,
                                Some("Bridge state uncertain, manual verification required".to_string())
                            ).await {
                                error!("Failed to update transaction status: {}", err);
                            }
                            
                            break;
                        }
                    }
                    
                    // Wait before checking again
                    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                }
                
                // Bridge monitoring complete
                break;
            } else if transaction_status == TransactionStatus::Failed {
                error!("Bridge transaction {} failed on source chain {}!", tx_hash, from_chain.name());
                
                // Update record status
                if let Err(err) = update_bridge_transaction_status(
                    &record_id,
                    TransactionStatus::Failed,
                    Some("Transaction failed on source chain".to_string())
                ).await {
                    error!("Failed to update transaction status: {}", err);
                }
                
                break;
            }
            
            // Check if we've exceeded the maximum number of checks
            if current_check >= MAX_CHECKS {
                warn!("Exceeded maximum check times ({}) for bridge transaction {}", MAX_CHECKS, tx_hash);
                
                // Check if transaction is still pending and already visible on target chain
                if transaction_status == TransactionStatus::Pending {
                    bridge_check_retries += 1;
                    
                    if bridge_check_retries <= MAX_BRIDGE_RETRIES {
                        info!("Checking if transaction {} is already visible on target chain ({}) despite pending status (retry {}/{})", 
                              tx_hash, to_chain.name(), bridge_check_retries, MAX_BRIDGE_RETRIES);
                        
                        // Get current target chain RPC URL
                        let current_to_rpc = &to_rpc_urls[current_to_rpc_index];
                        
                        // Check for transaction on target chain directly
                        let target_check = match to_chain {
                            DmdChain::Bsc => check_bsc_bridge_completion(tx_hash.clone(), current_to_rpc).await,
                            DmdChain::Near => check_near_bridge_completion(tx_hash.clone(), current_to_rpc).await,
                            DmdChain::Polygon => check_polygon_bridge_completion(tx_hash.clone(), current_to_rpc).await,
                            DmdChain::Solana => check_solana_bridge_completion(tx_hash.clone(), current_to_rpc).await,
                            _ => (false, None),
                        };
                        
                        if target_check.0 {
                            // Found on target chain despite pending on source chain
                            info!("Bridge transaction found on target chain {} despite pending status on source chain!", to_chain.name());
                            
                            // Update record status
                            if let Err(err) = update_bridge_transaction_status(
                                &record_id,
                                TransactionStatus::Completed,
                                target_check.1.map(|tx| format!("Target tx: {}", tx))
                            ).await {
                                error!("Failed to update transaction status: {}", err);
                            }
                            
                            // Notify success
                            notify_bridge_success(tx_hash.clone(), from_chain, to_chain, target_check.1).await;
                break;
            }
            
                        // Try with next RPC endpoint if available
                        if current_to_rpc_index + 1 < to_rpc_urls.len() {
                            current_to_rpc_index += 1;
                            current_check = 0; // Reset check counter to continue monitoring
                            continue;
                        }
                        
                        // If all RPC endpoints tried and MAX_BRIDGE_RETRIES not reached, continue with first RPC
                        if bridge_check_retries < MAX_BRIDGE_RETRIES {
                            current_to_rpc_index = 0;
                            current_check = 0; // Reset check counter to continue monitoring
                            continue;
                        }
                    }
                }
                
                // All recovery attempts failed, mark as timeout
                if let Err(err) = update_bridge_transaction_status(
                    &record_id,
                    TransactionStatus::Timeout,
                    Some(format!("Exceeded maximum check times ({})", MAX_CHECKS))
                ).await {
                    error!("Failed to update transaction status: {}", err);
                }
                
                break;
            }
            
            // Log status periodically
            let now = tokio::time::Instant::now();
            let elapsed = (now - start_time).as_secs();
            
            if elapsed >= next_status_log {
                info!(
                    "Bridge {} ({} -> {}): {} - check {}/{}, elapsed {} sec",
                    tx_hash, from_chain.name(), to_chain.name(), 
                    transaction_status.as_str(), current_check, MAX_CHECKS, elapsed
                );
                
                next_status_log = elapsed + 60; // Log every 60 seconds
            }
            
            // Increase check interval (exponential backoff)
            if check_interval < MAX_CHECK_INTERVAL_SEC {
                check_interval = std::cmp::min(check_interval * 2, MAX_CHECK_INTERVAL_SEC);
            }
            
            // Wait for next check
            tokio::time::sleep(tokio::time::Duration::from_secs(check_interval)).await;
        }
    });
    
    Ok(())
}

/// Alternative methods for bridge completion verification
async fn check_bsc_bridge_completion_alt(source_tx: String, rpc_url: &str) -> (bool, Option<String>) {
    // Alternative method using different event or API
    // This is a fallback in case the primary method fails
    (false, None) // Placeholder implementation
}

async fn check_near_bridge_completion_alt(source_tx: String, rpc_url: &str) -> (bool, Option<String>) {
    // Alternative method for NEAR
    (false, None) // Placeholder implementation
}

async fn check_polygon_bridge_completion_alt(source_tx: String, rpc_url: &str) -> (bool, Option<String>) {
    // Alternative method for Polygon
    (false, None) // Placeholder implementation
}

async fn check_solana_bridge_completion_alt(source_tx: String, rpc_url: &str) -> (bool, Option<String>) {
    // Alternative method for Solana
    (false, None) // Placeholder implementation
}

/// Manual verification of bridge completion
/// Used as a last resort when automatic methods fail
async fn verify_bridge_completion_manual(
    source_tx: &str,
    from_chain: DmdChain,
    to_chain: DmdChain,
    rpc_url: &str
) -> Result<(bool, Option<String>)> {
    // This is a more comprehensive and direct method to verify bridge completion
    // It can include:
    // 1. Direct contract queries
    // 2. Multiple event types checking
    // 3. Contract state inspection
    // 4. Bridge API calls if available
    
    // For demonstration purposes, this returns failure
    // In a real implementation, this would contain detailed logic
    Ok((false, None))
}

// Support functions to check transaction status
async fn verify_bsc_transaction_status(tx_hash: &str, rpc_url: &str) -> anyhow::Result<TransactionStatus> {
    // Call API to check transaction status
    let client = reqwest::Client::new();
    let response = client.post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionReceipt",
            "params": [tx_hash],
            "id": 1
        }))
        .send()
        .await?;
    
    let result = response.json::<serde_json::Value>().await?;
    
    // Parse result
    if let Some(receipt) = result.get("result").and_then(|r| r.as_object()) {
        if receipt.is_empty() {
            return Ok(TransactionStatus::Pending); // Transaction not yet confirmed
        }
        
        if let Some(status) = receipt.get("status").and_then(|s| s.as_str()) {
            if status == "0x1" {
                return Ok(TransactionStatus::Confirmed);
            } else {
                return Ok(TransactionStatus::Failed);
            }
        }
    }
    
    Ok(TransactionStatus::Pending)
}

async fn verify_near_transaction_status(tx_hash: &str, rpc_url: &str) -> anyhow::Result<TransactionStatus> {
    // Call API to check transaction status
    let client = reqwest::Client::new();
    let response = client.post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tx",
            "params": [tx_hash, "dmd.near"],
            "id": 1
        }))
        .send()
        .await?;
    
    let result = response.json::<serde_json::Value>().await?;
    
    // Parse result
    if let Some(error) = result.get("error") {
        let error_msg = error.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error");
        if error_msg.contains("does not exist") {
            return Ok(TransactionStatus::Pending);
        }
        return Ok(TransactionStatus::Failed);
    }
    
    if let Some(status) = result.get("result").and_then(|r| r.get("status")) {
        if status.get("SuccessValue").is_some() {
            return Ok(TransactionStatus::Confirmed);
        } else if status.get("Failure").is_some() {
            return Ok(TransactionStatus::Failed);
        }
    }
    
    Ok(TransactionStatus::Pending)
}

async fn verify_solana_transaction_status(tx_hash: &str, rpc_url: &str) -> anyhow::Result<TransactionStatus> {
    // Call API to check transaction status
    let client = reqwest::Client::new();
    let response = client.post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "getTransaction",
            "params": [
                tx_hash,
                {
                    "encoding": "json",
                    "commitment": "confirmed"
                }
            ],
            "id": 1
        }))
        .send()
        .await?;
    
    let result = response.json::<serde_json::Value>().await?;
    
    // Parse result
    if let Some(error) = result.get("error") {
        let error_msg = error.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error");
        if error_msg.contains("not found") {
            return Ok(TransactionStatus::Pending);
        }
        return Ok(TransactionStatus::Failed);
    }
    
    if let Some(tx_result) = result.get("result") {
        if tx_result.is_null() {
            return Ok(TransactionStatus::Pending);
        }
        
        if let Some(meta) = tx_result.get("meta") {
            if let Some(status) = meta.get("status").and_then(|s| s.as_object()) {
                if status.get("Ok").is_some() {
                    return Ok(TransactionStatus::Confirmed);
                } else {
                    return Ok(TransactionStatus::Failed);
                }
            }
        }
    }
    
    Ok(TransactionStatus::Pending)
}

async fn verify_evm_transaction_status(tx_hash: &str, rpc_url: &str) -> anyhow::Result<TransactionStatus> {
    // Use BSC verification function since both are EVM-compatible
    verify_bsc_transaction_status(tx_hash, rpc_url).await
}

// Bridge functions from BSC to other blockchains
async fn check_bsc_bridge_completion(source_tx: String, rpc_url: &str) -> (bool, Option<String>) {
    // Define JSON-RPC payload to query for LayerZero bridge event
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_getLogs",
        "params": [{
            "fromBlock": "0x0",  // Query from the beginning
            "toBlock": "latest",
            "address": "0x16a7FA783c2FF584B234e13825960F95244Fc7bC",  // BSC DMD token contract
            "topics": [
                "0x97c3d2190d93e82b9979b46195833b562a56e07a36faf642272a7799aafa95d3", // LayerZero MessageReceived event
                null,
                null,
                null
            ]
        }],
        "id": 1
    });
    
    // Send request to BSC node
    let client = reqwest::Client::new();
    let response = match client.post(rpc_url)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&payload).unwrap())
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await {
            Ok(resp) => resp,
            Err(e) => {
                warn!("Failed to query BSC node for bridge events: {}", e);
                return (false, None);
            }
        };
    
    // Parse response
    let response_json = match response.json::<serde_json::Value>().await {
        Ok(json) => json,
        Err(e) => {
            warn!("Failed to parse BSC node response: {}", e);
            return (false, None);
        }
    };
    
    // Extract logs from response
    let logs = match response_json.get("result") {
        Some(result) => match result.as_array() {
            Some(logs) => logs,
            None => {
                warn!("BSC bridge event result is not an array");
                return (false, None);
            }
        },
        None => {
            warn!("BSC bridge event query returned no result");
            return (false, None);
        }
    };
    
    // Check each log for our source transaction
    for log in logs {
        // Extract data from log
        let data = match log.get("data") {
            Some(data) => match data.as_str() {
                Some(data_str) => data_str,
                None => continue
            },
            None => continue
        };
        
        // Check if this log contains our source transaction hash
        // In a real implementation, would need to decode the event data
        // and match against source transaction fields
        if data.contains(&source_tx) || data.contains(&source_tx.trim_start_matches("0x")) {
            // Extract transaction hash of this event
            let target_tx = match log.get("transactionHash") {
                Some(tx) => match tx.as_str() {
                    Some(tx_str) => Some(tx_str.to_string()),
                    None => None
                },
                None => None
            };
            
            info!("Found matching BSC bridge completion event: {:?}", target_tx);
            return (true, target_tx);
        }
    }
    
    // If no direct match found, try to match by topic
    for log in logs {
        // Extract topics array
        let topics = match log.get("topics") {
            Some(topics_val) => match topics_val.as_array() {
                Some(topics_arr) => topics_arr,
                None => continue
            },
            None => continue
        };
        
        // Check if one of the topics contains our source transaction
        let source_tx_no_prefix = source_tx.trim_start_matches("0x");
        let matches_topic = topics.iter().any(|topic| {
            if let Some(topic_str) = topic.as_str() {
                topic_str.contains(source_tx_no_prefix)
    } else {
                false
            }
        });
        
        if matches_topic {
            // Extract transaction hash of this event
            let target_tx = match log.get("transactionHash") {
                Some(tx) => match tx.as_str() {
                    Some(tx_str) => Some(tx_str.to_string()),
                    None => None
                },
                None => None
            };
            
            info!("Found matching BSC bridge completion event by topic: {:?}", target_tx);
            return (true, target_tx);
        }
    }
    
    // Log additional information about query
    debug!("BSC bridge completion check for tx {} found {} logs, none matching", 
        source_tx, logs.len());
    
    // Not found in logs, try querying bridge contract state as fallback
    match check_bsc_bridge_state(&source_tx, rpc_url).await {
        Ok((found, tx_hash)) => {
            if found {
                info!("Bridge completion confirmed through contract state check: {:?}", tx_hash);
                return (true, tx_hash);
            }
        },
        Err(e) => {
            warn!("Error checking bridge contract state: {}", e);
        }
    }
    
    // Still not found after checking logs and contract state
        (false, None)
    }

// Fallback function to check bridge completion by inspecting contract state
async fn check_bsc_bridge_state(source_tx: &str, rpc_url: &str) -> Result<(bool, Option<String>)> {
    // This is a more direct method that queries the bridge contract's state
    // to determine if a cross-chain transaction has completed
    
    // Create payload to call the bridge contract's getBridgeStatus method
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{
            "to": "0x16a7FA783c2FF584B234e13825960F95244Fc7bC", // BSC DMD token contract
            "data": format!("0x4ac1a413{}", source_tx.trim_start_matches("0x")) // getBridgeStatus(bytes32)
        }, "latest"],
        "id": 1
    });
    
    // Send request to BSC node
    let client = reqwest::Client::new();
    let response = client.post(rpc_url)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&payload)?)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;
    
    let response_json = response.json::<serde_json::Value>().await?;
    
    // Extract result
    if let Some(result) = response_json.get("result").and_then(|r| r.as_str()) {
        // Parse the contract's response
        // For this example, we'll assume the contract returns "0x0000...0001" for completed bridges
        if result.ends_with("01") {
            // Bridge completed, but we don't have target tx hash from this method
            return Ok((true, None));
        }
    }
    
    // Bridge not completed according to contract state
    Ok((false, None))
}

async fn check_near_bridge_completion(source_tx: String, rpc_url: &str) -> (bool, Option<String>) {
    // Use BSC completion check function
    check_bsc_bridge_completion(source_tx, rpc_url).await
}

async fn check_polygon_bridge_completion(source_tx: String, rpc_url: &str) -> (bool, Option<String>) {
    // Use BSC completion check function
    check_bsc_bridge_completion(source_tx, rpc_url).await
}

async fn check_solana_bridge_completion(source_tx: String, rpc_url: &str) -> (bool, Option<String>) {
    // Use BSC completion check function
    check_bsc_bridge_completion(source_tx, rpc_url).await
}

// Notify bridge success function
async fn notify_bridge_success(source_tx: String, from_chain: DmdChain, to_chain: DmdChain, target_tx: Option<String>) {
    // Actual implementation needed for notifying bridge success
    // In actual implementation, could send email, webhook or update database
    info!(
        "Bridge successful: {} -> {}, TX source: {}, TX target: {}",
        from_chain.name(),
        to_chain.name(),
        source_tx,
        target_tx.unwrap_or_else(|| "unknown".to_string())
    );
}

/// Bridge from BSC to NEAR
async fn bridge_from_bsc_to_near(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
    let bsc_bridge = self.get_bsc_bridge().await?;
    bsc_bridge.bridge_tokens(private_key, to_address, amount).await
}

/// Bridge from BSC to Solana
async fn bridge_from_bsc_to_solana(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
    // Currently not supported directly from BSC to Solana
    // Need to implement through an intermediary bridge
    Err(anyhow!("Bridge from BSC to Solana not supported directly"))
}

/// Bridge from BSC to other EVM-compatible blockchains
async fn bridge_from_bsc_to_evm(&self, private_key: &str, to_address: &str, to_chain: DmdChain, amount: U256) -> Result<String> {
    let bsc_provider = self.get_bsc_provider().await?;
    
    // Convert DmdChain to LayerZero chain ID
    let lz_chain_id = match to_chain {
        DmdChain::Ethereum => 101,
        DmdChain::Avalanche => 106,
        DmdChain::Polygon => 109,
        DmdChain::Arbitrum => 110,
        DmdChain::Optimism => 111,
        DmdChain::Fantom => 112,
        _ => return Err(anyhow!("Bridge to chain {} not supported", to_chain.name())),
    };
    
    // Estimate bridge fee
    let bridge_fee = bsc_provider.estimate_bridge_fee(to_chain, to_address).await?;
    
    // Create wallet from private key
    let wallet = LocalWallet::from_str(private_key.trim_start_matches("0x"))?;
    let from_address = wallet.address();
    
    // DMD token contract address on BSC
    let config = self.get_chain_config(DmdChain::Bsc)?;
    let contract_address = Address::from_str(&config.token_address)?;
    
    // Create HTTP provider for BSC
    let provider = Provider::<Http>::try_from(&config.rpc_url)?;
    let provider = Arc::new(provider);
    
    // Create client middleware
    let client = SignerMiddleware::new(provider.clone(), wallet);
    
    // Convert EVM address to bytes
    let to_address_bytes = to_address.trim_start_matches("0x").as_bytes();
    
    // Create payload for bridgeToOtherChain function
    let function_signature = "bridgeToOtherChain(uint16,bytes,uint256,uint256)";
    let function_selector = &ethers::utils::keccak256(function_signature.as_bytes())[0..4];
    
    let mut data = Vec::from(function_selector);
    
    // Add parameters:
    // 1. LayerZero chain ID
    let mut chain_id_bytes = [0u8; 32];
    chain_id_bytes[30..32].copy_from_slice(&lz_chain_id.to_be_bytes());
    data.extend_from_slice(&chain_id_bytes);
    
    // 2. Offset for bytes toAddress
    let mut offset_bytes = [0u8; 32];
    offset_bytes[31] = 0x80; // offset = 128 bytes
    data.extend_from_slice(&offset_bytes);
    
    // 3. Token ID (0 for fungible token)
    let mut token_id_bytes = [0u8; 32];
    data.extend_from_slice(&token_id_bytes);
    
    // 4. Amount
    let amount_bytes = amount.to_be_bytes();
    data.extend_from_slice(&amount_bytes);
    
    // 5. Length of to_address_bytes
    let mut length_bytes = [0u8; 32];
    length_bytes[31] = to_address_bytes.len() as u8;
    data.extend_from_slice(&length_bytes);
    
    // 6. to_address_bytes (padded)
    let padding_size = (32 - (to_address_bytes.len() % 32)) % 32;
    data.extend_from_slice(to_address_bytes);
    data.extend_from_slice(&vec![0u8; padding_size]);
    
    // Create and send transaction
    let tx = TransactionRequest::new()
        .to(contract_address)
        .data(data)
        .value(bridge_fee)
        .from(from_address);
    
    // Send transaction with timeout and auto gas features
    let pending_tx = match tokio::time::timeout(
        tokio::time::Duration::from_secs(30),
        client.send_transaction(tx, None)
    ).await {
        Ok(result) => match result {
            Ok(tx) => tx,
            Err(err) => {
                error!("Bridge transaction error: {}", err);
                return Err(anyhow!("Transaction error: {}", err));
            }
        },
        Err(_) => {
            return Err(anyhow!("Bridge transaction timeout"));
        }
    };
    
    // Wait for transaction to be included in block
    let receipt = match tokio::time::timeout(
        tokio::time::Duration::from_secs(180),
        pending_tx.await
    ).await {
        Ok(result) => match result {
            Ok(Some(receipt)) => receipt,
            Ok(None) => return Err(anyhow!("Transaction not confirmed")),
            Err(err) => return Err(anyhow!("Waiting for transaction confirmed: {}", err)),
        },
        Err(_) => {
            // Transaction may have been sent but we don't know the result
            // Return hash for user to check later
            return Ok(format!("0x{:x} (pending)", pending_tx.tx_hash()));
        }
    };
    
    // Check transaction status
    if receipt.status != Some(1.into()) {
        return Err(anyhow!("Bridge transaction failed: 0x{:x}", receipt.transaction_hash));
    }
    
    Ok(format!("0x{:x}", receipt.transaction_hash))
}

/// Bridge from NEAR to BSC
async fn bridge_from_near_to_bsc(&self, private_key: &str, to_address: &str, amount: U256) -> Result<String> {
    let near_provider = self.get_near_provider().await?;
    
    // Convert U256 to u128 (NEAR uses u128)
    let amount_u128 = amount.as_u128();
    
    // Create transaction
    let tx_hash = near_provider.bridge_to_bsc(private_key, to_address, amount_u128).await?;
    
    Ok(tx_hash)
}

/// Get total supply of DMD token on Solana
async fn get_total_supply_solana(&self) -> anyhow::Result<f64> {
    // Get RPC configuration for Solana
    let config = match get_chain_config(DmdChain::Solana) {
        Ok(config) => config,
        Err(e) => {
            error!("Cannot get RPC configuration for Solana: {}", e);
            return Err(anyhow::anyhow!("Solana configuration error: {}", e));
        }
    };
    
    // Create client request
    let client = reqwest::Client::new();
    let response = client.post(&config.rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTokenSupply",
            "params": [&self.solana_token_address]
        }))
        .send()
        .await?;
    
    // Check response
    let result = response.json::<serde_json::Value>().await?;
    
    // Parse result
    if let Some(error) = result.get("error") {
        return Err(anyhow::anyhow!("Solana total supply query error: {}", error));
    }
    
    if let Some(result_value) = result.get("result") {
        if let Some(value) = result_value.get("value") {
            if let Some(amount) = value.get("amount").and_then(|a| a.as_str()) {
                // Convert from string to f64
                if let Ok(parsed_amount) = amount.parse::<f64>() {
                    // Convert amount according to decimals
                    if let Some(decimals) = value.get("decimals").and_then(|d| d.as_u64()) {
                        let divisor = 10u64.pow(decimals as u32) as f64;
                        let total_supply = parsed_amount / divisor;
                        
                        info!("Total DMD supply on Solana: {}", total_supply);
                        return Ok(total_supply);
                    }
                }
            }
        }
    }
    
    Err(anyhow::anyhow!("Cannot parse Solana total supply result"))
}

/// Get total supply of DMD token on NEAR
async fn get_total_supply_near(&self) -> anyhow::Result<f64> {
    // Get RPC configuration for NEAR
    let config = match get_chain_config(DmdChain::Near) {
        Ok(config) => config,
        Err(e) => {
            error!("Cannot get RPC configuration for NEAR: {}", e);
            return Err(anyhow::anyhow!("NEAR configuration error: {}", e));
        }
    };
    
    // Create client request
    let client = reqwest::Client::new();
    let response = client.post(&config.rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "query",
            "params": {
                "request_type": "call_function",
                "finality": "final",
                "account_id": &self.near_contract_address,
                "method_name": "ft_total_supply",
                "args_base64": ""
            }
        }))
        .send()
        .await?;
    
    // Check response
    let result = response.json::<serde_json::Value>().await?;
    
    // Parse result
    if let Some(error) = result.get("error") {
        return Err(anyhow::anyhow!("NEAR total supply query error: {}", error));
    }
    
    if let Some(result_value) = result.get("result") {
        if let Some(result_bytes) = result_value.get("result").and_then(|r| r.as_array()) {
            // Convert bytes array to UTF-8 string
            let bytes: Vec<u8> = result_bytes.iter()
                .filter_map(|v| v.as_u64().map(|n| n as u8))
                .collect();
            
            if let Ok(result_str) = String::from_utf8(bytes) {
                // Parse JSON string from result
                if let Ok(amount_value) = serde_json::from_str::<serde_json::Value>(&result_str) {
                    if let Some(amount_str) = amount_value.as_str() {
                        if let Ok(amount) = amount_str.parse::<f64>() {
                            // NEAR FT usually has 24 decimals
                            let total_supply = amount / 1_000_000_000_000_000_000_000_000.0;
                            
                            info!("Total DMD supply on NEAR: {}", total_supply);
                            return Ok(total_supply);
                        }
                    }
                }
            }
        }
    }
    
    Err(anyhow::anyhow!("Cannot parse NEAR total supply result"))
}

/// Get total supply token on multiple blockchains
pub async fn get_total_supply_multi_chain(&self) -> MultiChainSupply {
    // Initialize result
    let mut result = MultiChainSupply {
        bsc: 0.0,
        polygon: 0.0,
        solana: 0.0,
        near: 0.0,
        total: 0.0,
    };
    
    // Perform total supply query on each blockchain in parallel
    let bsc_future = self.get_total_supply(DmdChain::Bsc);
    let polygon_future = self.get_total_supply(DmdChain::Polygon);
    let solana_future = self.get_total_supply_solana();
    let near_future = self.get_total_supply_near();
    
    // Collect results
    if let Ok(bsc_supply) = bsc_future.await {
        result.bsc = bsc_supply;
        result.total += bsc_supply;
    } else {
        warn!("Cannot get BSC total supply");
    }
    
    if let Ok(polygon_supply) = polygon_future.await {
        result.polygon = polygon_supply;
        result.total += polygon_supply;
    } else {
        warn!("Cannot get Polygon total supply");
    }
    
    if let Ok(solana_supply) = solana_future.await {
        result.solana = solana_supply;
        result.total += solana_supply;
    } else {
        warn!("Cannot get Solana total supply");
    }
    
    if let Ok(near_supply) = near_future.await {
        result.near = near_supply;
        result.total += near_supply;
    } else {
        warn!("Cannot get NEAR total supply");
    }
    
    // Log total supply summary
    info!("Total DMD token supply on multiple blockchains:");
    info!("- BSC: {}", result.bsc);
    info!("- Polygon: {}", result.polygon);
    info!("- Solana: {}", result.solana);
    info!("- NEAR: {}", result.near);
    info!("- TOTAL: {}", result.total);
    
    result
}

/// Chuyển đổi số dư từ U256 sang f64, sử dụng decimals động từ contract
pub async fn convert_balance_to_f64(&self, balance: U256, chain: DmdChain) -> Result<f64> {
    // Lấy decimals từ contract thay vì hardcode
    let decimals = self.get_token_decimals(chain).await?;
    
    // Chuyển đổi sử dụng decimals đúng cho từng blockchain
    let divisor = U256::from(10).pow(U256::from(decimals as u64));
    
    // Chuyển đổi thành f64
    let balance_f64 = balance.as_u128() as f64 / divisor.as_u128() as f64;
    
    Ok(balance_f64)
}

/// Lấy decimals của token trên chain cụ thể
pub async fn get_token_decimals(&self, chain: DmdChain) -> Result<u8> {
    match chain {
        DmdChain::Ethereum => {
            // Kiểm tra cache nếu có
            if let Some(decimals) = self.eth_decimals.get() {
                return Ok(*decimals);
            }
            
            // Không có trong cache, lấy từ contract
            match self.eth_provider.as_ref() {
                Some(provider) => {
                    let decimals = provider.decimals();
                    self.eth_decimals.set(decimals).ok();
                    Ok(decimals)
                },
                None => Ok(18) // Fallback nếu provider chưa được khởi tạo
            }
        },
        DmdChain::Bsc => {
            if let Some(decimals) = self.bsc_decimals.get() {
                return Ok(*decimals);
            }
            
            match self.bsc_provider.as_ref() {
                Some(provider) => {
                    let decimals = provider.decimals();
                    self.bsc_decimals.set(decimals).ok();
                    Ok(decimals)
                },
                None => Ok(18)
            }
        },
        // Các chain khác tương tự
        DmdChain::Near => Ok(24), // NEAR có 24 decimals theo chuẩn
        DmdChain::Solana => Ok(9), // Solana thường có 9 decimals
        DmdChain::Avalanche => Ok(18),
        DmdChain::Polygon => Ok(18),
        DmdChain::Arbitrum => Ok(18),
        DmdChain::Optimism => Ok(18),
        DmdChain::Fantom => Ok(18),
        DmdChain::Diamond => Ok(18),
    }
}

/// Struct chứa các thông tin về token trên các blockchain khác nhau
pub struct DmdToken {
    /// Current chain
    current_chain: DmdChain,
    
    /// ETH provider
    eth_provider: Option<Arc<EthContractProvider>>,
    
    /// BSC provider
    bsc_provider: Option<Arc<BscContractProvider>>,
    
    /// NEAR provider
    near_provider: Option<Arc<NearContractProvider>>,
    
    /// Solana provider
    solana_provider: Option<Arc<SolanaContractProvider>>,
    
    /// Chain to provider cache
    chain_providers: RwLock<HashMap<DmdChain, ChainConfig>>,
    
    /// Cache decimals cho Ethereum
    eth_decimals: OnceCell<u8>,
    
    /// Cache decimals cho BSC
    bsc_decimals: OnceCell<u8>,
    
    /// Cache expiration time in seconds
    cache_ttl: u64,
    
    /// Last refresh timestamps for different cache types
    last_refresh: RwLock<HashMap<String, u64>>,
    
    // ... existing fields ...
}

impl DmdToken {
    /// Create new token
    pub fn new() -> Self {
        Self {
            current_chain: DmdChain::Ethereum,
            eth_provider: None,
            bsc_provider: None,
            near_provider: None,
            solana_provider: None,
            chain_providers: RwLock::new(HashMap::new()),
            eth_decimals: OnceCell::new(),
            bsc_decimals: OnceCell::new(),
            cache_ttl: 300, // Default 5 minute TTL
            last_refresh: RwLock::new(HashMap::new()),
            // ... existing fields ...
        }
    }
    
    /// Create new token with custom cache TTL
    pub fn with_cache_ttl(ttl: u64) -> Self {
        let mut token = Self::new();
        token.cache_ttl = ttl;
        token
    }
    
    /// Get cache TTL
    pub fn get_cache_ttl(&self) -> u64 {
        self.cache_ttl
    }
    
    /// Set cache TTL
    pub fn set_cache_ttl(&mut self, ttl: u64) {
        self.cache_ttl = ttl;
    }
    
    /// Check if cache is valid
    fn is_cache_valid(&self, cache_key: &str) -> bool {
        let last_refresh = self.last_refresh.read().unwrap();
        
        if let Some(timestamp) = last_refresh.get(cache_key) {
            let now = chrono::Utc::now().timestamp() as u64;
            let elapsed = now - timestamp;
            
            elapsed < self.cache_ttl
        } else {
            false
        }
    }
    
    /// Update cache timestamp
    fn update_cache_timestamp(&self, cache_key: &str) {
        let mut last_refresh = self.last_refresh.write().unwrap();
        last_refresh.insert(cache_key.to_string(), chrono::Utc::now().timestamp() as u64);
    }
    
    /// Initialize Ethereum decimals
    async fn init_eth_decimals(&self) -> Result<u8> {
        // Get decimals from provider or use default
        let decimals = match &self.eth_provider {
            Some(provider) => provider.decimals(),
            None => 18, // Default for ERC-20 tokens
        };
        
        // Update cache timestamp
        self.update_cache_timestamp("eth_decimals");
        
        // Store in OnceCell
        if let Err(_) = self.eth_decimals.set(decimals) {
            warn!("Failed to set eth_decimals in OnceCell - value already set");
        }
        
        Ok(decimals)
    }
    
    /// Get ETH decimals with cache validation
    pub async fn get_eth_decimals(&self) -> Result<u8> {
        // Check if we need to initialize or refresh
        if !self.is_cache_valid("eth_decimals") || self.eth_decimals.get().is_none() {
            return self.init_eth_decimals().await;
        }
        
        // Get from cache
        match self.eth_decimals.get() {
            Some(decimals) => Ok(*decimals),
            None => self.init_eth_decimals().await, // Should not happen due to check above
        }
    }
    
    /// Initialize BSC decimals
    async fn init_bsc_decimals(&self) -> Result<u8> {
        // Get decimals from provider or use default
        let decimals = match &self.bsc_provider {
            Some(provider) => provider.decimals(),
            None => 18, // Default for BEP-20 tokens
        };
        
        // Update cache timestamp
        self.update_cache_timestamp("bsc_decimals");
        
        // Store in OnceCell
        if let Err(_) = self.bsc_decimals.set(decimals) {
            warn!("Failed to set bsc_decimals in OnceCell - value already set");
        }
        
        Ok(decimals)
    }
    
    /// Get BSC decimals with cache validation
    pub async fn get_bsc_decimals(&self) -> Result<u8> {
        // Check if we need to initialize or refresh
        if !self.is_cache_valid("bsc_decimals") || self.bsc_decimals.get().is_none() {
            return self.init_bsc_decimals().await;
        }
        
        // Get from cache
        match self.bsc_decimals.get() {
            Some(decimals) => Ok(*decimals),
            None => self.init_bsc_decimals().await, // Should not happen due to check above
        }
    }
    
    /// Force refresh all caches
    pub async fn refresh_all_caches(&self) -> Result<()> {
        // Refresh ETH decimals
        let _ = self.init_eth_decimals().await?;
        
        // Refresh BSC decimals
        let _ = self.init_bsc_decimals().await?;
        
        // Refresh chain providers cache
        let chain_providers = self.chain_providers.read().unwrap();
        for (chain, _) in chain_providers.iter() {
            drop(chain_providers); // Release read lock before calling get_chain_config
            let _ = self.get_chain_config(*chain)?;
            let chain_providers = self.chain_providers.read().unwrap(); // Re-acquire read lock
        }
        
        info!("All caches refreshed successfully");
        Ok(())
    }
    
    /// Invalidate specific cache
    pub fn invalidate_cache(&self, cache_key: &str) -> Result<()> {
        // Remove timestamp
        {
            let mut last_refresh = self.last_refresh.write().unwrap();
            last_refresh.remove(cache_key);
        }
        
        // Log cache invalidation
        info!("Cache invalidated: {}", cache_key);
        Ok(())
    }
    
    /// Invalidate all caches
    pub fn invalidate_all_caches(&self) -> Result<()> {
        // Clear all timestamps
        {
            let mut last_refresh = self.last_refresh.write().unwrap();
            last_refresh.clear();
        }
        
        // Log cache invalidation
        info!("All caches invalidated");
        Ok(())
    }
    
    /// Convert token balance to f64 with correct decimals
    pub async fn convert_balance_to_f64(&self, balance: U256, chain: DmdChain) -> Result<f64> {
        let decimals = match chain {
            DmdChain::Ethereum => self.get_eth_decimals().await?,
            DmdChain::Bsc => self.get_bsc_decimals().await?,
            _ => 18, // Default for most tokens
        };
        
        // Convert with correct decimals
        let divisor = U256::from(10).pow(U256::from(decimals));
        let balance_f64 = balance.as_u128() as f64 / divisor.as_u128() as f64;
        
        Ok(balance_f64)
    }
}

/// Kiểm tra xem một địa chỉ token có được phép sử dụng không dựa trên tier của người dùng
pub async fn is_token_allowed(&self, user_address: &str, token_address: &str) -> Result<bool> {
    info!("Kiểm tra quyền sử dụng token: {} cho người dùng: {}", token_address, user_address);
    
    // Danh sách tokens mặc định được phép dùng cho mọi tier
    let default_allowed_tokens = vec![
        // Stablecoins
        "0xdac17f958d2ee523a2206206994597c13d831ec7", // USDT
        "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", // USDC
        "0x6b175474e89094c44da98b954eedeac495271d0f", // DAI
        "0x4fabb145d64652a948d72533023f6e7a623c7c53", // BUSD
        "0x8e870d67f660d95d5be530380d0ec0bd388289e1", // PAX
        
        // Main assets
        "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", // WETH
        "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599", // WBTC
        "0x7d1afa7b718fb893db30a3abc0cfc608aacfebb0", // MATIC
        "0xB8c77482e45F1F44dE1745F52C74426C631bDD52", // BNB
        "0x1f9840a85d5af5bf1d1762f925bdaddc4201f984", // UNI
        
        // Dự án Diamond
        "0x90fE084F877C65e1b577c7b2eA64B8D8dd1AB278", // DMD Token
        "0x16a7FA783c2FF584B234e13825960F95244Fc7bC", // DMD Token (BSC)
    ];
    
    // Danh sách tokens cho Silver tier trở lên
    let silver_tier_tokens = vec![
        // DeFi tokens
        "0x7fc66500c84a76ad7e9c93437bfc5ac33e2ddae9", // AAVE
        "0x9f8f72aa9304c8b593d555f12ef6589cc3a579a2", // MKR
        "0xc00e94cb662c3520282e6f5717214004a7f26888", // COMP
        "0x0bc529c00C6401aEF6D220BE8C6Ea1667F6Ad93e", // YFI
        "0x4e15361fd6b4bb609fa63c81a2be19d873717870", // FTM
        "0xba100000625a3754423978a60c9317c58a424e3d", // BAL
        "0xd533a949740bb3306d119cc777fa900ba034cd52", // CRV
    ];
    
    // Danh sách tokens cho Gold tier trở lên
    let gold_tier_tokens = vec![
        // GameFi tokens
        "0xf629cbd94d3791c9250152bd8dfbdf380e2a3b9c", // ENJ
        "0x0f5d2fb29fb7d3cfee444a200298f468908cc942", // MANA
        "0xbb0e17ef65f82ab018d8edd776e8dd940327b28b", // AXS
        "0x15d4c048f83bd7e37d49ea4c83a07267ec4203da", // GALA
        "0x4b1e80cac91e2216eeb63e29b957eb91ae9c2be8", // JOE
    ];
    
    // Danh sách tokens cho Diamond tier
    let diamond_tier_tokens = vec![
        // Bất kỳ token nào người dùng muốn trade
        "*"
    ];
    
    // Kiem tra token co trong danh sach mac dinh
    let token_address_lower = token_address.to_lowercase();
    if default_allowed_tokens.iter().any(|&addr| addr.to_lowercase() == token_address_lower) {
        info!("Token được cho phép trong danh sách mặc định");
        return Ok(true);
    }
    
    // Kiểm tra tier của người dùng
    let tier = match self.get_user_tier(user_address).await {
        Ok(tier) => tier,
        Err(e) => {
            warn!("Không thể xác định tier của người dùng {}: {}", user_address, e);
            return Ok(false);
        }
    };
    
    // Kiểm tra quyền dựa trên tier
    match tier {
        DmdTokenTier::Diamond => {
            // Diamond tier được phép sử dụng mọi token
            info!("User với Diamond tier được phép sử dụng mọi token");
            Ok(true)
        },
        DmdTokenTier::Gold => {
            // Gold tier được phép sử dụng Gold tier tokens và Silver tier tokens
            let is_allowed = silver_tier_tokens.iter().any(|&addr| addr.to_lowercase() == token_address_lower) ||
                             gold_tier_tokens.iter().any(|&addr| addr.to_lowercase() == token_address_lower);
            
            info!("User với Gold tier: token {} {}", token_address, if is_allowed { "được phép" } else { "không được phép" });
            Ok(is_allowed)
        },
        DmdTokenTier::Silver => {
            // Silver tier được phép sử dụng Silver tier tokens
            let is_allowed = silver_tier_tokens.iter().any(|&addr| addr.to_lowercase() == token_address_lower);
            
            info!("User với Silver tier: token {} {}", token_address, if is_allowed { "được phép" } else { "không được phép" });
            Ok(is_allowed)
        },
        DmdTokenTier::Bronze | DmdTokenTier::Regular => {
            // Bronze và Regular tier chỉ được phép sử dụng các token mặc định
            info!("User với Bronze/Regular tier: token {} không được phép", token_address);
            Ok(false)
        },
        DmdTokenTier::Custom(level) => {
            // Custom tier được xử lý tùy theo level, cao hơn 4 được coi như Diamond
            if level >= 4 {
                info!("User với Custom tier level {}: được phép sử dụng mọi token", level);
                Ok(true)
            } else {
                // Tùy theo level, xác định được phép dùng những token nào
                let is_allowed = if level >= 3 {
                    // Level 3 tương đương Gold
                    silver_tier_tokens.iter().any(|&addr| addr.to_lowercase() == token_address_lower) ||
                    gold_tier_tokens.iter().any(|&addr| addr.to_lowercase() == token_address_lower)
                } else if level >= 2 {
                    // Level 2 tương đương Silver
                    silver_tier_tokens.iter().any(|&addr| addr.to_lowercase() == token_address_lower)
                } else {
                    // Level 0-1 chỉ được phép dùng các token mặc định
                    false
                };
                
                info!("User với Custom tier level {}: token {} {}", level, token_address, 
                     if is_allowed { "được phép" } else { "không được phép" });
                Ok(is_allowed)
            }
        }
    }
}

/// Lấy tier của người dùng
async fn get_user_tier(&self, user_address: &str) -> Result<DmdTokenTier> {
    // Tìm thông tin balance của người dùng trên tất cả các chain hỗ trợ
    let mut max_tier = DmdTokenTier::Regular;
    
    // Thử lấy từ BSC contract
    match self.get_bsc_provider() {
        Ok(bsc_provider) => {
            match bsc_provider.check_user_tier(user_address).await {
                Ok(tier) => {
                    // Cập nhật max tier nếu tier mới cao hơn
                    if tier as u8 > max_tier as u8 {
                        max_tier = tier;
                    }
                },
                Err(e) => warn!("Không thể lấy tier từ BSC contract: {}", e)
            }
        },
        Err(e) => warn!("Không thể kết nối với BSC contract: {}", e)
    }
    
    // Thử lấy từ Arbitrum contract
    match self.get_arbitrum_provider() {
        Ok(arb_provider) => {
            match arb_provider.get_token_tier(user_address).await {
                Ok(tier) => {
                    // Cập nhật max tier nếu tier mới cao hơn
                    if tier as u8 > max_tier as u8 {
                        max_tier = tier;
                    }
                },
                Err(e) => warn!("Không thể lấy tier từ Arbitrum contract: {}", e)
            }
        },
        Err(e) => warn!("Không thể kết nối với Arbitrum contract: {}", e)
    }
    
    // Các contract khác có thể thêm tương tự
    
    info!("Xác định tier của người dùng {}: {:?}", user_address, max_tier);
    Ok(max_tier)
}

// Các hàm hỗ trợ để truy cập provider cho các chain
fn get_bsc_provider(&self) -> Result<BscContractProvider> {
    // Tạo BSC provider với cấu hình mặc định
    Ok(BscContractProvider::new(None))
}

fn get_arbitrum_provider(&self) -> Result<ArbContractProvider> {
    // Tạo Arbitrum provider với cấu hình mặc định
    ArbContractProvider::new(None).await
} 