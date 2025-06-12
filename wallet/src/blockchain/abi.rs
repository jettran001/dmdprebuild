//! Module xử lý ABI (Application Binary Interface) cho smart contracts
//!
//! Module này cung cấp các chức năng:
//! - Phân tích cú pháp ABI từ JSON
//! - Encode/decode dữ liệu gọi hàm
//! - Xử lý sự kiện và logs từ blockchain
//! - Tối ưu hóa hiệu năng khi làm việc với ABI

// External imports
use std::sync::Arc;
use ethers::abi::{
    Abi, AbiParser, AbiError, Contract, Event, Function,
    Token, Param, ParamType
};
use ethers::types::{H256, U256, Address, Bytes};
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use anyhow::{Result, Context, anyhow};
use tracing::{debug, error, info, warn};

// Internal imports
use crate::error::WalletError;

/// Bộ xử lý ABI cung cấp các phương thức làm việc với smart contract ABI
#[derive(Debug, Clone)]
pub struct AbiHandler {
    /// ABI đã phân tích
    abi: Abi,
    /// Cache các hàm đã tìm thấy bằng tên
    function_cache: Arc<std::sync::Mutex<std::collections::HashMap<String, Function>>>,
    /// Cache các sự kiện đã tìm thấy bằng tên
    event_cache: Arc<std::sync::Mutex<std::collections::HashMap<String, Event>>>,
}

impl AbiHandler {
    /// Tạo AbiHandler mới từ chuỗi JSON ABI
    ///
    /// # Arguments
    ///
    /// * `abi_str` - Chuỗi JSON chứa ABI
    ///
    /// # Returns
    ///
    /// * `Result<Self>` - Kết quả chứa AbiHandler hoặc lỗi
    pub fn new(abi_str: &str) -> Result<Self> {
        let abi = serde_json::from_str::<Abi>(abi_str)
            .context("Không thể phân tích ABI từ JSON")?;
        
        Ok(Self {
            abi,
            function_cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            event_cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        })
    }
    
    /// Tạo AbiHandler mới từ đối tượng Abi
    ///
    /// # Arguments
    ///
    /// * `abi` - Đối tượng ABI đã phân tích
    ///
    /// # Returns
    ///
    /// * `Self` - Instance AbiHandler
    pub fn from_parsed_abi(abi: Abi) -> Self {
        Self {
            abi,
            function_cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            event_cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }
    
    /// Lấy ABI đã phân tích
    pub fn get_abi(&self) -> &Abi {
        &self.abi
    }
    
    /// Mã hóa tham số hàm thành calldata
    ///
    /// # Arguments
    ///
    /// * `function_name` - Tên hàm cần gọi
    /// * `args` - Danh sách tham số
    ///
    /// # Returns
    ///
    /// * `Result<Bytes>` - Calldata đã mã hóa hoặc lỗi
    pub fn encode_function_data(&self, function_name: &str, args: &[Token]) -> Result<Bytes> {
        // Thử lấy từ cache trước
        let function = {
            let cache = self.function_cache.lock()
                .map_err(|_| anyhow!("Không thể khóa function_cache"))?;
            
            cache.get(function_name).cloned()
        };
        
        // Nếu không có trong cache, tìm trong ABI
        let function = match function {
            Some(f) => f,
            None => {
                let f = self.abi.function(function_name)
                    .map_err(|e| anyhow!("Không tìm thấy hàm {}: {}", function_name, e))?
                    .clone();
                
                // Thêm vào cache
                let mut cache = self.function_cache.lock()
                    .map_err(|_| anyhow!("Không thể khóa function_cache"))?;
                cache.insert(function_name.to_string(), f.clone());
                
                f
            }
        };
        
        // Mã hóa calldata
        let encoded = function.encode_input(args)
            .map_err(|e| anyhow!("Lỗi khi mã hóa tham số cho hàm {}: {}", function_name, e))?;
        
        Ok(Bytes::from(encoded))
    }
    
    /// Giải mã kết quả từ calldata
    ///
    /// # Arguments
    ///
    /// * `function_name` - Tên hàm đã gọi
    /// * `data` - Dữ liệu trả về từ blockchain
    ///
    /// # Returns
    ///
    /// * `Result<Vec<Token>>` - Danh sách token đã giải mã hoặc lỗi
    pub fn decode_function_output(&self, function_name: &str, data: &[u8]) -> Result<Vec<Token>> {
        // Thử lấy từ cache trước
        let function = {
            let cache = self.function_cache.lock()
                .map_err(|_| anyhow!("Không thể khóa function_cache"))?;
            
            cache.get(function_name).cloned()
        };
        
        // Nếu không có trong cache, tìm trong ABI
        let function = match function {
            Some(f) => f,
            None => {
                let f = self.abi.function(function_name)
                    .map_err(|e| anyhow!("Không tìm thấy hàm {}: {}", function_name, e))?
                    .clone();
                
                // Thêm vào cache
                let mut cache = self.function_cache.lock()
                    .map_err(|_| anyhow!("Không thể khóa function_cache"))?;
                cache.insert(function_name.to_string(), f.clone());
                
                f
            }
        };
        
        // Giải mã kết quả
        let decoded = function.decode_output(data)
            .map_err(|e| anyhow!("Lỗi khi giải mã kết quả từ hàm {}: {}", function_name, e))?;
        
        Ok(decoded)
    }
    
    /// Giải mã sự kiện từ log
    ///
    /// # Arguments
    ///
    /// * `event_name` - Tên sự kiện
    /// * `topics` - Danh sách topics từ log
    /// * `data` - Dữ liệu của log
    ///
    /// # Returns
    ///
    /// * `Result<Vec<Token>>` - Danh sách token đã giải mã hoặc lỗi
    pub fn decode_event(&self, event_name: &str, topics: &[H256], data: &[u8]) -> Result<Vec<Token>> {
        // Thử lấy từ cache trước
        let event = {
            let cache = self.event_cache.lock()
                .map_err(|_| anyhow!("Không thể khóa event_cache"))?;
            
            cache.get(event_name).cloned()
        };
        
        // Nếu không có trong cache, tìm trong ABI
        let event = match event {
            Some(e) => e,
            None => {
                let e = self.abi.event(event_name)
                    .map_err(|e| anyhow!("Không tìm thấy sự kiện {}: {}", event_name, e))?
                    .clone();
                
                // Thêm vào cache
                let mut cache = self.event_cache.lock()
                    .map_err(|_| anyhow!("Không thể khóa event_cache"))?;
                cache.insert(event_name.to_string(), e.clone());
                
                e
            }
        };
        
        // Giải mã sự kiện
        let raw_log = ethers::types::Log {
            address: Address::zero(), // Không quan trọng cho giải mã
            topics: topics.to_vec(),
            data: Bytes::from(data.to_vec()),
            block_hash: None,
            block_number: None,
            transaction_hash: None,
            transaction_index: None,
            log_index: None,
            transaction_log_index: None,
            log_type: None,
            removed: None,
        };
        
        let decoded = event.parse_log(raw_log.into())
            .map_err(|e| anyhow!("Lỗi khi giải mã sự kiện {}: {}", event_name, e))?;
        
        Ok(decoded.params.into_iter().map(|param| param.value).collect())
    }
    
    /// Tìm kiếm sự kiện được kích hoạt trong logs
    ///
    /// # Arguments
    ///
    /// * `event_name` - Tên sự kiện
    /// * `logs` - Danh sách logs từ transaction receipt
    ///
    /// # Returns
    ///
    /// * `Result<Vec<(Vec<Token>, usize)>>` - Danh sách kết quả với token và vị trí trong logs
    pub fn find_events_in_logs(&self, event_name: &str, logs: &[ethers::types::Log]) -> Result<Vec<(Vec<Token>, usize)>> {
        // Lấy sự kiện từ ABI
        let event = self.abi.event(event_name)
            .map_err(|e| anyhow!("Không tìm thấy sự kiện {}: {}", event_name, e))?;
        
        let mut results = Vec::new();
        
        // Lấy topic0 (event signature)
        let event_sig = event.signature();
        
        // Tìm trong tất cả logs
        for (idx, log) in logs.iter().enumerate() {
            if log.topics.is_empty() {
                continue;
            }
            
            // Kiểm tra xem log có match với sự kiện không
            if log.topics[0] == event_sig {
                match event.parse_log(log.clone().into()) {
                    Ok(parsed) => {
                        let tokens = parsed.params.into_iter().map(|param| param.value).collect();
                        results.push((tokens, idx));
                    }
                    Err(e) => {
                        warn!("Không thể giải mã log tại vị trí {} cho sự kiện {}: {}", idx, event_name, e);
                    }
                }
            }
        }
        
        Ok(results)
    }
    
    /// Lấy danh sách tất cả hàm trong ABI
    pub fn get_functions(&self) -> Vec<String> {
        self.abi.functions().map(|f| f.name.clone()).collect()
    }
    
    /// Lấy danh sách tất cả sự kiện trong ABI
    pub fn get_events(&self) -> Vec<String> {
        self.abi.events().map(|e| e.name.clone()).collect()
    }
    
    /// Tạo signature cho hàm
    ///
    /// # Arguments
    ///
    /// * `function_name` - Tên hàm
    ///
    /// # Returns
    ///
    /// * `Result<[u8; 4]>` - Function selector (4 bytes đầu tiên của keccak hash)
    pub fn get_function_selector(&self, function_name: &str) -> Result<[u8; 4]> {
        let function = self.abi.function(function_name)
            .map_err(|e| anyhow!("Không tìm thấy hàm {}: {}", function_name, e))?;
        
        Ok(function.selector())
    }
    
    /// Xác định hàm từ calldata
    ///
    /// # Arguments
    ///
    /// * `data` - Calldata cần phân tích
    ///
    /// # Returns
    ///
    /// * `Result<(Function, Vec<Token>)>` - Hàm và các tham số đã giải mã
    pub fn decode_function_call(&self, data: &[u8]) -> Result<(Function, Vec<Token>)> {
        if data.len() < 4 {
            return Err(anyhow!("Calldata quá ngắn, cần ít nhất 4 bytes"));
        }
        
        // Lấy function selector (4 bytes đầu tiên)
        let mut selector = [0u8; 4];
        selector.copy_from_slice(&data[..4]);
        
        // Tìm hàm phù hợp trong ABI
        for function in self.abi.functions() {
            if function.selector() == selector {
                // Giải mã tham số
                let decoded = function.decode_input(&data[4..])
                    .map_err(|e| anyhow!("Lỗi khi giải mã tham số: {}", e))?;
                
                return Ok((function.clone(), decoded));
            }
        }
        
        Err(anyhow!("Không tìm thấy hàm nào phù hợp với selector {:?}", selector))
    }
    
    /// Hiển thị thông tin ABI dưới dạng chuỗi dễ đọc
    pub fn display_abi_info(&self) -> String {
        let mut result = String::new();
        
        result.push_str("=== FUNCTIONS ===\n");
        for function in self.abi.functions() {
            result.push_str(&format!("{}(", function.name));
            
            let params: Vec<String> = function.inputs
                .iter()
                .map(|param| format!("{}: {}", param.name, param.kind))
                .collect();
            
            result.push_str(&params.join(", "));
            result.push_str(")");
            
            // Thêm thông tin outputs
            if !function.outputs.is_empty() {
                result.push_str(" -> (");
                let outputs: Vec<String> = function.outputs
                    .iter()
                    .map(|param| format!("{}: {}", param.name, param.kind))
                    .collect();
                
                result.push_str(&outputs.join(", "));
                result.push_str(")");
            }
            
            // Thêm thông tin state mutability
            result.push_str(&format!(" [{}]\n", function.state_mutability));
        }
        
        result.push_str("\n=== EVENTS ===\n");
        for event in self.abi.events() {
            result.push_str(&format!("{}(", event.name));
            
            let params: Vec<String> = event.inputs
                .iter()
                .map(|param| {
                    let indexed = if param.indexed { " indexed" } else { "" };
                    format!("{}{}: {}", param.name, indexed, param.kind)
                })
                .collect();
            
            result.push_str(&params.join(", "));
            result.push_str(")\n");
        }
        
        result
    }
}

/// Tạo function selector từ signature
///
/// # Arguments
///
/// * `signature` - Chuỗi signature (ví dụ: "transfer(address,uint256)")
///
/// # Returns
///
/// * `[u8; 4]` - Function selector (4 bytes đầu tiên của keccak hash)
pub fn function_selector(signature: &str) -> [u8; 4] {
    let hash = ethers::utils::keccak256(signature.as_bytes());
    let mut selector = [0u8; 4];
    selector.copy_from_slice(&hash[..4]);
    selector
}

/// Tạo topic từ event signature
///
/// # Arguments
///
/// * `signature` - Chuỗi signature (ví dụ: "Transfer(address,address,uint256)")
///
/// # Returns
///
/// * `H256` - Event topic
pub fn event_topic(signature: &str) -> H256 {
    let hash = ethers::utils::keccak256(signature.as_bytes());
    H256::from_slice(&hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::types::H160;
    
    const TEST_ABI: &str = r#"[
        {
            "inputs": [
                {"internalType": "address", "name": "recipient", "type": "address"},
                {"internalType": "uint256", "name": "amount", "type": "uint256"}
            ],
            "name": "transfer",
            "outputs": [{"internalType": "bool", "name": "", "type": "bool"}],
            "stateMutability": "nonpayable",
            "type": "function"
        },
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "internalType": "address", "name": "from", "type": "address"},
                {"indexed": true, "internalType": "address", "name": "to", "type": "address"},
                {"indexed": false, "internalType": "uint256", "name": "value", "type": "uint256"}
            ],
            "name": "Transfer",
            "type": "event"
        }
    ]"#;
    
    #[test]
    fn test_abi_handler_creation() {
        let handler = AbiHandler::new(TEST_ABI);
        assert!(handler.is_ok());
    }
    
    #[test]
    fn test_encode_function_data() {
        let handler = AbiHandler::new(TEST_ABI).unwrap();
        
        let recipient = Token::Address(H160::from_low_u64_be(1));
        let amount = Token::Uint(U256::from(100));
        
        let result = handler.encode_function_data("transfer", &[recipient, amount]);
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_function_selector() {
        // transfer(address,uint256) = 0xa9059cbb
        let selector = function_selector("transfer(address,uint256)");
        assert_eq!(selector, [0xa9, 0x05, 0x9c, 0xbb]);
    }
    
    #[test]
    fn test_event_topic() {
        // Transfer(address,address,uint256)
        let topic = event_topic("Transfer(address,address,uint256)");
        let expected = H256::from_slice(&ethers::utils::keccak256("Transfer(address,address,uint256)".as_bytes()));
        assert_eq!(topic, expected);
    }
} 