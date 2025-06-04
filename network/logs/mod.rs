//! # Network Logs Module
//!
//! Module này cung cấp các hàm ghi log lỗi chuyên biệt cho từng domain: network, wallet, snipebot, common.
//! Mỗi loại log sẽ được ghi ra file riêng biệt để dễ quản lý và truy xuất.
//!
//! Features:
//! - Cấu hình đường dẫn log thông qua environment variables
//! - Hỗ trợ non-blocking IO thông qua tokio
//! - Xử lý lỗi khi ghi log
//! - Rotation logs theo kích thước

use std::fs::{OpenOptions, File};
use std::io::{Write, Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::env;
use std::sync::Once;
use chrono::Local;
use tokio::task;
use tracing::{error, warn, info};
use std::sync::atomic::{AtomicBool, Ordering};
use lazy_static::lazy_static;

lazy_static! {
    // Cờ kiểm soát trạng thái khởi tạo
    static ref LOGS_INITIALIZED: AtomicBool = AtomicBool::new(false);
    
    // Đường dẫn gốc cho logs, cấu hình từ env hoặc mặc định
    static ref LOG_BASE_PATH: String = env::var("NETWORK_LOG_PATH")
        .unwrap_or_else(|_| "logs".to_string());
}

// Kích thước tối đa cho mỗi file log (5MB)
const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024;

// Hàm khởi tạo thư mục logs nếu chưa tồn tại
fn ensure_log_directory() -> Result<(), IoError> {
    let log_dir = PathBuf::from(LOG_BASE_PATH.as_str());
    if !log_dir.exists() {
        std::fs::create_dir_all(&log_dir)?;
    }
    Ok(())
}

// Hàm kiểm tra và rotate log file nếu quá kích thước
fn check_and_rotate(log_path: &Path) -> Result<(), IoError> {
    if log_path.exists() {
        let metadata = std::fs::metadata(log_path)?;
        if metadata.len() > MAX_LOG_SIZE {
            let mut rotation_path = log_path.to_path_buf();
            let file_stem = log_path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("log");
            let extension = log_path.extension()
                .and_then(|s| s.to_str())
                .unwrap_or("log");
            let timestamp = Local::now().format("%Y%m%d_%H%M%S");
            let rotated_name = format!("{}.{}.{}", file_stem, timestamp, extension);
            rotation_path.set_file_name(rotated_name);
            
            // Rename file (rotate)
            std::fs::rename(log_path, &rotation_path)?;
            info!("Rotated log file to: {:?}", rotation_path);
        }
    }
    Ok(())
}

/// Ghi log lỗi cho network - non-blocking với xử lý lỗi
pub fn log_network_error(msg: &str) {
    let domain_log_path = format!("{}/network_error.log", LOG_BASE_PATH.as_str());
    let message = msg.to_string();
    
    task::spawn(async move {
        if let Err(e) = log_to_file_internal(&domain_log_path, &message).await {
            eprintln!("Failed to write network error log: {}", e);
        }
    });
}

/// Ghi log lỗi cho wallet - non-blocking với xử lý lỗi
pub fn log_wallet_error(msg: &str) {
    let domain_log_path = format!("{}/wallet_error.log", LOG_BASE_PATH.as_str());
    let message = msg.to_string();
    
    task::spawn(async move {
        if let Err(e) = log_to_file_internal(&domain_log_path, &message).await {
            eprintln!("Failed to write wallet error log: {}", e);
        }
    });
}

/// Ghi log lỗi cho snipebot - non-blocking với xử lý lỗi
pub fn log_snipebot_error(msg: &str) {
    let domain_log_path = format!("{}/snipebot_error.log", LOG_BASE_PATH.as_str());
    let message = msg.to_string();
    
    task::spawn(async move {
        if let Err(e) = log_to_file_internal(&domain_log_path, &message).await {
            eprintln!("Failed to write snipebot error log: {}", e);
        }
    });
}

/// Ghi log lỗi cho common/frontend/gateway - non-blocking với xử lý lỗi
pub fn log_common_error(msg: &str) {
    let domain_log_path = format!("{}/common_error.log", LOG_BASE_PATH.as_str());
    let message = msg.to_string();
    
    task::spawn(async move {
        if let Err(e) = log_to_file_internal(&domain_log_path, &message).await {
            eprintln!("Failed to write common error log: {}", e);
        }
    });
}

/// Hàm nội bộ để ghi log ra file với timestamp, xử lý lỗi, và rotation
async fn log_to_file_internal(path_str: &str, msg: &str) -> Result<(), IoError> {
    // Đảm bảo thư mục log tồn tại
    if !LOGS_INITIALIZED.load(Ordering::SeqCst) {
        if let Err(e) = ensure_log_directory() {
            return Err(IoError::new(
                ErrorKind::Other,
                format!("Failed to create log directory: {}", e)
            ));
        }
        LOGS_INITIALIZED.store(true, Ordering::SeqCst);
    }
    
    let log_path = Path::new(path_str);
    
    // Kiểm tra và rotate log nếu cần
    if let Err(e) = check_and_rotate(log_path) {
        warn!("Failed to rotate log file {}: {}", path_str, e);
        // Vẫn tiếp tục ghi log ngay cả khi rotate thất bại
    }
    
    // Format message với timestamp
    let now = Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{}] {}\n", now, msg);
    
    // Mở file và ghi log
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    
    // Ghi nội dung vào file
    file.write_all(line.as_bytes())?;
    
    Ok(())
} 