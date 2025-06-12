// use std::sync::Arc;  // Unused import
use anyhow::{Result, anyhow};

/// Service quản lý Threshold MPC
pub struct MpcThresholdService {
    /// Threshold (số lượng tối thiểu các bên tham gia cần thiết)
    threshold: u8,
    
    /// Tổng số bên tham gia
    total_parties: u8,
    
    /// Các bên tham gia hiện tại
    active_parties: Vec<PartyInfo>,
}

/// Thông tin về một bên tham gia
pub struct PartyInfo {
    /// ID của bên tham gia
    pub id: String,
    
    /// Địa chỉ kết nối
    pub endpoint: String,
    
    /// Public key của bên tham gia
    pub public_key: Vec<u8>,
    
    /// Trạng thái
    pub status: PartyStatus,
}

/// Trạng thái của bên tham gia
pub enum PartyStatus {
    /// Online và sẵn sàng
    Ready,
    
    /// Offline hoặc không phản hồi
    Unavailable,
    
    /// Bị loại khỏi hệ thống
    Revoked,
}

/// Thông tin về phiên MPC
pub struct ThresholdSession {
    /// ID phiên
    pub id: String,
    
    /// Các bên tham gia
    pub parties: Vec<String>,
    
    /// Trạng thái phiên
    pub status: SessionStatus,
    
    /// Dữ liệu đang được xử lý
    pub data: Vec<u8>,
}

/// Trạng thái phiên
pub enum SessionStatus {
    /// Đang khởi tạo
    Initializing,
    
    /// Đang xử lý
    InProgress,
    
    /// Hoàn thành
    Completed,
    
    /// Thất bại
    Failed(String),
}

impl MpcThresholdService {
    /// Tạo mới threshold service
    pub fn new(threshold: u8, total_parties: u8) -> Self {
        if threshold > total_parties {
            panic!("Threshold cannot be greater than total parties");
        }
        
        Self {
            threshold,
            total_parties,
            active_parties: Vec::new(),
        }
    }
    
    /// Thêm bên tham gia mới
    pub fn add_party(&mut self, id: &str, endpoint: &str, public_key: Vec<u8>) -> Result<()> {
        // Kiểm tra xem đã có bên tham gia này chưa
        if self.active_parties.iter().any(|p| p.id == id) {
            return Err(anyhow!("Party with ID {} already exists", id));
        }
        
        // Kiểm tra số lượng bên tham gia
        if self.active_parties.len() >= self.total_parties as usize {
            return Err(anyhow!("Maximum number of parties ({}) reached", self.total_parties));
        }
        
        // Thêm bên tham gia mới
        let party = PartyInfo {
            id: id.to_string(),
            endpoint: endpoint.to_string(),
            public_key,
            status: PartyStatus::Ready,
        };
        
        self.active_parties.push(party);
        Ok(())
    }
    
    /// Loại bỏ bên tham gia
    pub fn revoke_party(&mut self, id: &str) -> Result<()> {
        if let Some(party) = self.active_parties.iter_mut().find(|p| p.id == id) {
            party.status = PartyStatus::Revoked;
            Ok(())
        } else {
            Err(anyhow!("Party with ID {} not found", id))
        }
    }
    
    /// Kiểm tra xem có đủ bên tham gia để đạt threshold không
    pub fn has_threshold(&self) -> bool {
        self.active_parties.iter()
            .filter(|p| matches!(p.status, PartyStatus::Ready))
            .count() >= self.threshold as usize
    }
    
    /// Tạo một phiên MPC mới
    pub fn create_session(&self, data: Vec<u8>) -> Result<ThresholdSession> {
        // Kiểm tra xem có đủ bên tham gia không
        if !self.has_threshold() {
            return Err(anyhow!("Not enough active parties to meet threshold"));
        }
        
        // Lấy các bên tham gia sẵn sàng
        let ready_parties: Vec<String> = self.active_parties.iter()
            .filter(|p| matches!(p.status, PartyStatus::Ready))
            .map(|p| p.id.clone())
            .collect();
            
        // Tạo phiên mới
        let session = ThresholdSession {
            id: format!("session_{}", uuid::Uuid::new_v4()),
            parties: ready_parties,
            status: SessionStatus::Initializing,
            data,
        };
        
        Ok(session)
    }
    
    /// Thực hiện MPC
    pub async fn execute_mpc(&self, session: &mut ThresholdSession) -> Result<Vec<u8>> {
        // Kiểm tra trạng thái phiên
        if !matches!(session.status, SessionStatus::Initializing) {
            return Err(anyhow!("Session is not in initializing state"));
        }
        
        // Cập nhật trạng thái
        session.status = SessionStatus::InProgress;
        
        // TODO: Thực hiện MPC thực tế
        // Mô phỏng các bước:
        // 1. Gửi dữ liệu đến các bên tham gia
        // 2. Thu thập các kết quả từ các bên
        // 3. Kết hợp kết quả
        
        // Giả lập kết quả
        let result = vec![0u8; 32]; // Placeholder
        
        // Cập nhật trạng thái
        session.status = SessionStatus::Completed;
        
        Ok(result)
    }
    
    /// Lấy thông tin về các bên tham gia
    pub fn get_parties(&self) -> &[PartyInfo] {
        &self.active_parties
    }
    
    /// Lấy threshold hiện tại
    pub fn get_threshold(&self) -> u8 {
        self.threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_threshold_service() {
        let mut service = MpcThresholdService::new(2, 3);
        
        // Thêm các bên tham gia
        service.add_party("party1", "endpoint1", vec![1, 2, 3]).unwrap();
        service.add_party("party2", "endpoint2", vec![4, 5, 6]).unwrap();
        
        // Kiểm tra threshold
        assert!(service.has_threshold());
        
        // Thêm bên tham gia thứ 3
        service.add_party("party3", "endpoint3", vec![7, 8, 9]).unwrap();
        
        // Loại bỏ một bên
        service.revoke_party("party1").unwrap();
        
        // Vẫn còn đủ để đạt threshold
        assert!(service.has_threshold());
        
        // Loại bỏ bên thứ 2
        service.revoke_party("party2").unwrap();
        
        // Không còn đủ để đạt threshold
        assert!(!service.has_threshold());
    }
} 