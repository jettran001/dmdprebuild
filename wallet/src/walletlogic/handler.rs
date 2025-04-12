// wallet/src/walletlogic/handler.rs

use ethers::signers::LocalWallet;
use ethers::types::Address;
use tracing::{info, warn};

use crate::error::WalletError;
use crate::walletlogic::init::WalletManager;
use crate::walletmanager::types::WalletSecret;

impl WalletManager {
    /// Xuất seed phrase của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    ///
    /// # Returns
    /// Trả về seed phrase.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại hoặc không có seed phrase.
    pub fn export_seed_phrase_internal(&self, address: Address) -> Result<String, WalletError> {
        info!("Exporting seed phrase for wallet: {}", address);

        let info = self
            .wallets
            .get(&address)
            .ok_or(WalletError::WalletNotFound(address))?;

        match &info.secret {
            WalletSecret::Seed(phrase) => Ok(phrase.clone()),
            WalletSecret::PrivateKey(_) => Err(WalletError::InvalidSeedOrKey(format!(
                "Wallet {} does not have a seed phrase",
                address
            ))),
        }
    }

    /// Xuất private key của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    ///
    /// # Returns
    /// Trả về private key (hex string).
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại hoặc không có private key.
    pub fn export_private_key_internal(&self, address: Address) -> Result<String, WalletError> {
        info!("Exporting private key for wallet: {}", address);

        let info = self
            .wallets
            .get(&address)
            .ok_or(WalletError::WalletNotFound(address))?;

        match &info.secret {
            WalletSecret::PrivateKey(key) => Ok(key.clone()),
            WalletSecret::Seed(_) => Err(WalletError::InvalidSeedOrKey(format!(
                "Wallet {} was imported via seed phrase; use export_seed_phrase instead",
                address
            ))),
        }
    }

    /// Xóa ví khỏi danh sách quản lý.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    ///
    /// # Returns
    /// Trả về `Ok(())` nếu xóa thành công.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    pub fn remove_wallet_internal(&mut self, address: Address) -> Result<(), WalletError> {
        if let Some(info) = self.wallets.remove(&address) {
            info!("Wallet {} removed successfully, user_id: {}", address, info.user_id);
            Ok(())
        } else {
            warn!("Attempted to remove non-existent wallet {}", address);
            Err(WalletError::WalletNotFound(address))
        }
    }

    /// Cập nhật chain ID cho ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    /// - `new_chain_id`: Chain ID mới.
    ///
    /// # Returns
    /// Trả về `Ok(())` nếu cập nhật thành công.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    pub fn update_chain_id_internal(
        &mut self,
        address: Address,
        new_chain_id: u64,
    ) -> Result<(), WalletError> {
        info!("Updating chain ID for wallet {} to {}", address, new_chain_id);

        let info = self
            .wallets
            .get_mut(&address)
            .ok_or(WalletError::WalletNotFound(address))?;
        info.chain_id = new_chain_id;

        info!("Chain ID updated successfully for wallet {}", address);
        Ok(())
    }

    /// Lấy chain ID của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    ///
    /// # Returns
    /// Trả về chain ID.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    pub fn get_chain_id_internal(&self, address: Address) -> Result<u64, WalletError> {
        self.wallets
            .get(&address)
            .map(|info| info.chain_id)
            .ok_or(WalletError::WalletNotFound(address))
    }

    /// Kiểm tra ví có được quản lý không.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    ///
    /// # Returns
    /// `true` nếu ví tồn tại, `false` nếu không.
    pub fn has_wallet_internal(&self, address: Address) -> bool {
        self.wallets.contains_key(&address)
    }

    /// Lấy ví theo địa chỉ.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    ///
    /// # Returns
    /// Trả về tham chiếu đến `LocalWallet`.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    pub fn get_wallet_internal(&self, address: Address) -> Result<&LocalWallet, WalletError> {
        Ok(&self
            .wallets
            .get(&address)
            .ok_or(WalletError::WalletNotFound(address))?
            .wallet)
    }

    /// Lấy ID người dùng của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ ví.
    ///
    /// # Returns
    /// Trả về ID người dùng.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    pub fn get_user_id_internal(&self, address: Address) -> Result<String, WalletError> {
        self.wallets
            .get(&address)
            .map(|info| info.user_id.clone())
            .ok_or(WalletError::WalletNotFound(address))
    }

    /// Liệt kê tất cả ví đang quản lý.
    ///
    /// # Returns
    /// Trả về danh sách địa chỉ ví.
    pub fn list_wallets_internal(&self) -> Vec<Address> {
        self.wallets.keys().copied().collect()
    }
}