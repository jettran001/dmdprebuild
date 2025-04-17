//! Logic bridge cho các blockchain EVM làm spoke

use std::sync::Arc;
use std::collections::HashMap;
use anyhow::{Result, Context, anyhow};
use async_trait::async_trait;
use uuid::Uuid;
use ethers::types::{Address, U256, H256};
use ethers::utils::{parse_ether, parse_units};
use chrono::Utc;
use tokio::sync::RwLock;
use tracing::{info, debug, warn, error};

use crate::smartcontracts::{
    evm_provider::EvmProvider,
    dmd_token::{DmdChain, DmdTokenService},
};

use super::{
    error::{BridgeError, BridgeResult},
    traits::BridgeSpoke,
    types::{BridgeTransaction, BridgeStatus, BridgeConfig, BridgeTokenType},
};

/// ABI cần thiết cho ERC1155Wrapper contract
const ERC1155_WRAPPER_ABI: &str = r#"[
    {"inputs":[{"internalType":"address","name":"erc1155Address","type":"address"},{"internalType":"uint256","name":"tokenId","type":"uint256"},{"internalType":"uint256","name":"amount","type":"uint256"}],"name":"wrapToErc20","outputs":[{"internalType":"bool","name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},
    {"inputs":[{"internalType":"uint256","name":"amount","type":"uint256"}],"name":"unwrapToErc1155","outputs":[{"internalType":"bool","name":"","type":"bool"}],"stateMutability":"nonpayable","type":"function"},
    {"inputs":[{"internalType":"address","name":"owner","type":"address"}],"name":"balanceOf","outputs":[{"internalType":"uint256","name":"","type":"uint256"}],"stateMutability":"view","type":"function"}
]"#;

/// ABI cần thiết cho LayerZero Endpoint
const LAYERZERO_ENDPOINT_ABI: &str = r#"[
    {"inputs":[{"internalType":"uint16","name":"_dstChainId","type":"uint16"},{"internalType":"address","name":"_destination","type":"address"},{"internalType":"bytes","name":"_payload","type":"bytes"},{"internalType":"address payable","name":"_refundAddress","type":"address"},{"internalType":"address","name":"_zroPaymentAddress","type":"address"},{"internalType":"bytes","name":"_adapterParams","type":"bytes"}],"name":"send","outputs":[],"stateMutability":"payable","type":"function"},
    {"inputs":[{"internalType":"uint16","name":"_dstChainId","type":"uint16"},{"internalType":"bytes","name":"_destination","type":"bytes"},{"internalType":"bytes","name":"_payload","type":"bytes"},{"internalType":"address payable","name":"_refundAddress","type":"address"},{"internalType":"address","name":"_zroPaymentAddress","type":"address"},{"internalType":"bytes","name":"_adapterParams","type":"bytes"}],"name":"estimateFees","outputs":[{"internalType":"uint256","name":"nativeFee","type":"uint256"},{"internalType":"uint256","name":"zroFee","type":"uint256"}],"stateMutability":"view","type":"function"}
]"#;

/// Bridge spoke cho các chuỗi EVM (Ethereum, BSC, Arbitrum, v.v.)
pub struct EvmBridgeSpoke {
    /// Chain ID
    chain: DmdChain,
    /// Cấu hình bridge
    config: BridgeConfig,
    /// Provider cho blockchain EVM
    evm_provider: Option<Arc<EvmProvider>>,
    /// DMD Token Service
    dmd_service: Option<Arc<DmdTokenService>>,
    /// Cache các giao dịch bridge đang xử lý
    transactions: Arc<RwLock<HashMap<String, BridgeTransaction>>>,
    /// Địa chỉ contract bridge LayerZero
    bridge_contract_address: Option<String>,
    /// Địa chỉ contract ERC20 wrapper
    erc20_wrapper_address: Option<String>,
    /// Địa chỉ contract ERC1155
    erc1155_token_address: Option<String>,
    /// Địa chỉ LayerZero endpoint
    lz_endpoint_address: Option<String>,
    /// LayerZero chain ID mapping
    lz_chain_map: HashMap<DmdChain, u16>,
}

impl EvmBridgeSpoke {
    /// Tạo spoke bridge mới
    pub fn new(chain: DmdChain) -> Self {
        let mut lz_chain_map = HashMap::new();
        lz_chain_map.insert(DmdChain::Ethereum, 101);
        lz_chain_map.insert(DmdChain::BinanceSmartChain, 102);
        lz_chain_map.insert(DmdChain::Avalanche, 106);
        lz_chain_map.insert(DmdChain::Polygon, 109);
        lz_chain_map.insert(DmdChain::Arbitrum, 110);
        lz_chain_map.insert(DmdChain::Optimism, 111);
        lz_chain_map.insert(DmdChain::Fantom, 112);
        lz_chain_map.insert(DmdChain::Near, 115);

        Self {
            chain,
            config: BridgeConfig::default(),
            evm_provider: None,
            dmd_service: None,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            bridge_contract_address: None,
            erc20_wrapper_address: None,
            erc1155_token_address: None,
            lz_endpoint_address: None,
            lz_chain_map,
        }
    }

    /// Tạo spoke bridge với cấu hình tùy chỉnh
    pub fn with_config(chain: DmdChain, config: BridgeConfig) -> Self {
        let mut instance = Self::new(chain);
        instance.config = config;
        instance
    }
    
    /// Lấy LayerZero chain ID cho chain cụ thể
    fn get_lz_chain_id(&self, chain: DmdChain) -> BridgeResult<u16> {
        self.lz_chain_map
            .get(&chain)
            .copied()
            .ok_or_else(|| BridgeError::UnsupportedRoute(
                format!("Route không được hỗ trợ: {:?} -> {:?}", self.chain, chain)
            ))
    }
    
    /// Tạo transaction bridge mới
    fn create_transaction(
        &self,
        is_to_hub: bool,
        from_address: &str,
        to_address: &str,
        amount: U256,
    ) -> BridgeTransaction {
        let tx_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        
        let (from_chain, to_chain, source_token_type, target_token_type) = 
            if is_to_hub {
                // Gửi đến NEAR hub (EVM -> NEAR)
                (
                    self.chain,
                    DmdChain::Near,
                    BridgeTokenType::Erc20,
                    BridgeTokenType::Nep141,
                )
            } else {
                // Nhận từ NEAR hub (NEAR -> EVM)
                (
                    DmdChain::Near,
                    self.chain,
                    BridgeTokenType::Nep141,
                    BridgeTokenType::Erc20, // Sẽ unwrap thành ERC-1155 sau
                )
            };
        
        BridgeTransaction {
            id: tx_id,
            from_chain,
            to_chain,
            from_address: from_address.to_string(),
            to_address: to_address.to_string(),
            amount,
            fee: U256::zero(), // Will be updated later
            source_token_type,
            target_token_type,
            status: BridgeStatus::Initiated,
            source_tx_hash: None,
            target_tx_hash: None,
            initiated_at: now,
            updated_at: now,
            completed_at: None,
            error_message: None,
        }
    }
    
    /// Lưu thông tin giao dịch mới
    async fn store_transaction(&self, tx: BridgeTransaction) -> BridgeResult<()> {
        let mut txs = self.transactions.write().await;
        txs.insert(tx.id.clone(), tx);
        Ok(())
    }
    
    /// Cập nhật thông tin giao dịch
    async fn update_transaction(
        &self,
        tx_id: &str,
        status: BridgeStatus,
        source_tx_hash: Option<String>,
        target_tx_hash: Option<String>,
        error_message: Option<String>,
    ) -> BridgeResult<BridgeTransaction> {
        let mut txs = self.transactions.write().await;
        
        if let Some(tx) = txs.get_mut(tx_id) {
            tx.status = status;
            tx.updated_at = Utc::now();
            
            if let Some(hash) = source_tx_hash {
                tx.source_tx_hash = Some(hash);
            }
            
            if let Some(hash) = target_tx_hash {
                tx.target_tx_hash = Some(hash);
            }
            
            if let Some(error) = error_message {
                tx.error_message = Some(error);
            }
            
            if status == BridgeStatus::Completed || status == BridgeStatus::Failed {
                tx.completed_at = Some(Utc::now());
            }
            
            Ok(tx.clone())
        } else {
            Err(BridgeError::TransactionNotFound(tx_id.to_string()))
        }
    }
    
    /// Kiểm tra EVM provider đã được khởi tạo chưa
    fn ensure_provider_initialized(&self) -> BridgeResult<Arc<EvmProvider>> {
        self.evm_provider
            .clone()
            .ok_or_else(|| BridgeError::ProviderError(format!("EVM provider for {:?} not initialized", self.chain)))
    }
    
    /// Kiểm tra DMD service đã được khởi tạo chưa
    fn ensure_dmd_service_initialized(&self) -> BridgeResult<Arc<DmdTokenService>> {
        self.dmd_service
            .clone()
            .ok_or_else(|| BridgeError::ProviderError(format!("DMD token service for {:?} not initialized", self.chain)))
    }
    
    /// Kiểm tra địa chỉ bridge contract đã được cài đặt chưa
    fn ensure_bridge_contract_address(&self) -> BridgeResult<String> {
        self.bridge_contract_address
            .clone()
            .ok_or_else(|| BridgeError::ProviderError(format!("Bridge contract address for {:?} not set", self.chain)))
    }
    
    /// Kiểm tra địa chỉ ERC20 wrapper đã được cài đặt chưa
    fn ensure_erc20_wrapper_address(&self) -> BridgeResult<String> {
        self.erc20_wrapper_address
            .clone()
            .ok_or_else(|| BridgeError::ProviderError(format!("ERC20 wrapper address for {:?} not set", self.chain)))
    }
    
    /// Kiểm tra địa chỉ ERC1155 token contract đã được cài đặt chưa
    fn ensure_erc1155_token_address(&self) -> BridgeResult<String> {
        self.erc1155_token_address
            .clone()
            .ok_or_else(|| BridgeError::ProviderError(format!("ERC1155 token address for {:?} not set", self.chain)))
    }
    
    /// Kiểm tra địa chỉ LayerZero endpoint đã được cài đặt chưa
    fn ensure_lz_endpoint_address(&self) -> BridgeResult<String> {
        self.lz_endpoint_address
            .clone()
            .ok_or_else(|| BridgeError::ProviderError(format!("LayerZero endpoint address for {:?} not set", self.chain)))
    }
}

#[async_trait]
impl BridgeSpoke for EvmBridgeSpoke {
    /// Loại chain của spoke
    fn chain_type(&self) -> DmdChain {
        self.chain
    }
    
    /// Khởi tạo spoke với cấu hình cho trước
    async fn initialize(&mut self, config: BridgeConfig) -> BridgeResult<()> {
        self.config = config;
        
        // Thiết lập địa chỉ contract theo chain
        match self.chain {
            DmdChain::BinanceSmartChain => {
                self.bridge_contract_address = Some(self.config.bsc_erc20_bridge_address.clone());
                self.erc20_wrapper_address = Some(self.config.bsc_erc20_bridge_address.clone());
                self.erc1155_token_address = Some(self.config.bsc_erc1155_address.clone());
                self.lz_endpoint_address = Some(self.config.bsc_lz_endpoint.clone());
            },
            DmdChain::Ethereum => {
                self.bridge_contract_address = Some(self.config.eth_erc20_bridge_address.clone());
                self.erc20_wrapper_address = Some(self.config.eth_erc20_bridge_address.clone());
                self.erc1155_token_address = Some(self.config.eth_erc1155_address.clone());
                self.lz_endpoint_address = Some(self.config.eth_lz_endpoint.clone());
            },
            _ => return Err(BridgeError::UnsupportedChain(format!("{:?} không được hỗ trợ hiện tại", self.chain))),
        }
        
        // Khởi tạo EVM provider
        let provider_config = match self.chain {
            DmdChain::Ethereum => "ethereum",
            DmdChain::BinanceSmartChain => "bsc",
            DmdChain::Polygon => "polygon",
            DmdChain::Avalanche => "avalanche",
            DmdChain::Arbitrum => "arbitrum",
            DmdChain::Optimism => "optimism",
            DmdChain::Base => "base",
            _ => return Err(BridgeError::UnsupportedChain(format!("{:?}", self.chain))),
        };
        
        let evm_provider = EvmProvider::new(provider_config, None)
            .await
            .map_err(|e| BridgeError::ProviderError(format!("Không thể tạo EVM provider: {}", e)))?;
        
        self.evm_provider = Some(Arc::new(evm_provider.clone()));
        
        // Khởi tạo DMD Token Service
        let dmd_service = DmdTokenService::new(&evm_provider)
            .await
            .map_err(|e| BridgeError::ProviderError(format!("Không thể tạo DMD token service: {}", e)))?;
        
        self.dmd_service = Some(Arc::new(dmd_service));
        
        info!(
            "{:?} bridge spoke đã khởi tạo với bridge contract: {}, ERC1155: {}",
            self.chain, 
            self.bridge_contract_address.clone().unwrap_or_default(),
            self.erc1155_token_address.clone().unwrap_or_default()
        );
        
        Ok(())
    }
    
    /// Gửi token từ spoke sang hub (wrap ERC-1155 -> ERC-20 -> NEP-141)
    async fn send_to_hub(
        &self,
        private_key: &str,
        near_account: &str,
        amount: U256
    ) -> BridgeResult<BridgeTransaction> {
        // Kiểm tra provider đã được khởi tạo
        let evm_provider = self.ensure_provider_initialized()?;
        
        // Lấy địa chỉ ví từ private key
        let wallet = evm_provider.get_wallet_from_private_key(private_key)
            .map_err(|e| BridgeError::ProviderError(format!("Không thể tạo ví: {}", e)))?;
        let from_address = wallet.address().to_string();
        
        // Kiểm tra số dư token trước khi thực hiện giao dịch
        let dmd_service = self.ensure_dmd_service_initialized()?;
        let wrapper_address = self.ensure_erc20_wrapper_address()?;
        
        // Kiểm tra số dư ERC-1155
        let erc1155_balance = dmd_service.get_balance(&from_address)
            .await
            .map_err(|e| BridgeError::ProviderError(format!("Không thể lấy số dư: {}", e)))?;
            
        if erc1155_balance < amount {
            return Err(BridgeError::InvalidAmount(
                format!("Số dư không đủ: có {}, cần {}", erc1155_balance, amount)
            ));
        }
        
        // Tạo giao dịch bridge
        let tx = self.create_transaction(true, &from_address, near_account, amount);
        self.store_transaction(tx.clone()).await?;
        
        // Cập nhật trạng thái
        let tx_id = tx.id.clone();
        self.update_transaction(
            &tx_id,
            BridgeStatus::TokenProcessing,
            None,
            None,
            None
        ).await?;
        
        // Wrap ERC-1155 thành ERC-20
        debug!("Wrapping {} DMD từ ERC-1155 sang ERC-20", amount);
        let wrap_tx_hash = self.wrap_erc1155_to_erc20(private_key, amount).await?;
        
        // Cập nhật hash giao dịch wrap
        self.update_transaction(
            &tx_id,
            BridgeStatus::TokenProcessing,
            Some(wrap_tx_hash.clone()),
            None,
            None
        ).await?;
        
        // Chờ xác nhận giao dịch wrap với timeout
        let wrap_success = self.wait_for_transaction_confirmation(&wrap_tx_hash, self.config.confirmation_timeout)
            .await
            .map_err(|e| BridgeError::TransactionFailed(format!("Wrap transaction failed: {}", e)))?;
            
        if !wrap_success {
            return Err(BridgeError::TransactionFailed("Wrap transaction không thành công".to_string()));
        }
        
        // Lấy địa chỉ bridge contract và LayerZero endpoint
        let bridge_contract = self.ensure_bridge_contract_address()?;
        let lz_endpoint = self.ensure_lz_endpoint_address()?;
        
        // Cập nhật trạng thái
        self.update_transaction(
            &tx_id,
            BridgeStatus::AwaitingRelayer,
            Some(wrap_tx_hash.clone()),
            None,
            None
        ).await?;
        
        // Ước tính phí bridge
        let fee = self.estimate_fee(amount).await?;
        
        // Tạo contract interface cho bridge
        let bridge_contract_instance = evm_provider
            .create_contract_instance(&bridge_contract, ERC1155_WRAPPER_ABI)
            .map_err(|e| BridgeError::ProviderError(format!("Không thể tạo contract instance: {}", e)))?;
        
        // Lấy LZ chain ID cho NEAR
        let near_lz_id = self.get_lz_chain_id(DmdChain::Near)?;
        
        // Gửi token qua bridge
        debug!("Sending {} DMD từ {} đến {}", amount, from_address, near_account);
        let bridge_tx = bridge_contract_instance
            .send_with_value(
                near_lz_id,
                near_account.to_string(),
                amount,
                from_address.clone(),
                fee,
            )
            .await
            .map_err(|e| BridgeError::TransactionFailed(format!("Bridge transaction failed: {}", e)))?;
        
        // Lấy hash giao dịch bridge
        let bridge_tx_hash = bridge_tx.transaction_hash.to_string();
        
        // Cập nhật hash giao dịch bridge
        self.update_transaction(
            &tx_id,
            BridgeStatus::Sending,
            Some(bridge_tx_hash.clone()),
            None,
            None
        ).await?;
        
        // Chờ xác nhận giao dịch bridge với timeout
        let bridge_success = self.wait_for_transaction_confirmation(&bridge_tx_hash, self.config.confirmation_timeout)
            .await
            .map_err(|e| BridgeError::TransactionFailed(format!("Bridge transaction failed: {}", e)))?;
            
        if !bridge_success {
            return Err(BridgeError::TransactionFailed("Bridge transaction không thành công".to_string()));
        }
        
        // Cập nhật trạng thái giao dịch
        self.update_transaction(
            &tx_id,
            BridgeStatus::AwaitingConfirmation,
            Some(bridge_tx_hash),
            None,
            None
        ).await?;
        
        // Lấy thông tin giao dịch đã cập nhật
        let updated_tx = self.get_transaction(&tx_id).await?;
        
        Ok(updated_tx)
    }
    
    /// Chờ xác nhận giao dịch với timeout
    async fn wait_for_transaction_confirmation(&self, tx_hash: &str, timeout_seconds: u64) -> BridgeResult<bool> {
        // Kiểm tra provider đã được khởi tạo
        let evm_provider = self.ensure_provider_initialized()?;
        
        // Tạo deadline dựa trên timeout
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_seconds);
        
        // Poll cho đến khi nhận được kết quả hoặc timeout
        debug!("Waiting for transaction confirmation: {}", tx_hash);
        
        // Cố gắng chờ trong khoảng thời gian timeout
        while std::time::Instant::now() < deadline {
            // Kiểm tra trạng thái giao dịch
            match evm_provider.get_transaction_receipt(tx_hash).await {
                Ok(Some(receipt)) => {
                    // Kiểm tra xem giao dịch đã thành công chưa
                    if receipt.status.unwrap_or_default().as_u64() == 1 {
                        debug!("Transaction confirmed successfully: {}", tx_hash);
                        return Ok(true);
                    } else {
                        // Giao dịch thất bại
                        error!("Transaction failed: {}", tx_hash);
                        return Err(BridgeError::TransactionFailed(format!("Transaction failed with status 0: {}", tx_hash)));
                    }
                },
                Ok(None) => {
                    // Giao dịch vẫn đang pending, đợi thêm
                    debug!("Transaction still pending, waiting: {}", tx_hash);
                    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                },
                Err(e) => {
                    // Lỗi khi kiểm tra giao dịch, thử lại
                    warn!("Error checking transaction status, retrying: {}: {}", tx_hash, e);
                    tokio::time::sleep(std::time::Duration::from_millis(5000)).await;
                }
            }
        }
        
        // Nếu đã hết timeout mà vẫn chưa nhận được kết quả
        error!("Transaction confirmation timed out after {} seconds: {}", timeout_seconds, tx_hash);
        Err(BridgeError::TransactionFailed(format!("Timeout waiting for confirmation after {} seconds: {}", 
            timeout_seconds, tx_hash)))
    }
    
    /// Nhận token từ hub (unwrap NEP-141 -> ERC-20 -> ERC-1155)
    async fn receive_from_hub(
        &self,
        tx_proof: &str,
        to_address: &str,
        amount: U256
    ) -> BridgeResult<BridgeTransaction> {
        // Kiểm tra địa chỉ EVM hợp lệ
        if !self.is_address_valid(to_address) {
            return Err(BridgeError::InvalidAddress(format!("Địa chỉ EVM không hợp lệ: {}", to_address)));
        }
        
        // Tạo thông tin giao dịch
        let from_near = "near.bridge.testnet";
        let tx = self.create_transaction(false, from_near, to_address, amount);
        self.store_transaction(tx.clone()).await?;
        
        // Cập nhật trạng thái
        let tx = self.update_transaction(
            &tx.id,
            BridgeStatus::Processing,
            None,
            None,
            None,
        ).await?;
        
        // Trong thực tế, chúng ta sẽ kiểm tra proof từ NEAR và xác thực
        // Sau đó gọi hàm unwrap để chuyển đổi từ ERC20 thành ERC1155
        // Giả lập tx hash cho việc nhận token từ NEAR (thông qua LayerZero)
        let receive_tx_hash = format!("0x{}", Uuid::new_v4().to_string().replace("-", ""));
        
        // Cập nhật trạng thái
        let tx = self.update_transaction(
            &tx.id,
            BridgeStatus::TokenProcessing,
            None,
            Some(receive_tx_hash),
            None,
        ).await?;
        
        // Giả lập unwrap
        let unwrap_tx_hash = format!("0x{}", Uuid::new_v4().to_string().replace("-", ""));
        
        // Cập nhật trạng thái hoàn thành
        let updated_tx = self.update_transaction(
            &tx.id,
            BridgeStatus::Completed,
            None,
            Some(unwrap_tx_hash),
            None,
        ).await?;
        
        info!(
            "Bridge transaction completed: NEAR -> {:?}, to: {}, amount: {}, id: {}",
            self.chain, to_address, amount, tx.id
        );
        
        Ok(updated_tx)
    }
    
    /// Wrap token từ ERC-1155 sang ERC-20
    async fn wrap_erc1155_to_erc20(
        &self,
        private_key: &str,
        amount: U256
    ) -> BridgeResult<String> {
        let evm_provider = self.ensure_provider_initialized()?;
        let erc1155_address = self.ensure_erc1155_token_address()?;
        let wrapper_address = self.ensure_erc20_wrapper_address()?;
        
        // Trong thực tế, chúng ta sẽ:
        // 1. Approve ERC1155 cho wrapper contract
        // 2. Gọi hàm wrap từ wrapper contract
        
        // Giả lập transaction hash
        let tx_hash = format!("0x{}", Uuid::new_v4().to_string().replace("-", ""));
        
        info!(
            "Wrapped ERC1155 to ERC20: token_id: {}, amount: {}, tx: {}",
            self.config.dmd_token_id, amount, tx_hash
        );
        
        Ok(tx_hash)
    }
    
    /// Unwrap token từ ERC-20 sang ERC-1155
    async fn unwrap_erc20_to_erc1155(
        &self,
        private_key: &str,
        amount: U256
    ) -> BridgeResult<String> {
        let evm_provider = self.ensure_provider_initialized()?;
        let wrapper_address = self.ensure_erc20_wrapper_address()?;
        
        // Trong thực tế, chúng ta sẽ:
        // 1. Gọi hàm unwrap từ wrapper contract
        
        // Giả lập transaction hash
        let tx_hash = format!("0x{}", Uuid::new_v4().to_string().replace("-", ""));
        
        info!(
            "Unwrapped ERC20 to ERC1155: token_id: {}, amount: {}, tx: {}",
            self.config.dmd_token_id, amount, tx_hash
        );
        
        Ok(tx_hash)
    }
    
    /// Kiểm tra trạng thái giao dịch
    async fn check_transaction_status(&self, tx_hash: &str) -> BridgeResult<BridgeStatus> {
        let txs = self.transactions.read().await;
        
        // Tìm theo transaction hash
        for tx in txs.values() {
            if let Some(hash) = &tx.source_tx_hash {
                if hash == tx_hash {
                    return Ok(tx.status);
                }
            }
            
            if let Some(hash) = &tx.target_tx_hash {
                if hash == tx_hash {
                    return Ok(tx.status);
                }
            }
        }
        
        Err(BridgeError::TransactionNotFound(format!("Không tìm thấy giao dịch với hash: {}", tx_hash)))
    }
    
    /// Lấy thông tin giao dịch bridge
    async fn get_transaction(&self, tx_id: &str) -> BridgeResult<BridgeTransaction> {
        let txs = self.transactions.read().await;
        
        txs.get(tx_id)
            .cloned()
            .ok_or_else(|| BridgeError::TransactionNotFound(format!("Không tìm thấy giao dịch với ID: {}", tx_id)))
    }
    
    /// Ước tính phí bridge
    async fn estimate_fee(&self, amount: U256) -> BridgeResult<U256> {
        let evm_provider = self.ensure_provider_initialized()?;
        let lz_endpoint = self.ensure_lz_endpoint_address()?;
        
        // Lấy LayerZero chain ID cho NEAR
        let destination_chain_id = self.get_lz_chain_id(DmdChain::Near)?;
        
        // Base fee cho wrap/unwrap
        let wrap_gas = U256::from(150_000u64); // Ước tính 150k gas cho wrap
        
        // Gas price ước tính
        let gas_price = match self.chain {
            DmdChain::Ethereum => U256::from(50_000_000_000u64), // 50 gwei
            DmdChain::BinanceSmartChain => U256::from(5_000_000_000u64),       // 5 gwei
            DmdChain::Avalanche => U256::from(25_000_000_000u64), // 25 gwei
            DmdChain::Polygon => U256::from(100_000_000_000u64),  // 100 gwei
            DmdChain::Arbitrum => U256::from(1_000_000_000u64),   // 1 gwei
            DmdChain::Optimism => U256::from(1_000_000u64),       // 0.001 gwei
            _ => U256::from(10_000_000_000u64),                   // 10 gwei default
        };
        
        // Phí wrap
        let wrap_fee = gas_price.checked_mul(wrap_gas)
            .ok_or_else(|| BridgeError::SystemError("Tràn số khi tính phí wrap".to_string()))?;
        
        // LayerZero fee (ước tính)
        let lz_fee = U256::from(0.002 * 1e18 as u64); // ~0.002 ETH hoặc tương đương
        
        // Tổng phí = wrap_fee + lz_fee
        let total_fee = wrap_fee.checked_add(lz_fee)
            .ok_or_else(|| BridgeError::SystemError("Tràn số khi tính tổng phí".to_string()))?;
        
        Ok(total_fee)
    }
    
    /// Kiểm tra xem spoke có hỗ trợ một địa chỉ cụ thể
    fn is_address_valid(&self, address: &str) -> bool {
        // Xác thực địa chỉ EVM
        address.starts_with("0x") && address.len() == 42 && 
            address.chars().skip(2).all(|c| c.is_ascii_hexdigit())
    }
} 