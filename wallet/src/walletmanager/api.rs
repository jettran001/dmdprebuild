// wallet/src/walletmanager/api.rs

use ethers::types::Address;

use crate::error::WalletError;
use crate::walletmanager::types::{WalletConfig, SeedLength};
use crate::walletmanager::walletlogic::WalletManager;

impl WalletManager {
    /// Tạo ví mới với seed phrase 12 hoặc 24 từ.
    ///
    /// # Arguments
    /// - `seed_length`: Loại seed phrase (12 hoặc 24 từ).
    /// - `chain_id`: Chain ID mà ví sẽ hoạt động.
    ///
    /// # Returns
    /// Trả về địa chỉ của ví, seed phrase được tạo và ID người dùng.
    ///
    /// # Errors
    /// Trả về lỗi nếu không thể tạo seed phrase hoặc ví đã tồn tại.
    ///
    /// # Flow
    /// Hàm này tạo ví mới và lưu vào `wallet` để sử dụng trong `snipebot`.
    #[flow_from("user_request")]
    pub fn create_wallet(
        &mut self,
        seed_length: SeedLength,
        chain_id: u64,
    ) -> Result<(Address, String, String), WalletError> {
        self.create_wallet_internal(seed_length, chain_id)
    }

    /// Nhập ví từ seed phrase hoặc private key.
    ///
    /// # Arguments
    /// - `config`: Cấu hình ví bao gồm seed phrase/private key, chain ID, và loại seed phrase.
    ///
    /// # Returns
    /// Trả về địa chỉ của ví và ID người dùng nếu nhập thành công.
    ///
    /// # Errors
    /// Trả về lỗi nếu seed/key không hợp lệ hoặc ví đã tồn tại.
    ///
    /// # Flow
    /// Hàm này nhận dữ liệu từ người dùng và lưu vào `wallet` để sử dụng trong `snipebot`.
    #[flow_from("user_input")]
    pub fn import_wallet(&mut self, config: WalletConfig) -> Result<(Address, String), WalletError> {
        self.import_wallet_internal(config)
    }

    /// Xuất seed phrase của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví cần xuất.
    ///
    /// # Returns
    /// Trả về seed phrase dưới dạng chuỗi.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại hoặc không có seed phrase.
    ///
    /// # Flow
    /// Hàm này lấy dữ liệu từ `wallet` để cung cấp cho người dùng hoặc `snipebot`.
    #[flow_from("wallet")]
    pub fn export_seed_phrase(&self, address: Address) -> Result<String, WalletError> {
        self.export_seed_phrase_internal(address)
    }

    /// Xuất private key của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví cần xuất.
    ///
    /// # Returns
    /// Trả về private key dưới dạng chuỗi hex.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại hoặc không có private key trực tiếp.
    ///
    /// # Flow
    /// Hàm này lấy dữ liệu từ `wallet` để cung cấp cho người dùng hoặc `snipebot`.
    #[flow_from("wallet")]
    pub fn export_private_key(&self, address: Address) -> Result<String, WalletError> {
        self.export_private_key_internal(address)
    }

    /// Xóa ví khỏi danh sách quản lý.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví cần xóa.
    ///
    /// # Returns
    /// Trả về `Ok(())` nếu xóa thành công.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    ///
    /// # Flow
    /// Hàm này cập nhật `wallet` để loại bỏ ví trước khi sử dụng trong `snipebot`.
    #[flow_from("user_request")]
    pub fn remove_wallet(&mut self, address: Address) -> Result<(), WalletError> {
        self.remove_wallet_internal(address)
    }

    /// Cập nhật chain ID cho ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví cần cập nhật.
    /// - `new_chain_id`: Chain ID mới.
    ///
    /// # Returns
    /// Trả về `Ok(())` nếu cập nhật thành công.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    ///
    /// # Flow
    /// Hàm này cập nhật `wallet` để sử dụng chain ID mới trong `snipebot`.
    #[flow_from("user_request")]
    pub fn update_chain_id(&mut self, address: Address, new_chain_id: u64) -> Result<(), WalletError> {
        self.update_chain_id_internal(address, new_chain_id)
    }

    /// Lấy chain ID của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví.
    ///
    /// # Returns
    /// Trả về chain ID nếu ví tồn tại.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    ///
    /// # Flow
    /// Hàm này cung cấp chain ID từ `wallet` để sử dụng trong `snipebot`.
    #[flow_from("wallet")]
    pub fn get_chain_id(&self, address: Address) -> Result<u64, WalletError> {
        self.get_chain_id_internal(address)
    }

    /// Kiểm tra xem ví đã được quản lý chưa.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví.
    ///
    /// # Returns
    /// `true` nếu ví tồn tại, `false` nếu không.
    ///
    /// # Flow
    /// Hàm này kiểm tra trạng thái `wallet` để hỗ trợ frontend hoặc automation trong `snipebot`.
    #[flow_from("wallet")]
    pub fn has_wallet(&self, address: Address) -> bool {
        self.has_wallet_internal(address)
    }

    /// Lấy ví theo địa chỉ.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví.
    ///
    /// # Returns
    /// Trả về tham chiếu đến `LocalWallet` nếu tồn tại.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    ///
    /// # Flow
    /// Hàm này cung cấp ví cho `snipebot` để thực hiện giao dịch.
    #[flow_from("wallet")]
    pub fn get_wallet(&self, address: Address) -> Result<&LocalWallet, WalletError> {
        self.get_wallet_internal(address)
    }

    /// Lấy ID người dùng của ví.
    ///
    /// # Arguments
    /// - `address`: Địa chỉ của ví.
    ///
    /// # Returns
    /// Trả về ID người dùng nếu ví tồn tại.
    ///
    /// # Errors
    /// Trả về lỗi nếu ví không tồn tại.
    ///
    /// # Flow
    /// Hàm này cung cấp ID người dùng từ `wallet` để sử dụng trong `snipebot`.
    #[flow_from("wallet")]
    pub fn get_user_id(&self, address: Address) -> Result<String, WalletError> {
        self.get_user_id_internal(address)
    }

    /// Liệt kê tất cả các ví đang quản lý.
    ///
    /// # Returns
    /// Trả về danh sách địa chỉ của các ví.
    pub fn list_wallets(&self) -> Vec<Address> {
        self.list_wallets_internal()
    }
}