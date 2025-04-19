/// Định nghĩa trạng thái và tiêu chí đánh giá rủi ro của token.
///
/// Struct này được sử dụng để xác định mức độ an toàn của token dựa trên các tiêu chí như điểm audit,
/// thuế giao dịch, thanh khoản, trạng thái hợp đồng, và các đặc điểm nguy hiểm khác.
///
/// # Flow
/// Dữ liệu từ `blockchain` -> `snipebot` để phân tích token.
#[derive(Debug, Clone)]
pub struct TokenStatus {
    /// Điểm audit, giá trị từ 0 đến 100.
    pub audit_score: f64,
    /// Thuế mua, tính theo phần trăm.
    pub tax_buy: f64,
    /// Thuế bán, tính theo phần trăm.
    pub tax_sell: f64,
    /// Giá trị thanh khoản của pool, tính bằng USD.
    pub liquidity_pool: f64,
    /// Smart contract đã được verified hay chưa.
    pub contract_verified: bool,
    /// Thanh khoản đã được khóa hay chưa.
    pub pool_locked: bool,
    /// Có mint vô hạn hay không.
    pub mint_infinite: bool,
    /// Có lock sell (không thể bán) hay không.
    pub lock_sell: bool,
    /// Có phải honeypot (mua được nhưng không bán được) hay không.
    pub honeypot: bool,
    /// Có rebate giả mạo (trả lại token giả) hay không.
    pub rebate: bool,
    /// Có hàm pause() cho phép tạm dừng giao dịch hay không.
    pub pausable: bool,
    /// Có hàm blacklistAddress() để chặn ví người dùng hay không.
    pub blacklist: bool,
    /// Chủ sở hữu ẩn danh hoặc contract proxy không rõ ràng.
    pub hidden_ownership: bool,
    /// Tỷ lệ sở hữu cao (>50% token nằm trong 1-2 ví).
    pub high_ownership_concentration: bool,
    /// Giả mạo từ bỏ quyền sở hữu nhưng vẫn giữ quyền qua backdoor.
    pub fake_renounced_ownership: bool,
    /// Thanh khoản thấp (< $1,000).
    pub low_liquidity: bool,
    /// Hàm transfer() tùy chỉnh đáng ngờ (có thể đánh cắp token).
    pub suspicious_transfer_functions: bool,
}

/// Các mức độ trạng thái của token.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenSafety {
    /// Nguy hiểm (🔴): Token có rủi ro cao, nên tránh.
    Dangerous,
    /// Trung bình (🟡): Token chấp nhận được nhưng cần thận trọng.
    Moderate,
    /// Tốt (🟢): Token an toàn, đáng tin cậy.
    Safe,
}

impl TokenStatus {
    /// Kiểm tra trạng thái an toàn của token dựa trên các tiêu chí.
    ///
    /// # Tiêu chí
    /// - **Nguy hiểm (🔴)**: Nếu bất kỳ điều kiện nào sau đây đúng:
    ///   - Điểm audit < 60/100.
    ///   - Có mint vô hạn.
    ///   - Có lock sell (không thể bán).
    ///   - Là honeypot (mua được nhưng không bán được).
    ///   - Có rebate giả mạo.
    ///   - Thuế mua hoặc bán > 10%.
    ///   - Smart contract chưa verified.
    ///   - Có hàm pause() (dấu hiệu scam như rug pull).
    ///   - Có hàm blacklistAddress() (thao túng người dùng).
    ///   - Chủ sở hữu ẩn danh.
    ///   - Tỷ lệ sở hữu tập trung cao (>50% trong 1-2 ví).
    ///   - Giả mạo từ bỏ quyền sở hữu.
    ///   - Thanh khoản thấp (< $1,000).
    ///   - Hàm transfer() đáng ngờ.
    /// - **Trung bình (🟡)**: Nếu tất cả các điều kiện sau đúng:
    ///   - Điểm audit ≥ 60/100.
    ///   - Thuế mua và bán < 5%.
    ///   - Không có hàm nguy hiểm (pause, blacklist, suspicious transfer).
    ///   - Smart contract đã verified.
    ///   - Thanh khoản pool > $2,000.
    /// - **Tốt (🟢)**: Nếu tất cả các điều kiện sau đúng:
    ///   - Điểm audit ≥ 80/100.
    ///   - Thanh khoản đã khóa (pool_locked).
    ///   - Smart contract đã verified.
    ///   - Thanh khoản pool > $5,000.
    ///   - Không có hàm nguy hiểm (pause, blacklist, suspicious transfer).
    ///
    /// # Returns
    /// Trả về trạng thái `TokenSafety` tương ứng.
    ///
    /// # Flow
    /// Hàm này nhận dữ liệu từ `blockchain` và được gọi trong `snipebot` để đánh giá token.
    #[flow_from("blockchain")]
    pub fn evaluate_safety(&self) -> TokenSafety {
        // Kiểm tra trạng thái nguy hiểm
        if self.audit_score < 60.0
            || self.mint_infinite
            || self.lock_sell
            || self.honeypot
            || self.rebate
            || self.tax_buy > 10.0
            || self.tax_sell > 10.0
            || !self.contract_verified
            || self.pausable
            || self.blacklist
            || self.hidden_ownership
            || self.high_ownership_concentration
            || self.fake_renounced_ownership
            || self.low_liquidity
            || self.suspicious_transfer_functions
        {
            return TokenSafety::Dangerous;
        }

        // Kiểm tra trạng thái tốt
        if self.audit_score >= 80.0
            && self.pool_locked
            && self.contract_verified
            && self.liquidity_pool > 5000.0
            && !self.pausable
            && !self.blacklist
            && !self.suspicious_transfer_functions
        {
            return TokenSafety::Safe;
        }

        // Kiểm tra trạng thái trung bình
        if self.audit_score >= 60.0
            && self.tax_buy < 5.0
            && self.tax_sell < 5.0
            && !self.pausable
            && !self.blacklist
            && !self.suspicious_transfer_functions
            && self.contract_verified
            && self.liquidity_pool > 2000.0
        {
            return TokenSafety::Moderate;
        }

        // Mặc định là nguy hiểm nếu không thỏa mãn các điều kiện trên
        TokenSafety::Dangerous
    }
}