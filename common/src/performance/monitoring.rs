use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use metrics::{counter, gauge, histogram};

/// Service theo dõi hiệu năng của hệ thống
pub struct MonitoringService {
    /// Các metric hiện tại
    metrics: Arc<RwLock<HashMap<String, MetricValue>>>,
    
    /// Thời điểm bắt đầu
    start_time: Instant,
    
    /// Đã được khởi tạo
    initialized: bool,
}

/// Giá trị metric
pub enum MetricValue {
    /// Counter (giá trị tăng dần)
    Counter(u64),
    
    /// Gauge (giá trị có thể tăng/giảm)
    Gauge(f64),
    
    /// Histogram (phân phối các giá trị)
    Histogram(Vec<f64>),
}

/// Loại metric
pub enum MetricType {
    /// Counter
    Counter,
    
    /// Gauge
    Gauge,
    
    /// Histogram
    Histogram,
}

/// Snapshot của các metric
pub struct MetricsSnapshot {
    /// Thời điểm chụp
    pub timestamp: Instant,
    
    /// Các metric
    pub metrics: HashMap<String, MetricValue>,
    
    /// Thời gian uptime (giây)
    pub uptime_seconds: u64,
}

impl MonitoringService {
    /// Tạo mới monitoring service
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(HashMap::new())),
            start_time: Instant::now(),
            initialized: false,
        }
    }
    
    /// Khởi tạo monitoring service
    pub async fn initialize(&mut self) -> Result<(), String> {
        if self.initialized {
            return Ok(());
        }
        
        // Đăng ký các metric cơ bản
        {
            let mut metrics = self.metrics.write().await;
            metrics.insert("system.uptime".to_string(), MetricValue::Gauge(0.0));
            metrics.insert("system.memory.usage".to_string(), MetricValue::Gauge(0.0));
            metrics.insert("system.cpu.usage".to_string(), MetricValue::Gauge(0.0));
            metrics.insert("system.request.total".to_string(), MetricValue::Counter(0));
            metrics.insert("system.request.error".to_string(), MetricValue::Counter(0));
            metrics.insert("system.request.latency".to_string(), MetricValue::Histogram(Vec::new()));
        }
        
        // Khởi tạo metrics backend (sử dụng thư viện metrics)
        self.initialize_metrics_backend();
        
        self.initialized = true;
        Ok(())
    }
    
    /// Khởi tạo metrics backend
    fn initialize_metrics_backend(&self) {
        // TODO: Triển khai metrics backend thực tế
        // Ví dụ, có thể sử dụng metrics-exporter-prometheus để export metrics
        
        // Đăng ký các metric cơ bản với thư viện metrics
        gauge!("system.uptime", 0.0);
        gauge!("system.memory.usage", 0.0);
        gauge!("system.cpu.usage", 0.0);
        counter!("system.request.total", 0);
        counter!("system.request.error", 0);
    }
    
    /// Cập nhật giá trị counter
    pub async fn increment_counter(&self, name: &str, value: u64) {
        let mut metrics = self.metrics.write().await;
        
        match metrics.get_mut(name) {
            Some(MetricValue::Counter(current)) => {
                *current += value;
            },
            _ => {
                metrics.insert(name.to_string(), MetricValue::Counter(value));
            }
        }
        
        // Cập nhật metrics backend - tạo bản sao của name
        let name_owned = name.to_string();
        counter!(name_owned, value);
    }
    
    /// Cập nhật giá trị gauge
    pub async fn update_gauge(&self, name: &str, value: f64) {
        let mut metrics = self.metrics.write().await;
        
        metrics.insert(name.to_string(), MetricValue::Gauge(value));
        
        // Cập nhật metrics backend - tạo bản sao của name
        let name_owned = name.to_string();
        gauge!(name_owned, value);
    }
    
    /// Ghi giá trị vào histogram
    pub async fn record_histogram(&self, name: &str, value: f64) {
        let mut metrics = self.metrics.write().await;
        
        match metrics.get_mut(name) {
            Some(MetricValue::Histogram(values)) => {
                values.push(value);
                // Giữ histogram trong giới hạn hợp lý
                if values.len() > 1000 {
                    values.remove(0);
                }
            },
            _ => {
                metrics.insert(name.to_string(), MetricValue::Histogram(vec![value]));
            }
        }
        
        // Cập nhật metrics backend - tạo bản sao của name
        let name_owned = name.to_string();
        histogram!(name_owned, value);
    }
    
    /// Theo dõi thời gian thực hiện của function
    pub async fn record_execution_time(&self, operation: &str, duration: Duration) {
        let duration_ms = duration.as_secs_f64() * 1000.0;
        let metric_name = format!("{}.latency", operation);
        
        self.record_histogram(&metric_name, duration_ms).await;
    }
    
    /// Ghi nhận request
    pub async fn record_request(&self, success: bool, latency: Duration) {
        // Tăng tổng số request
        self.increment_counter("system.request.total", 1).await;
        
        // Nếu lỗi, tăng counter lỗi
        if !success {
            self.increment_counter("system.request.error", 1).await;
        }
        
        // Ghi nhận latency
        let latency_ms = latency.as_secs_f64() * 1000.0;
        self.record_histogram("system.request.latency", latency_ms).await;
    }
    
    /// Lấy snapshot của tất cả metrics
    pub async fn get_snapshot(&self) -> MetricsSnapshot {
        // Cập nhật uptime
        {
            let uptime = self.start_time.elapsed().as_secs_f64();
            let mut metrics = self.metrics.write().await;
            
            if let Some(MetricValue::Gauge(current)) = metrics.get_mut("system.uptime") {
                *current = uptime;
            }
        }
        
        // Lấy bản sao của tất cả metrics
        let metrics = {
            let metrics_lock = self.metrics.read().await;
            metrics_lock.clone()
        };
        
        MetricsSnapshot {
            timestamp: Instant::now(),
            metrics,
            uptime_seconds: self.start_time.elapsed().as_secs(),
        }
    }
    
    /// Làm sạch dữ liệu metrics cũ
    pub async fn cleanup_old_data(&self) {
        let mut metrics = self.metrics.write().await;
        
        // Xử lý histogram, giới hạn kích thước
        for (_, value) in metrics.iter_mut() {
            if let MetricValue::Histogram(values) = value {
                if values.len() > 1000 {
                    let new_values = values.iter()
                        .skip(values.len() - 1000)
                        .cloned()
                        .collect();
                    *values = new_values;
                }
            }
        }
    }
}

// Clone for MetricValue
impl Clone for MetricValue {
    fn clone(&self) -> Self {
        match self {
            Self::Counter(val) => Self::Counter(*val),
            Self::Gauge(val) => Self::Gauge(*val),
            Self::Histogram(vals) => Self::Histogram(vals.clone()),
        }
    }
}

impl Default for MonitoringService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_monitoring_service() {
        let mut service = MonitoringService::new();
        service.initialize().await.unwrap();
        
        // Test counter
        service.increment_counter("test.counter", 5).await;
        service.increment_counter("test.counter", 3).await;
        
        // Test gauge
        service.update_gauge("test.gauge", 42.5).await;
        
        // Test histogram
        service.record_histogram("test.histogram", 10.0).await;
        service.record_histogram("test.histogram", 20.0).await;
        service.record_histogram("test.histogram", 30.0).await;
        
        // Get snapshot
        let snapshot = service.get_snapshot().await;
        
        // Verify counter
        if let Some(MetricValue::Counter(value)) = snapshot.metrics.get("test.counter") {
            assert_eq!(*value, 8);
        } else {
            panic!("Counter not found or wrong type");
        }
        
        // Verify gauge
        if let Some(MetricValue::Gauge(value)) = snapshot.metrics.get("test.gauge") {
            assert_eq!(*value, 42.5);
        } else {
            panic!("Gauge not found or wrong type");
        }
        
        // Verify histogram
        if let Some(MetricValue::Histogram(values)) = snapshot.metrics.get("test.histogram") {
            assert_eq!(values.len(), 3);
            assert_eq!(values[0], 10.0);
            assert_eq!(values[1], 20.0);
            assert_eq!(values[2], 30.0);
        } else {
            panic!("Histogram not found or wrong type");
        }
    }
} 