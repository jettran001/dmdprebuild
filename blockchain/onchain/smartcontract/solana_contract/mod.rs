use std::ops::Drop;
use rand::RngCore;

/// SecretKey wraps sensitive key material and implements secure handling
pub struct SecretKey {
    bytes: Vec<u8>,
}

impl SecretKey {
    /// Creates a new SecretKey from a string (base58 encoded)
    pub fn from_base58(private_key: &str) -> Result<Self> {
        let bytes = base58::decode(private_key)
            .map_err(|e| anyhow::anyhow!("Failed to decode private key: {}", e))?;
        
        if bytes.len() != 64 {
            return Err(anyhow::anyhow!("Invalid key length: {} (expected 64)", bytes.len()));
        }
        
        Ok(Self { bytes })
    }
    
    /// Access the raw bytes 
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

// Implement Drop to ensure the memory is zeroed when the SecretKey is dropped
impl Drop for SecretKey {
    fn drop(&mut self) {
        // Overwrite with random data first
        let mut rng = rand::thread_rng();
        rng.fill_bytes(&mut self.bytes);
        
        // Then zero the memory
        for byte in self.bytes.iter_mut() {
            *byte = 0;
        }
    }
}

/// Chuyển token tới một địa chỉ
pub async fn transfer_token(&self, private_key: &str, to_account: &str, amount: u64) -> Result<String> {
    info!("Bắt đầu chuyển {} tokens đến địa chỉ {}", amount, to_account);
    
    // Kiểm tra tham số đầu vào
    if private_key.is_empty() {
        return Err(anyhow!("Private key không được để trống"));
    }
    
    if to_account.is_empty() {
        return Err(anyhow!("Địa chỉ nhận không được để trống"));
    }
    
    // Kiểm tra địa chỉ Solana
    const BASE58_CHARS: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    if !to_account.chars().all(|c| BASE58_CHARS.contains(c)) || to_account.len() < 32 || to_account.len() > 44 {
        return Err(anyhow!("Địa chỉ Solana không hợp lệ: {}", to_account));
    }
    
    // Create a SecretKey that will be zeroed when dropped
    let secret_key = SecretKey::from_base58(private_key)?;
    
    // Thiết lập cơ chế retry
    const MAX_RETRY: usize = 3;
    for attempt in 0..MAX_RETRY {
        info!("Thử chuyển token lần {}/{}", attempt + 1, MAX_RETRY);
        
        match self.execute_transfer_transaction_secure(&secret_key, to_account, amount as f64).await {
            Ok(tx_hash) => {
                info!("Tạo transaction thành công: {}", tx_hash);
                
                // Kiểm tra trạng thái transaction
                match self.verify_transaction_status(tx_hash.clone()).await {
                    Ok(true) => {
                        info!("Transaction thành công: {}", tx_hash);
                        return Ok(tx_hash);
                    },
                    Ok(false) => {
                        if attempt < MAX_RETRY - 1 {
                            warn!("Transaction chưa hoàn thành, thử lại lần {}", attempt + 2);
                            tokio::time::sleep(Duration::from_millis(2000 * (attempt as u64 + 1))).await;
                            continue;
                        } else {
                            info!("Transaction đang chờ xử lý: {}", tx_hash);
                            return Ok(tx_hash);
                        }
                    },
                    Err(e) => {
                        error!("Lỗi kiểm tra trạng thái transaction: {}", e);
                        if attempt < MAX_RETRY - 1 {
                            tokio::time::sleep(Duration::from_millis(1000 * (attempt as u64 + 1))).await;
                            continue;
                        } else {
                            return Ok(tx_hash); // Vẫn trả về tx_hash vì transaction có thể đã được gửi
                        }
                    }
                }
            },
            Err(e) => {
                error!("Lỗi khi tạo transaction (lần {}): {}", attempt + 1, e);
                if attempt < MAX_RETRY - 1 {
                    // Chờ trước khi thử lại với thời gian chờ tăng dần
                    tokio::time::sleep(Duration::from_millis(2000 * (attempt as u64 + 1))).await;
                } else {
                    return Err(anyhow!("Không thể tạo transaction sau {} lần thử: {}", MAX_RETRY, e));
                }
            }
        }
    }
    
    // Không bao giờ nên đến đây, nhưng để đảm bảo
    Err(anyhow!("Không thể chuyển token sau {} lần thử", MAX_RETRY))
}

/// Phiên bản bảo mật của execute_transfer_transaction - sử dụng SecretKey 
/// thay vì string để tăng bảo mật
async fn execute_transfer_transaction_secure(
    &self,
    secret_key: &SecretKey,
    to_address: &str,
    amount: f64,
) -> Result<String> {
    let from_keypair = match self.create_secure_keypair(secret_key) {
        Ok(keypair) => keypair,
        Err(e) => return Err(anyhow::anyhow!("Failed to create keypair: {}", e)),
    };
    
    let from_pubkey = from_keypair.pubkey();
    let from_address = from_pubkey.to_string();
    
    info!("Thực hiện chuyển DMD trên Solana: {} -> {}, lượng: {}", 
          from_address, to_address, amount);

    // Xác thực đầu vào
    if to_address.is_empty() || !to_address.starts_with("So") {
        return Err(anyhow::anyhow!("Địa chỉ đích không hợp lệ: {}", to_address));
    }
    
    if amount <= 0.0 {
        return Err(anyhow::anyhow!("Số lượng phải lớn hơn 0"));
    }
    
    #[cfg(feature = "solana_sdk")]
    {
        // Triển khai sử dụng Solana SDK khi feature "solana_sdk" được bật
        use solana_sdk::{
            commitment_config::CommitmentConfig,
            instruction::{AccountMeta, Instruction},
            message::Message,
            pubkey::Pubkey,
            signature::Keypair,
            signer::Signer,
            transaction::Transaction,
        };
        use solana_client::rpc_client::RpcClient;
        use solana_program::program_pack::Pack;
        use spl_token::instruction as token_instruction;
        
        // Khởi tạo client với cấu hình
        let rpc_config = self.get_rpc_config()?;
        let client = RpcClient::new_with_commitment(
            rpc_config.url.clone(),
            CommitmentConfig::confirmed(),
        );
        
        // Chuyển đổi địa chỉ string sang Pubkey
        let to_pubkey = Pubkey::from_str(to_address)?;
        
        // Tìm token accounts
        let token_pubkey = Pubkey::from_str(&self.token_address)?;
        
        // Scope để bảo đảm keypair bị drop sau khi dùng xong
        {
            // Tìm token account của người gửi
            let from_token_accounts = client.get_token_accounts_by_owner(
                &from_pubkey,
                spl_token::id(),
            )?;
            
            let from_token_account = from_token_accounts.iter()
                .find(|acc| acc.pubkey.to_string() == format!("{}Token", from_address))
                .ok_or_else(|| anyhow::anyhow!("Không tìm thấy token account cho địa chỉ {}", from_address))?;
            
            // Tìm hoặc tạo token account của người nhận
            let to_token_accounts = client.get_token_accounts_by_owner(
                &to_pubkey,
                spl_token::id(),
            )?;
            
            let to_token_account = to_token_accounts.iter()
                .find(|acc| acc.pubkey.to_string() == format!("{}Token", to_address))
                .map(|acc| acc.pubkey.clone());
            
            // Nếu không tìm thấy token account của người nhận, tạo mới
            let instruction = if let Some(to_acc) = to_token_account {
                // Tạo instruction chuyển token
                token_instruction::transfer(
                    &spl_token::id(),
                    &from_token_account.pubkey,
                    &to_acc,
                    &from_pubkey,
                    &[&from_pubkey],
                    (amount * 1_000_000_000.0) as u64,
                )?
            } else {
                // Tạo account mới và chuyển token
                let rent = client.get_minimum_balance_for_rent_exemption(
                    spl_token::state::Account::LEN,
                )?;
                
                let new_account = Keypair::new();
                let create_account_ix = solana_sdk::system_instruction::create_account(
                    &from_pubkey,
                    &new_account.pubkey(),
                    rent,
                    spl_token::state::Account::LEN as u64,
                    &spl_token::id(),
                );
                
                let init_account_ix = token_instruction::initialize_account(
                    &spl_token::id(),
                    &new_account.pubkey(),
                    &token_pubkey,
                    &to_pubkey,
                )?;
                
                let transfer_ix = token_instruction::transfer(
                    &spl_token::id(),
                    &from_token_account.pubkey,
                    &new_account.pubkey(),
                    &from_pubkey,
                    &[&from_pubkey],
                    (amount * 1_000_000_000.0) as u64,
                )?;
                
                // Kết hợp tất cả instruction
                let message = Message::new(
                    &[create_account_ix, init_account_ix, transfer_ix],
                    Some(&from_pubkey),
                );
                
                let blockhash = client.get_latest_blockhash()?;
                
                let mut tx = Transaction::new_unsigned(message);
                tx.sign(&[&from_keypair, &new_account], blockhash);
                
                // Gửi giao dịch đã ký
                let signature = client.send_and_confirm_transaction_with_spinner(&tx)?;
                
                return Ok(signature.to_string());
            };
            
            // Tạo giao dịch từ instruction
            let blockhash = client.get_latest_blockhash()?;
            let message = Message::new(&[instruction], Some(&from_pubkey));
            let mut tx = Transaction::new_unsigned(message);
            tx.sign(&[&from_keypair], blockhash);
            
            // Gửi giao dịch đã ký
            let signature = client.send_and_confirm_transaction_with_spinner(&tx)?;
            
            info!("Giao dịch Solana thành công: {}", signature);
            return Ok(signature.to_string());
        } // Keypair auto-dropped here
    }
    
    #[cfg(not(feature = "solana_sdk"))]
    {
        // Implementation for non-solana_sdk feature
        // ...
        Err(anyhow::anyhow!("Not implemented without solana_sdk feature"))
    }
}

/// Helper method to create a Solana Keypair from our SecretKey
#[cfg(feature = "solana_sdk")]
fn create_secure_keypair(&self, secret_key: &SecretKey) -> Result<solana_sdk::signature::Keypair> {
    use solana_sdk::signature::Keypair;
    
    let keypair = Keypair::from_bytes(secret_key.as_bytes())
        .map_err(|e| anyhow::anyhow!("Failed to create Solana keypair: {}", e))?;
    
    Ok(keypair)
}

// Original function is kept for backward compatibility but marked as deprecated
#[deprecated(since = "1.1.0", note = "Use execute_transfer_transaction_secure instead")]
async fn execute_transfer_transaction(
    &self,
    from_address: &str,
    to_address: &str,
    amount: f64,
    private_key: &str,
) -> Result<String> {
    // Create a temporary SecretKey
    let secret_key = SecretKey::from_base58(private_key)?;
    
    // Call the secure version 
    self.execute_transfer_transaction_secure(&secret_key, to_address, amount).await
}

/// Kiểm tra trạng thái giao dịch
async fn verify_transaction_status(&self, tx_hash: &str) -> Result<TransactionStatus> {
    info!("Kiểm tra trạng thái giao dịch Solana: {}", tx_hash);
    
    // Kiểm tra hash giao dịch hợp lệ
    if tx_hash.is_empty() {
        return Err(anyhow::anyhow!("Hash giao dịch không được để trống"));
    }
    
    // Khởi tạo HTTP client
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(30000)) // Timeout 30 giây
        .build()?;
    
    // Bước 1: Kiểm tra trạng thái chữ ký giao dịch
    let signature_status_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSignatureStatuses",
        "params": [
            [tx_hash]
        ]
    });
    
    let signature_response = client.post(&self.rpc_url)
        .json(&signature_status_request)
        .send()
        .await?;
    
    let signature_json: serde_json::Value = signature_response.json().await?;
    
    if let Some(error) = signature_json.get("error") {
        error!("Lỗi khi kiểm tra trạng thái chữ ký Solana: {:?}", error);
        return Err(anyhow::anyhow!("Lỗi RPC Solana: {:?}", error));
    }
    
    // Lấy trạng thái từ phản hồi
    let status = match signature_json.get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first()) {
            Some(status) => status,
            None => {
                info!("Không tìm thấy thông tin trạng thái cho giao dịch {}", tx_hash);
                return Ok(TransactionStatus::Pending);
            }
        };
    
    // Kiểm tra nếu trạng thái là null (giao dịch không tồn tại)
    if status.is_null() {
        info!("Giao dịch {} chưa được xác nhận hoặc không tồn tại", tx_hash);
        return Ok(TransactionStatus::Pending);
    }

    // Kiểm tra xem giao dịch đã được xác nhận hay chưa
    let confirmed = status.get("confirmationStatus")
        .and_then(|c| c.as_str())
        .map(|s| s == "finalized" || s == "confirmed")
        .unwrap_or(false);

    if !confirmed {
        info!("Giao dịch {} chưa được xác nhận hoàn toàn", tx_hash);
        return Ok(TransactionStatus::Pending);
    }

    // Nếu giao dịch đã được xác nhận, kiểm tra chi tiết để xác định thành công hay thất bại
    let transaction_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTransaction",
        "params": [
            tx_hash,
            { "encoding": "json", "maxSupportedTransactionVersion": 0 }
        ]
    });

    let transaction_response = client.post(&self.rpc_url)
        .json(&transaction_request)
        .send()
        .await?;

    let transaction_json: serde_json::Value = transaction_response.json().await?;

    if let Some(error) = transaction_json.get("error") {
        error!("Lỗi khi lấy thông tin giao dịch Solana: {:?}", error);
        return Err(anyhow::anyhow!("Lỗi RPC Solana: {:?}", error));
    }

    // Kiểm tra meta.err - nếu không null thì giao dịch thất bại
    let err = transaction_json.get("result")
        .and_then(|r| r.get("meta"))
        .and_then(|m| m.get("err"));

    if err.is_some() && !err.unwrap().is_null() {
        error!("Giao dịch Solana thất bại: {}, lỗi: {:?}", tx_hash, err);
        return Ok(TransactionStatus::Failed);
    }

    // Nếu tất cả kiểm tra đều ổn, giao dịch đã thành công
    info!("Giao dịch Solana thành công: {}", tx_hash);
    Ok(TransactionStatus::Success)
}

/// Lấy tổng cung token DMD trên Solana
async fn total_supply(&self) -> Result<f64> {
    info!("Lấy tổng cung DMD trên Solana");
    
    #[cfg(feature = "solana_sdk")]
    {
        use solana_client::rpc_client::RpcClient;
        use solana_sdk::{
            commitment_config::CommitmentConfig,
            pubkey::Pubkey,
        };
        use spl_token::state::{Mint, Account};
        
        // Khởi tạo client với cấu hình
        let rpc_config = self.get_rpc_config()?;
        let client = RpcClient::new_with_commitment(
            rpc_config.url.clone(),
            CommitmentConfig::confirmed(),
        );
        
        // Lấy thông tin về token mint từ địa chỉ token
        let token_pubkey = Pubkey::from_str(&self.token_address)?;
        let mint_account = client.get_account(&token_pubkey)?;
        
        // Parse thông tin mint từ account data
        let mint_data = mint_account.data.as_slice();
        let mint_info = Mint::unpack(mint_data)?;
        
        // Lấy tổng cung (supply) từ mint info và chuyển đổi sang đơn vị DMD
        let total_supply = (mint_info.supply as f64) / 1_000_000_000.0;
        
        info!("Tổng cung DMD trên Solana: {}", total_supply);
        Ok(total_supply)
    }
    
    #[cfg(not(feature = "solana_sdk"))]
    {
        // Sử dụng Solana Web3 API khi không có feature "solana_sdk"
        let rpc_config = self.get_rpc_config()?;
        let client = reqwest::Client::new();
        
        // Tạo yêu cầu lấy thông tin về token mint
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTokenSupply",
            "params": [self.token_address]
        });
        
        // Gửi yêu cầu và nhận phản hồi
        let response = client.post(&rpc_config.url)
            .json(&request)
            .send()
            .await?;
            
        let supply_json: serde_json::Value = response.json().await?;
        
        // Kiểm tra lỗi trong phản hồi
        if let Some(error) = supply_json.get("error") {
            error!("Lỗi khi lấy tổng cung token: {:?}", error);
            return Err(anyhow::anyhow!("Không thể lấy tổng cung token: {:?}", error));
        }
        
        // Trích xuất thông tin tổng cung từ phản hồi
        let amount = supply_json["result"]["value"]["amount"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Không thể đọc lượng token từ phản hồi"))?;
            
        let decimals = supply_json["result"]["value"]["decimals"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Không thể đọc số thập phân từ phản hồi"))?;
            
        // Chuyển đổi amount string sang số và tính theo đơn vị
        let amount_value = amount.parse::<f64>()?;
        let total_supply = amount_value / (10.0_f64.powi(decimals as i32));
        
        info!("Tổng cung DMD trên Solana: {}", total_supply);
        Ok(total_supply)
    }
}

/// Lấy số dư DMD token của một tài khoản Solana
pub async fn get_balance(&self, account: &str) -> Result<f64> {
    info!("Truy vấn số dư DMD cho tài khoản Solana: {}", account);
    
    // Xác thực địa chỉ tài khoản
    if account.is_empty() {
        return Err(anyhow::anyhow!("Địa chỉ tài khoản Solana không hợp lệ"));
    }
    
    // Cố gắng parse địa chỉ Solana
    match Pubkey::from_str(account) {
        Ok(_) => {}, // Địa chỉ hợp lệ
        Err(e) => return Err(anyhow::anyhow!("Địa chỉ Solana không hợp lệ: {}", e)),
    }
    
    #[cfg(feature = "solana_sdk")]
    {
        use solana_client::rpc_client::RpcClient;
        use solana_sdk::{
            commitment_config::CommitmentConfig,
            pubkey::Pubkey,
        };
        use spl_token::state::{Account as TokenAccount};
        
        // Khởi tạo client với cấu hình
        let rpc_config = self.get_rpc_config()?;
        let client = RpcClient::new_with_commitment(
            rpc_config.url.clone(),
            CommitmentConfig::confirmed(),
        );
        
        let token_pubkey = Pubkey::from_str(&self.token_address)?;
        let account_pubkey = Pubkey::from_str(account)?;
        
        // Tìm token account cho tài khoản này
        let token_accounts = client.get_token_accounts_by_owner(
            &account_pubkey,
            spl_token::id_token_program(&token_pubkey),
        )?;
        
        // Tìm token account cho DMD token
        let mut total_balance = 0.0;
        for token_account in token_accounts {
            let account_data = client.get_account(&Pubkey::from_str(&token_account.pubkey)?)?;
            let token_account_info = TokenAccount::unpack(account_data.data.as_slice())?;
            
            if token_account_info.mint == token_pubkey {
                let balance = token_account_info.amount as f64 / 1_000_000_000.0;
                total_balance += balance;
            }
        }
        
        info!("Số dư DMD của tài khoản {}: {}", account, total_balance);
        Ok(total_balance)
    }
    
    #[cfg(not(feature = "solana_sdk"))]
    {
        // Sử dụng Solana Web3 API khi không có feature "solana_sdk"
        // Khởi tạo client HTTP
        let rpc_config = self.get_rpc_config()?;
        let client = reqwest::Client::new();
        
        // Tạo RPC request để lấy tất cả token accounts của người dùng
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTokenAccountsByOwner",
            "params": [
                account,
                {
                    "programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA" // SPL Token Program ID
                },
                {
                    "encoding": "jsonParsed"
                }
            ]
        });
        
        // Gửi request và xử lý response
        let response = client.post(&rpc_config.url)
            .json(&request)
            .send()
            .await?;
            
        let accounts_json: serde_json::Value = response.json().await?;
        
        // Kiểm tra lỗi trong response
        if let Some(error) = accounts_json.get("error") {
            error!("Lỗi khi lấy token account: {:?}", error);
            return Err(anyhow::anyhow!("Không thể lấy token account: {:?}", error));
        }
        
        // Tính tổng số dư từ tất cả token accounts có mint là DMD token
        let mut total_balance = 0.0;
        
        if let Some(accounts) = accounts_json["result"]["value"].as_array() {
            for account in accounts {
                if let Some(mint) = account["account"]["data"]["parsed"]["info"]["mint"].as_str() {
                    // Kiểm tra nếu token này là DMD token
                    if mint == self.token_address {
                        if let Some(amount_str) = account["account"]["data"]["parsed"]["info"]["tokenAmount"]["amount"].as_str() {
                            if let Some(decimals) = account["account"]["data"]["parsed"]["info"]["tokenAmount"]["decimals"].as_u64() {
                                if let Ok(amount) = amount_str.parse::<f64>() {
                                    let token_balance = amount / (10.0_f64.powi(decimals as i32));
                                    total_balance += token_balance;
                                }
                            }
                        }
                    }
                }
            }
        }
        
        info!("Số dư DMD của tài khoản {}: {}", account, total_balance);
        Ok(total_balance)
    }
}

#[async_trait]
impl TokenInterface for SolanaContractProvider {
    async fn get_balance_of(&self, account: &str) -> Result<ethers::types::U256> {
        // Lấy số dư và chuyển đổi sang U256
        let balance = self.get_balance(account).await?;
        let amount_base = (balance * 1_000_000_000.0) as u128;
        Ok(ethers::types::U256::from(amount_base))
    }

    async fn get_total_supply(&self) -> Result<ethers::types::U256> {
        // Lấy tổng cung và chuyển đổi sang U256
        let supply = self.total_supply().await?;
        // Chuyển đổi từ f64 sang U256 với 9 decimals (Solana chuẩn)
        let supply_base = (supply * 1_000_000_000.0) as u128;
        Ok(ethers::types::U256::from(supply_base))
    }
    
    /// Phương thức fetch chuỗi blockchain
    async fn get_chain(&self) -> DmdChain {
        DmdChain::Solana
    }
    
    /// Phương thức fetch tên token
    fn token_name(&self) -> String {
        "Diamond Token".to_string()
    }
    
    /// Phương thức fetch ký hiệu token
    fn token_symbol(&self) -> String {
        "DMD".to_string()
    }
    
    /// Phương thức fetch số thập phân
    fn decimals(&self) -> u8 {
        9 // Solana thường dùng 9 decimals
    }
}