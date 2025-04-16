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
    
    // Get Solana provider configuration from cache or create new
    let solana_config = self.get_chain_config(DmdChain::Solana)?;
    
    // Initialize SolanaContractProvider
    let solana_provider = SolanaContractProvider::new(Some(SolanaContractConfig {
        rpc_url: solana_config.rpc_url.clone(),
        token_mint: solana_config.token_address.clone(),
        timeout_ms: solana_config.timeout_ms,
        ..Default::default()
    })).await?;
    
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
    
    // Create client middleware
    let client = SignerMiddleware::new(provider, wallet);
    
    // Check current gas price to optimize transaction fee
    let gas_price = client.get_gas_price().await?;
    let max_priority_fee = U256::from(30_000_000_000u64); // 30 Gwei
    
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
    
    // Create and send transaction with custom gas parameters
    let tx = TransactionRequest::new()
        .to(contract_address)
        .data(data)
        .gas_price(gas_price)
        .from(from_address);
    
    let pending_tx = client.send_transaction(tx, None).await?;
    let receipt = pending_tx.await?
        .ok_or_else(|| anyhow!("No receipt received"))?;
    
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
    let bridge_key = format!("{}_to_{}_{}_{}",
        from_chain.name(),
        to_chain.name(),
        address_short,
        private_key_to_public_address(private_key)?
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
async fn monitor_bridge_transaction(&self, tx_hash: String, from_chain: DmdChain, to_chain: DmdChain) -> anyhow::Result<()> {
    // Create background task to track bridge transaction
    tokio::spawn(async move {
        // Tracking parameters
        const MAX_CHECKS: usize = 30; // Maximum checks 30 times
        const CHECK_INTERVAL_SEC: u64 = 20; // Initially check every 20 seconds
        const MAX_CHECK_INTERVAL_SEC: u64 = 300; // Maximum check every 5 minutes
        
        let mut check_interval = CHECK_INTERVAL_SEC;
        let mut next_status_log = 0;
        
        // Get chain configuration
        let from_config = match get_chain_config(from_chain) {
            Ok(config) => config,
            Err(e) => {
                error!("Cannot get RPC configuration for source chain {}: {}", from_chain.name(), e);
                return;
            }
        };
        
        let to_config = match get_chain_config(to_chain) {
            Ok(config) => config,
            Err(e) => {
                error!("Cannot get RPC configuration for target chain {}: {}", to_chain.name(), e);
                return;
            }
        };
        
        info!("Starting bridge transaction tracking {} (chain {} -> {})", tx_hash, from_chain.name(), to_chain.name());
        
        // Current transaction status
        let mut transaction_status = TransactionStatus::Pending;
        let mut current_check = 0;
        let start_time = tokio::time::Instant::now();
        
        // Store information for successful bridge
        let mut target_tx_hash: Option<String> = None;
        
        // Loop to check transaction status
        loop {
            current_check += 1;
            
            // Check transaction status from source chain
            transaction_status = match from_chain {
                DmdChain::Bsc => {
                    match verify_bsc_transaction_status(&tx_hash, from_config.rpc_url.as_str()).await {
                        Ok(status) => status,
                        Err(e) => {
                            warn!("Cannot check BSC transaction status {}: {}", tx_hash, e);
                            TransactionStatus::Pending
                        }
                    }
                },
                DmdChain::Near => {
                    match verify_near_transaction_status(&tx_hash, from_config.rpc_url.as_str()).await {
                        Ok(status) => status,
                        Err(e) => {
                            warn!("Cannot check NEAR transaction status {}: {}", tx_hash, e);
                            TransactionStatus::Pending
                        }
                    }
                },
                DmdChain::Polygon => {
                    match verify_evm_transaction_status(&tx_hash, from_config.rpc_url.as_str()).await {
                        Ok(status) => status,
                        Err(e) => {
                            warn!("Cannot check Polygon transaction status {}: {}", tx_hash, e);
                            TransactionStatus::Pending
                        }
                    }
                },
                DmdChain::Solana => {
                    match verify_solana_transaction_status(&tx_hash, from_config.rpc_url.as_str()).await {
                        Ok(status) => status,
                        Err(e) => {
                            warn!("Cannot check Solana transaction status {}: {}", tx_hash, e);
                            TransactionStatus::Pending
                        }
                    }
                },
                _ => {
                    // Other EVM-compatible blockchain
                    match verify_evm_transaction_status(&tx_hash, from_config.rpc_url.as_str()).await {
                        Ok(status) => status,
                        Err(e) => {
                            warn!("Cannot check {} transaction status from {}: {}", tx_hash, from_chain.name(), e);
                            TransactionStatus::Pending
                        }
                    }
                }
            };
            
            // Check if transaction has been confirmed on source chain
            if transaction_status == TransactionStatus::Confirmed {
                info!("Transaction {} confirmed on source chain {}!", tx_hash, from_chain.name());
                
                // Estimated time to complete bridge
                let estimated_bridge_time = match (from_chain, to_chain) {
                    (DmdChain::Bsc, DmdChain::Near) | (DmdChain::Near, DmdChain::Bsc) => 180, // ~3 minutes
                    (DmdChain::Bsc, DmdChain::Solana) | (DmdChain::Solana, DmdChain::Bsc) => 240, // ~4 minutes
                    (DmdChain::Bsc, DmdChain::Polygon) | (DmdChain::Polygon, DmdChain::Bsc) => 120, // ~2 minutes
                    _ => 300, // ~5 minutes for other cases
                };
                
                info!("Waiting for LayerZero bridge to complete (estimated {} seconds)...", estimated_bridge_time);
                
                // Increase check interval
                check_interval = 30;
                
                // Check bridge events on target chain
                // First wait a period for LayerZero bridge to process
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                
                // Start checking bridge events on target chain
                let mut bridge_complete = false;
                let mut bridge_checks = 0;
                
                while bridge_checks < 10 && !bridge_complete {
                    bridge_checks += 1;
                    
                    // Check bridge events completed
                    let (completed, result_tx) = match to_chain {
                        DmdChain::Bsc => check_bsc_bridge_completion(tx_hash.clone(), to_config.rpc_url.as_str()).await,
                        DmdChain::Near => check_near_bridge_completion(tx_hash.clone(), to_config.rpc_url.as_str()).await,
                        DmdChain::Polygon => check_polygon_bridge_completion(tx_hash.clone(), to_config.rpc_url.as_str()).await,
                        DmdChain::Solana => check_solana_bridge_completion(tx_hash.clone(), to_config.rpc_url.as_str()).await,
                        _ => (false, None),
                    };
                    
                    if completed {
                        bridge_complete = true;
                        target_tx_hash = result_tx.clone();
                        
                        info!("Bridge from {} to {} completed! Transaction hash on target chain: {}", 
                            from_chain.name(), to_chain.name(), result_tx.unwrap_or_else(|| "unknown".to_string()));
                        
                        // Notify bridge success
                        notify_bridge_success(tx_hash.clone(), from_chain, to_chain, result_tx).await;
                        break;
                    }
                    
                    // Wait before checking again
                    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                }
                
                if !bridge_complete {
                    warn!("Cannot confirm bridge completed after multiple checks. Transaction may still be processing.");
                }
                
                // Bridge completed or checked enough times
                break;
            } else if transaction_status == TransactionStatus::Failed {
                error!("Bridge transaction {} failed on source chain {}!", tx_hash, from_chain.name());
                break;
            }
            
            // Check if exceeded maximum check times
            if current_check >= MAX_CHECKS {
                warn!("Exceeded maximum check times ({}) for bridge transaction {}", MAX_CHECKS, tx_hash);
                break;
            }
            
            // In log status periodically (to not print too much log)
            let now = tokio::time::Instant::now();
            let elapsed = (now - start_time).as_secs();
            
            if elapsed >= next_status_log {
                info!(
                    "Bridge {} ({} -> {}): {} - check {}/{}, passed {} seconds",
                    tx_hash, from_chain.name(), to_chain.name(), 
                    transaction_status.as_str(), current_check, MAX_CHECKS, elapsed
                );
                
                next_status_log = elapsed + 60; // In log every 60 seconds
            }
            
            // Increase gradually check interval (exponential backoff)
            if check_interval < MAX_CHECK_INTERVAL_SEC {
                check_interval = std::cmp::min(check_interval * 2, MAX_CHECK_INTERVAL_SEC);
            }
            
            // Wait before checking again
            tokio::time::sleep(tokio::time::Duration::from_secs(check_interval)).await;
        }
        
        // Calculate total time
        let total_time = (tokio::time::Instant::now() - start_time).as_secs();
        
        // Print summary information
        match transaction_status {
            TransactionStatus::Confirmed => {
                if let Some(target_tx) = target_tx_hash {
                    info!(
                        "Bridge tracking complete: {} ({} -> {}) completed after {} seconds. Hash on target chain: {}",
                        tx_hash, from_chain.name(), to_chain.name(), total_time, target_tx
                    );
                } else {
                    info!(
                        "Bridge tracking complete: {} ({} -> {}) confirmed on source chain after {} seconds.",
                        tx_hash, from_chain.name(), to_chain.name(), total_time
                    );
                }
            },
            TransactionStatus::Failed => {
                error!(
                    "Bridge tracking complete: {} ({} -> {}) failed after {} seconds.",
                    tx_hash, from_chain.name(), to_chain.name(), total_time
                );
            },
            TransactionStatus::Pending => {
                warn!(
                    "Bridge tracking complete: {} ({} -> {}) still processing after {} seconds and {} checks.",
                    tx_hash, from_chain.name(), to_chain.name(), total_time, current_check
                );
            }
        }
    });
    
    Ok(())
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
    // Call LayerZero event query
    // This is a mock, actual implementation needed when API available
    
    // Mock: 80% chance of bridge success
    let random = rand::random::<f32>();
    if random < 0.8 {
        // Create random hash for transaction on target chain
        let target_tx = format!("0x{:x}", rand::random::<u128>());
        (true, Some(target_tx))
    } else {
        (false, None)
    }
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
    // ... existing fields ...
    
    /// Cache decimals cho Ethereum
    eth_decimals: OnceCell<u8>,
    
    /// Cache decimals cho BSC
    bsc_decimals: OnceCell<u8>,
    
    // ... existing fields ...
}

impl DmdToken {
    /// Tạo đối tượng token mới với cấu hình mặc định
    pub fn new() -> Self {
        // ... existing code ...
        
        Self {
            // ... existing fields ...
            eth_decimals: OnceCell::new(),
            bsc_decimals: OnceCell::new(),
            // ... existing fields ...
        }
    }
    
    // ... existing code ...
    
    /// Lấy số dư của người dùng trên một blockchain cụ thể
    pub async fn get_balance(&self, chain: DmdChain, address: &str) -> Result<f64> {
        let balance = match chain {
            DmdChain::Ethereum => {
                match self.eth_provider.as_ref() {
                    Some(provider) => provider.get_balance(address).await?,
                    None => return Err(anyhow!("ETH provider not initialized")),
                }
            },
            // ... existing cases ...
        };
        
        // Sử dụng hàm mới để chuyển đổi
        self.convert_balance_to_f64(balance, chain).await
    }
    
    // ... existing code ...
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