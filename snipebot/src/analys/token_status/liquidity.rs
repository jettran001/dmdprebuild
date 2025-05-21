//! Phân tích thanh khoản, rugpull, abnormal events
//!
//! Module này chứa các hàm phân tích liên quan đến:
//! - Phân tích sự kiện thanh khoản bất thường
//! - Phát hiện dấu hiệu rugpull
//! - Kiểm tra khóa thanh khoản

use super::types::{ContractInfo, LiquidityEvent, LiquidityEventType};

/// Phân tích các sự kiện thanh khoản bất thường
pub fn abnormal_liquidity_events(events: &[LiquidityEvent]) -> bool {
    // Không có sự kiện -> không bất thường
    if events.is_empty() {
        return false;
    }
    
    // Đếm sự kiện thêm/xóa
    let mut add_count = 0;
    let mut remove_count = 0;
    let mut total_added = 0.0;
    let mut total_removed = 0.0;
    
    // Tính tổng giá trị thêm/xóa LP và thời gian trung bình giữa các sự kiện
    for event in events {
        match event.event_type {
            LiquidityEventType::AddLiquidity => {
                add_count += 1;
                if let Ok(amount) = event.base_amount.parse::<f64>() {
                    total_added += amount;
                }
            },
            LiquidityEventType::RemoveLiquidity => {
                remove_count += 1;
                if let Ok(amount) = event.base_amount.parse::<f64>() {
                    total_removed += amount;
                }
            }
        }
    }
    
    // Nếu không thêm LP nhưng có xóa LP -> bất thường
    if add_count == 0 && remove_count > 0 {
        return true;
    }
    
    // Tỷ lệ xóa LP quá cao so với thêm LP (> 70%) -> bất thường
    if total_added > 0.0 {
        let remove_ratio = total_removed / total_added;
        if remove_ratio > 0.7 {
            return true;
        }
    }
    
    // Phát hiện rút LP nhanh
    let mut rapid_remove = false;
    
    // Nếu có > 2 sự kiện
    if events.len() >= 3 {
        // Sắp xếp theo thời gian
        let mut sorted_events = events.to_vec();
        sorted_events.sort_by_key(|e| e.timestamp);
        
        // Tìm chuỗi rút LP nhanh
        let mut consecutive_removes = 0;
        let mut last_time = 0;
        
        for event in &sorted_events {
            if event.event_type == LiquidityEventType::RemoveLiquidity {
                consecutive_removes += 1;
                // Nếu là lần đầu hoặc rút nhanh (trong vòng 1 giờ = 3600s)
                if last_time == 0 || event.timestamp - last_time < 3600 {
                    last_time = event.timestamp;
                    // Nếu có ít nhất 3 lần rút LP nhanh liên tiếp
                    if consecutive_removes >= 3 {
                        rapid_remove = true;
                    }
                } else {
                    consecutive_removes = 1;
                    last_time = event.timestamp;
                }
            } else {
                consecutive_removes = 0;
            }
        }
    }
    
    // Kiểm tra rút LP ngay sau khi thêm
    let mut quick_dump = false;
    
    // Phát hiện thêm LP và rút ngay sau đó (trong vòng 10 phút = 600s)
    if events.len() >= 2 {
        let mut sorted_events = events.to_vec();
        sorted_events.sort_by_key(|e| e.timestamp);
        
        for i in 0..sorted_events.len() - 1 {
            if sorted_events[i].event_type == LiquidityEventType::AddLiquidity && 
               sorted_events[i+1].event_type == LiquidityEventType::RemoveLiquidity {
                // Thời gian giữa thêm và rút
                let time_diff = sorted_events[i+1].timestamp - sorted_events[i].timestamp;
                // Nếu rút trong vòng 10 phút sau khi thêm
                if time_diff < 600 {
                    quick_dump = true;
                    break;
                }
            }
        }
    }
    
    // Trả về true nếu có bất kỳ dấu hiệu bất thường nào
    rapid_remove || quick_dump
}

/// Phát hiện dấu hiệu rugpull từ sự kiện thanh khoản
pub fn detect_rugpull_pattern(events: &[LiquidityEvent]) -> bool {
    // Kiểm tra bất thường
    if abnormal_liquidity_events(events) {
        return true;
    }
    
    // Thêm các logic phát hiện rugpull khác
    let mut total_added = 0.0;
    let mut total_removed = 0.0;
    let mut remove_events_count = 0;
    
    for event in events {
        match event.event_type {
            LiquidityEventType::AddLiquidity => {
                if let Ok(amount) = event.base_amount.parse::<f64>() {
                    total_added += amount;
                }
            },
            LiquidityEventType::RemoveLiquidity => {
                remove_events_count += 1;
                if let Ok(amount) = event.base_amount.parse::<f64>() {
                    total_removed += amount;
                }
            }
        }
    }
    
    // Nếu tổng gửi > 0 và đã rút quá 80%
    if total_added > 0.0 && (total_removed / total_added) > 0.8 {
        return true;
    }
    
    // Nếu có nhiều sự kiện rút LP (>= 3) trong thời gian ngắn
    if remove_events_count >= 3 && events.len() >= 5 {
        return true;
    }
    
    // Kiểm tra rút LP bằng ví khác với thêm LP
    let mut add_addresses = std::collections::HashSet::new();
    let mut remove_addresses = std::collections::HashSet::new();
    
    for event in events {
        match event.event_type {
            LiquidityEventType::AddLiquidity => {
                add_addresses.insert(&event.executor);
            },
            LiquidityEventType::RemoveLiquidity => {
                remove_addresses.insert(&event.executor);
            }
        }
    }
    
    // Nếu có địa chỉ rút LP mà không thêm LP
    for addr in &remove_addresses {
        if !add_addresses.contains(addr) {
            return true;
        }
    }
    
    false
}

/// Phân tích lịch sử thanh khoản chi tiết
pub fn analyze_liquidity_history(events: &[LiquidityEvent]) -> (bool, f64, f64, u64, u64) {
    let mut is_suspicious = false;
    let mut total_added = 0.0;
    let mut total_removed = 0.0;
    let mut first_timestamp = u64::MAX;
    let mut last_timestamp = 0;
    
    for event in events {
        // Cập nhật timestamps
        first_timestamp = first_timestamp.min(event.timestamp);
        last_timestamp = last_timestamp.max(event.timestamp);
        
        match event.event_type {
            LiquidityEventType::AddLiquidity => {
                if let Ok(amount) = event.base_amount.parse::<f64>() {
                    total_added += amount;
                }
            },
            LiquidityEventType::RemoveLiquidity => {
                if let Ok(amount) = event.base_amount.parse::<f64>() {
                    total_removed += amount;
                }
            }
        }
    }
    
    // Tính thời gian tồn tại (giây)
    let mut lifetime = 0;
    if first_timestamp != u64::MAX && last_timestamp > 0 {
        lifetime = last_timestamp - first_timestamp;
    }
    
    // Dấu hiệu đáng ngờ:
    // 1. Rút > 50% thanh khoản
    // 2. Tồn tại < 7 ngày (604800 giây)
    if (total_added > 0.0 && total_removed / total_added > 0.5) || 
       (lifetime > 0 && lifetime < 604800) {
        is_suspicious = true;
    }
    
    (is_suspicious, total_added, total_removed, first_timestamp, last_timestamp)
}

/// Kiểm tra nếu LP đã được khóa
pub fn is_liquidity_locked(contract_info: &ContractInfo) -> bool {
    if let Some(ref source_code) = contract_info.source_code {
        // Kiểm tra các pattern LP lock
        let lock_patterns = [
            "liquidity.*lock",
            "lockLiquidity",
            "lockLP",
            "lpLock",
            "lock.*liquidity",
            "unicrypt",
            "liquidityLock",
            "LiquidityLocker",
            "lockedUntil",
            "TeamFinance",
            "lockTime",
            "liquidityPoolLock",
        ];
        
        for pattern in &lock_patterns {
            if source_code.contains(pattern) {
                return true;
            }
        }
    }
    
    false
} 