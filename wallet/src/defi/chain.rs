use std::fmt;
use std::str::FromStr;
use serde::{Serialize, Deserialize};
use ethers::types::Chain as EthersChain;

/// Chain ID cho các blockchain được hỗ trợ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChainId {
    /// Ethereum Mainnet
    EthereumMainnet,
    /// Ethereum Goerli Testnet
    EthereumGoerli,
    /// Ethereum Sepolia Testnet
    EthereumSepolia,
    /// Binance Smart Chain Mainnet
    BscMainnet,
    /// Binance Smart Chain Testnet
    BscTestnet,
    /// Polygon Mainnet
    PolygonMainnet,
    /// Polygon Mumbai Testnet
    PolygonMumbai,
    /// Arbitrum One Mainnet
    ArbitrumMainnet,
    /// Arbitrum Goerli Testnet
    ArbitrumGoerli,
    /// Avalanche C-Chain
    AvalancheMainnet,
    /// Avalanche Fuji Testnet
    AvalancheFuji,
    /// Optimism Mainnet
    OptimismMainnet,
    /// Optimism Goerli Testnet
    OptimismGoerli,
    /// Base Mainnet
    BaseMainnet,
    /// Base Goerli Testnet
    BaseGoerli,
    /// zkSync Era Mainnet
    ZkSyncMainnet,
    /// zkSync Era Testnet
    ZkSyncTestnet,
    /// Solana Mainnet
    SolanaMainnet,
    /// Solana Devnet
    SolanaDevnet,
    /// Solana Testnet
    SolanaTestnet,
    /// Near Mainnet
    NearMainnet,
    /// Near Testnet
    NearTestnet,
    /// Tron Mainnet
    TronMainnet,
    /// Tron Nile Testnet
    TronNile,
    /// Tron Shasta Testnet
    TronShasta,
    /// Diamond Mainnet
    DiamondMainnet,
    /// Diamond Testnet
    DiamondTestnet,
    /// Cosmos Hub Mainnet
    CosmosMainnet,
    /// Cosmos Testnet
    CosmosTestnet,
    /// Hedera Mainnet
    HederaMainnet,
    /// Hedera Testnet
    HederaTestnet,
}

impl fmt::Display for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EthereumMainnet => write!(f, "Ethereum Mainnet"),
            Self::EthereumGoerli => write!(f, "Ethereum Goerli Testnet"),
            Self::EthereumSepolia => write!(f, "Ethereum Sepolia Testnet"),
            Self::BscMainnet => write!(f, "Binance Smart Chain Mainnet"),
            Self::BscTestnet => write!(f, "Binance Smart Chain Testnet"),
            Self::PolygonMainnet => write!(f, "Polygon Mainnet"),
            Self::PolygonMumbai => write!(f, "Polygon Mumbai Testnet"),
            Self::ArbitrumMainnet => write!(f, "Arbitrum One Mainnet"),
            Self::ArbitrumGoerli => write!(f, "Arbitrum Goerli Testnet"),
            Self::AvalancheMainnet => write!(f, "Avalanche C-Chain"),
            Self::AvalancheFuji => write!(f, "Avalanche Fuji Testnet"),
            Self::OptimismMainnet => write!(f, "Optimism Mainnet"),
            Self::OptimismGoerli => write!(f, "Optimism Goerli Testnet"),
            Self::BaseMainnet => write!(f, "Base Mainnet"),
            Self::BaseGoerli => write!(f, "Base Goerli Testnet"),
            Self::ZkSyncMainnet => write!(f, "zkSync Era Mainnet"),
            Self::ZkSyncTestnet => write!(f, "zkSync Era Testnet"),
            Self::SolanaMainnet => write!(f, "Solana Mainnet"),
            Self::SolanaDevnet => write!(f, "Solana Devnet"),
            Self::SolanaTestnet => write!(f, "Solana Testnet"),
            Self::NearMainnet => write!(f, "Near Mainnet"),
            Self::NearTestnet => write!(f, "Near Testnet"),
            Self::TronMainnet => write!(f, "Tron Mainnet"),
            Self::TronNile => write!(f, "Tron Nile Testnet"),
            Self::TronShasta => write!(f, "Tron Shasta Testnet"),
            Self::DiamondMainnet => write!(f, "Diamond Mainnet"),
            Self::DiamondTestnet => write!(f, "Diamond Testnet"),
            Self::CosmosMainnet => write!(f, "Cosmos Hub Mainnet"),
            Self::CosmosTestnet => write!(f, "Cosmos Testnet"),
            Self::HederaMainnet => write!(f, "Hedera Mainnet"),
            Self::HederaTestnet => write!(f, "Hedera Testnet"),
        }
    }
}

impl ChainId {
    /// Chuyển đổi sang EthersChain
    pub fn to_ethers_chain(&self) -> Option<EthersChain> {
        match self {
            Self::EthereumMainnet => Some(EthersChain::Mainnet),
            Self::EthereumGoerli => Some(EthersChain::Goerli),
            Self::EthereumSepolia => Some(EthersChain::Sepolia),
            Self::BscMainnet => Some(EthersChain::BinanceSmartChain),
            Self::BscTestnet => Some(EthersChain::BinanceSmartChainTestnet),
            Self::PolygonMainnet => Some(EthersChain::Polygon),
            Self::PolygonMumbai => Some(EthersChain::PolygonMumbai),
            Self::ArbitrumMainnet => Some(EthersChain::ArbitrumOne),
            Self::ArbitrumGoerli => Some(EthersChain::ArbitrumGoerli),
            Self::AvalancheMainnet => Some(EthersChain::Avalanche),
            Self::AvalancheFuji => Some(EthersChain::AvalancheFuji),
            Self::OptimismMainnet => Some(EthersChain::Optimism),
            Self::OptimismGoerli => Some(EthersChain::OptimismGoerli),
            Self::BaseMainnet => Some(EthersChain::Base),
            Self::BaseGoerli => Some(EthersChain::BaseGoerli),
            _ => None,
        }
    }
    
    /// Lấy chain ID dạng số
    pub fn as_u64(&self) -> u64 {
        match self {
            Self::EthereumMainnet => 1,
            Self::EthereumGoerli => 5,
            Self::EthereumSepolia => 11155111,
            Self::BscMainnet => 56,
            Self::BscTestnet => 97,
            Self::PolygonMainnet => 137,
            Self::PolygonMumbai => 80001,
            Self::ArbitrumMainnet => 42161,
            Self::ArbitrumGoerli => 421613,
            Self::AvalancheMainnet => 43114,
            Self::AvalancheFuji => 43113,
            Self::OptimismMainnet => 10,
            Self::OptimismGoerli => 420,
            Self::BaseMainnet => 8453,
            Self::BaseGoerli => 84531,
            Self::ZkSyncMainnet => 324,
            Self::ZkSyncTestnet => 280,
            Self::SolanaMainnet => 0, // Non-EVM, không có ID chuẩn
            Self::SolanaDevnet => 0,  // Non-EVM
            Self::SolanaTestnet => 0, // Non-EVM
            Self::NearMainnet => 0,   // Non-EVM
            Self::NearTestnet => 0,   // Non-EVM
            Self::TronMainnet => 728126428,
            Self::TronNile => 3448148188,
            Self::TronShasta => 2494104990,
            Self::DiamondMainnet => 111111,
            Self::DiamondTestnet => 11111,
            Self::CosmosMainnet => 118,
            Self::CosmosTestnet => 119,
            Self::HederaMainnet => 0, // Non-EVM
            Self::HederaTestnet => 0, // Non-EVM
        }
    }

    /// Lấy URL mặc định cho chuỗi này
    pub fn default_rpc_url(&self) -> &'static str {
        match self {
            Self::EthereumMainnet => "https://eth.llamarpc.com",
            Self::EthereumGoerli => "https://goerli.infura.io/v3/9aa3d95b3bc440fa88ea12eaa4456161",
            Self::EthereumSepolia => "https://sepolia.infura.io/v3/9aa3d95b3bc440fa88ea12eaa4456161",
            Self::BscMainnet => "https://bsc-dataseed.binance.org",
            Self::BscTestnet => "https://data-seed-prebsc-1-s1.binance.org:8545",
            Self::PolygonMainnet => "https://polygon-rpc.com",
            Self::PolygonMumbai => "https://rpc-mumbai.maticvigil.com",
            Self::ArbitrumMainnet => "https://arb1.arbitrum.io/rpc",
            Self::ArbitrumGoerli => "https://goerli-rollup.arbitrum.io/rpc",
            Self::AvalancheMainnet => "https://api.avax.network/ext/bc/C/rpc",
            Self::AvalancheFuji => "https://api.avax-test.network/ext/bc/C/rpc",
            Self::OptimismMainnet => "https://mainnet.optimism.io",
            Self::OptimismGoerli => "https://goerli.optimism.io",
            Self::BaseMainnet => "https://mainnet.base.org",
            Self::BaseGoerli => "https://goerli.base.org",
            Self::ZkSyncMainnet => "https://mainnet.era.zksync.io",
            Self::ZkSyncTestnet => "https://testnet.era.zksync.dev",
            Self::SolanaMainnet => "https://api.mainnet-beta.solana.com",
            Self::SolanaDevnet => "https://api.devnet.solana.com",
            Self::SolanaTestnet => "https://api.testnet.solana.com",
            Self::NearMainnet => "https://rpc.mainnet.near.org",
            Self::NearTestnet => "https://rpc.testnet.near.org",
            Self::TronMainnet => "https://api.trongrid.io",
            Self::TronNile => "https://nile.trongrid.io",
            Self::TronShasta => "https://api.shasta.trongrid.io",
            Self::DiamondMainnet => "https://mainnet-rpc.diamondprotocol.co",
            Self::DiamondTestnet => "https://testnet-rpc.diamondprotocol.co",
            Self::CosmosMainnet => "https://cosmos-rpc.polkachu.com",
            Self::CosmosTestnet => "https://cosmos-testnet-rpc.polkachu.com",
            Self::HederaMainnet => "https://mainnet.hedera.com",
            Self::HederaTestnet => "https://testnet.hedera.com",
        }
    }

    /// Lấy explorer URL mặc định cho chuỗi
    pub fn default_explorer_url(&self) -> &'static str {
        match self {
            Self::EthereumMainnet => "https://etherscan.io",
            Self::EthereumGoerli => "https://goerli.etherscan.io",
            Self::EthereumSepolia => "https://sepolia.etherscan.io",
            Self::BscMainnet => "https://bscscan.com",
            Self::BscTestnet => "https://testnet.bscscan.com",
            Self::PolygonMainnet => "https://polygonscan.com",
            Self::PolygonMumbai => "https://mumbai.polygonscan.com",
            Self::ArbitrumMainnet => "https://arbiscan.io",
            Self::ArbitrumGoerli => "https://goerli.arbiscan.io",
            Self::AvalancheMainnet => "https://snowtrace.io",
            Self::AvalancheFuji => "https://testnet.snowtrace.io",
            Self::OptimismMainnet => "https://optimistic.etherscan.io",
            Self::OptimismGoerli => "https://goerli-optimism.etherscan.io",
            Self::BaseMainnet => "https://basescan.org",
            Self::BaseGoerli => "https://goerli.basescan.org",
            Self::ZkSyncMainnet => "https://explorer.zksync.io",
            Self::ZkSyncTestnet => "https://goerli.explorer.zksync.io",
            Self::SolanaMainnet => "https://explorer.solana.com",
            Self::SolanaDevnet => "https://explorer.solana.com/?cluster=devnet",
            Self::SolanaTestnet => "https://explorer.solana.com/?cluster=testnet",
            Self::NearMainnet => "https://explorer.near.org",
            Self::NearTestnet => "https://explorer.testnet.near.org",
            Self::TronMainnet => "https://tronscan.org",
            Self::TronNile => "https://nile.tronscan.org",
            Self::TronShasta => "https://shasta.tronscan.org",
            Self::DiamondMainnet => "https://explorer.diamondprotocol.co",
            Self::DiamondTestnet => "https://testnet-explorer.diamondprotocol.co",
            Self::CosmosMainnet => "https://www.mintscan.io/cosmos",
            Self::CosmosTestnet => "https://testnet.mintscan.io/cosmos-testnet",
            Self::HederaMainnet => "https://hashscan.io/mainnet",
            Self::HederaTestnet => "https://hashscan.io/testnet",
        }
    }

    /// Chuyển đổi từ số sang chain ID
    pub fn from_u64(chain_id: u64) -> Option<Self> {
        let chain = match chain_id {
            1 => Self::EthereumMainnet,
            5 => Self::EthereumGoerli,
            11155111 => Self::EthereumSepolia,
            56 => Self::BscMainnet,
            97 => Self::BscTestnet,
            137 => Self::PolygonMainnet,
            80001 => Self::PolygonMumbai,
            42161 => Self::ArbitrumMainnet,
            421613 => Self::ArbitrumGoerli,
            43114 => Self::AvalancheMainnet,
            43113 => Self::AvalancheFuji,
            10 => Self::OptimismMainnet,
            420 => Self::OptimismGoerli,
            8453 => Self::BaseMainnet,
            84531 => Self::BaseGoerli,
            324 => Self::ZkSyncMainnet,
            280 => Self::ZkSyncTestnet,
            728126428 => Self::TronMainnet,
            3448148188 => Self::TronNile,
            2494104990 => Self::TronShasta,
            111111 => Self::DiamondMainnet,
            11111 => Self::DiamondTestnet,
            118 => Self::CosmosMainnet,
            119 => Self::CosmosTestnet,
            _ => return None,
        };
        Some(chain)
    }
    
    /// Kiểm tra xem chain có phải EVM hay không
    pub fn is_evm(&self) -> bool {
        match self {
            | Self::SolanaMainnet
            | Self::SolanaDevnet
            | Self::SolanaTestnet
            | Self::NearMainnet
            | Self::NearTestnet
            | Self::TronMainnet
            | Self::TronNile
            | Self::TronShasta
            | Self::DiamondMainnet
            | Self::DiamondTestnet
            | Self::CosmosMainnet
            | Self::CosmosTestnet
            | Self::HederaMainnet
            | Self::HederaTestnet => false,
            _ => true,
        }
    }
    
    /// Kiểm tra xem chain có phải testnet hay không
    pub fn is_testnet(&self) -> bool {
        match self {
            | Self::EthereumGoerli
            | Self::EthereumSepolia
            | Self::BscTestnet
            | Self::PolygonMumbai
            | Self::ArbitrumGoerli
            | Self::AvalancheFuji
            | Self::OptimismGoerli
            | Self::BaseGoerli
            | Self::ZkSyncTestnet
            | Self::SolanaDevnet
            | Self::SolanaTestnet
            | Self::NearTestnet
            | Self::TronNile
            | Self::TronShasta
            | Self::DiamondTestnet
            | Self::CosmosTestnet
            | Self::HederaTestnet => true,
            _ => false,
        }
    }
    
    /// Lấy tên ngắn gọn của chain
    pub fn short_name(&self) -> &'static str {
        match self {
            Self::EthereumMainnet => "eth",
            Self::EthereumGoerli => "goerli",
            Self::EthereumSepolia => "sepolia",
            Self::BscMainnet => "bsc",
            Self::BscTestnet => "bsc-testnet",
            Self::PolygonMainnet => "polygon",
            Self::PolygonMumbai => "mumbai",
            Self::ArbitrumMainnet => "arbitrum",
            Self::ArbitrumGoerli => "arb-goerli",
            Self::AvalancheMainnet => "avax",
            Self::AvalancheFuji => "avax-fuji",
            Self::OptimismMainnet => "optimism",
            Self::OptimismGoerli => "op-goerli",
            Self::BaseMainnet => "base",
            Self::BaseGoerli => "base-goerli",
            Self::ZkSyncMainnet => "zksync",
            Self::ZkSyncTestnet => "zksync-testnet",
            Self::SolanaMainnet => "solana",
            Self::SolanaDevnet => "solana-devnet",
            Self::SolanaTestnet => "solana-testnet",
            Self::NearMainnet => "near",
            Self::NearTestnet => "near-testnet",
            Self::TronMainnet => "tron",
            Self::TronNile => "tron-nile",
            Self::TronShasta => "tron-shasta",
            Self::DiamondMainnet => "diamond",
            Self::DiamondTestnet => "diamond-testnet",
            Self::CosmosMainnet => "cosmos",
            Self::CosmosTestnet => "cosmos-testnet",
            Self::HederaMainnet => "hedera",
            Self::HederaTestnet => "hedera-testnet",
        }
    }
    
    /// Parse chain ID từ string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "ethereum" | "eth" | "mainnet" => Some(Self::EthereumMainnet),
            "goerli" => Some(Self::EthereumGoerli),
            "sepolia" => Some(Self::EthereumSepolia),
            "bsc" | "binance" => Some(Self::BscMainnet),
            "bsc-testnet" | "binance-testnet" => Some(Self::BscTestnet),
            "polygon" | "matic" => Some(Self::PolygonMainnet),
            "mumbai" => Some(Self::PolygonMumbai),
            "arbitrum" | "arbitrum-one" => Some(Self::ArbitrumMainnet),
            "arbitrum-goerli" | "arb-goerli" => Some(Self::ArbitrumGoerli),
            "avalanche" | "avax" => Some(Self::AvalancheMainnet),
            "fuji" | "avax-fuji" => Some(Self::AvalancheFuji),
            "optimism" | "op" => Some(Self::OptimismMainnet),
            "optimism-goerli" | "op-goerli" => Some(Self::OptimismGoerli),
            "base" => Some(Self::BaseMainnet),
            "base-goerli" => Some(Self::BaseGoerli),
            "zksync" | "zksync-era" => Some(Self::ZkSyncMainnet),
            "zksync-testnet" | "zksync-goerli" => Some(Self::ZkSyncTestnet),
            "solana" | "sol" => Some(Self::SolanaMainnet),
            "solana-devnet" | "sol-devnet" => Some(Self::SolanaDevnet),
            "solana-testnet" | "sol-testnet" => Some(Self::SolanaTestnet),
            "near" => Some(Self::NearMainnet),
            "near-testnet" => Some(Self::NearTestnet),
            "tron" => Some(Self::TronMainnet),
            "tron-nile" | "nile" => Some(Self::TronNile),
            "tron-shasta" | "shasta" => Some(Self::TronShasta),
            "diamond" => Some(Self::DiamondMainnet),
            "diamond-testnet" => Some(Self::DiamondTestnet),
            "cosmos" | "atom" => Some(Self::CosmosMainnet),
            "cosmos-testnet" | "atom-testnet" => Some(Self::CosmosTestnet),
            "hedera" | "hbar" => Some(Self::HederaMainnet),
            "hedera-testnet" | "hbar-testnet" => Some(Self::HederaTestnet),
            _ => {
                // Thử parse như là chain ID số
                if let Ok(id) = u64::from_str(s) {
                    Self::from_u64(id)
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_id_as_u64() {
        assert_eq!(ChainId::EthereumMainnet.as_u64(), 1);
        assert_eq!(ChainId::BscMainnet.as_u64(), 56);
        assert_eq!(ChainId::DiamondMainnet.as_u64(), 111111);
        assert_eq!(ChainId::DiamondTestnet.as_u64(), 11111);
    }

    #[test]
    fn test_chain_id_from_u64() {
        assert_eq!(ChainId::from_u64(1), Some(ChainId::EthereumMainnet));
        assert_eq!(ChainId::from_u64(56), Some(ChainId::BscMainnet));
        assert_eq!(ChainId::from_u64(111111), Some(ChainId::DiamondMainnet));
        assert_eq!(ChainId::from_u64(11111), Some(ChainId::DiamondTestnet));
        assert_eq!(ChainId::from_u64(999999), None);
    }

    #[test]
    fn test_chain_id_is_evm() {
        assert!(ChainId::EthereumMainnet.is_evm());
        assert!(ChainId::BscMainnet.is_evm());
        assert!(!ChainId::SolanaMainnet.is_evm());
        assert!(!ChainId::TronMainnet.is_evm());
        assert!(!ChainId::DiamondMainnet.is_evm());
    }

    #[test]
    fn test_chain_id_is_testnet() {
        assert!(!ChainId::EthereumMainnet.is_testnet());
        assert!(ChainId::EthereumGoerli.is_testnet());
        assert!(ChainId::DiamondTestnet.is_testnet());
        assert!(!ChainId::DiamondMainnet.is_testnet());
    }

    #[test]
    fn test_chain_id_from_str() {
        assert_eq!(ChainId::from_str("eth"), Some(ChainId::EthereumMainnet));
        assert_eq!(ChainId::from_str("ethereum"), Some(ChainId::EthereumMainnet));
        assert_eq!(ChainId::from_str("1"), Some(ChainId::EthereumMainnet));
        assert_eq!(ChainId::from_str("diamond"), Some(ChainId::DiamondMainnet));
        assert_eq!(ChainId::from_str("diamond-testnet"), Some(ChainId::DiamondTestnet));
        assert_eq!(ChainId::from_str("111111"), Some(ChainId::DiamondMainnet));
        assert_eq!(ChainId::from_str("invalid"), None);
    }
} 