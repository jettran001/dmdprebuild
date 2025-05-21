# DePIN Node Deployment Guide: Log Forwarding & Centralized Monitoring

## 1. Mục tiêu
Hướng dẫn dev/ops cấu hình forwarding log từ các node DePIN về server chỉ định để tập trung giám sát, cảnh báo và audit.

---

## 2. Các phương pháp forwarding log phổ biến

### 2.1. Sử dụng log forwarding agent (khuyến nghị)
- **Cài đặt agent**: Filebeat, Fluentd, Vector, Promtail...
- **Cấu hình agent đọc file log** (ví dụ: `network/logs/network_error.log`)
- **Forward về server log trung tâm** (Elasticsearch, Loki, Graylog, Splunk...)

**Ví dụ với Filebeat (Elasticsearch):**
```bash
# 1. Cài đặt Filebeat
sudo apt-get install filebeat

# 2. Cấu hình filebeat.yml
filebeat.inputs:
- type: log
  enabled: true
  paths:
    - /path/to/network/logs/*.log
output.elasticsearch:
  hosts: ["http://log-server.example.com:9200"]

# 3. Khởi động Filebeat
sudo systemctl start filebeat
```

**Ví dụ với Promtail (Loki):**
```yaml
# promtail-config.yaml
server:
  http_listen_port: 9080
positions:
  filename: /tmp/positions.yaml
clients:
  - url: http://loki-server.example.com:3100/loki/api/v1/push
scrape_configs:
  - job_name: network-logs
    static_configs:
      - targets: [localhost]
        labels:
          job: network
          __path__: /path/to/network/logs/*.log
```
```bash
# Chạy promtail
promtail --config.file=promtail-config.yaml
```

**Ví dụ với Filebeat (Graylog):**
```yaml
# filebeat.yml
filebeat.inputs:
- type: log
  enabled: true
  paths:
    - /path/to/network/logs/*.log
output.logstash:
  hosts: ["graylog-server.example.com:5044"]
```

**Ví dụ với Splunk Universal Forwarder:**
```bash
# 1. Cài đặt Splunk Universal Forwarder
wget -O splunkforwarder.tgz 'https://download.splunk.com/products/universalforwarder/releases/9.0.0/linux/splunkforwarder-9.0.0-Linux-x86_64.tgz'
tar -xvf splunkforwarder.tgz -C /opt
cd /opt/splunkforwarder
./bin/splunk start --accept-license

# 2. Thêm đường dẫn log
./bin/splunk add monitor /path/to/network/logs/*.log

# 3. Cấu hình forwarding tới Splunk indexer
./bin/splunk add forward-server splunk-indexer.example.com:9997

# 4. Khởi động lại forwarder
./bin/splunk restart
```

### 2.2. Forward log qua API log collector
- **Cấu hình node gửi log qua HTTP POST** tới server log collector.
- **Ví dụ:**
```bash
curl -X POST http://log-server.example.com/api/logs \
     -H "Content-Type: text/plain" \
     --data-binary @network/logs/network_error.log
```
- Có thể tích hợp vào script cron hoặc agent tự động.

### 2.3. Sử dụng syslog hoặc remote logging
- **Cấu hình syslog forwarding** tới server log tập trung.
- **Ví dụ:**
```bash
# Thêm vào /etc/rsyslog.conf
*.* @log-server.example.com:514
# Khởi động lại rsyslog
sudo systemctl restart rsyslog
```

### 2.4. Prometheus/Grafana Alert (nếu log chuyển thành metric)
- **Chuyển log thành metric** (ví dụ: số lượng lỗi theo thời gian)
- **Expose endpoint /metrics** trong code (dùng crate `metrics`, `prometheus`)
- **Prometheus scrape endpoint /metrics**
- **Cấu hình alert trong Grafana**

**Ví dụ tích hợp Prometheus/Grafana:**

1. **Expose metrics trong code Rust:**
```rust
use metrics_exporter_prometheus::PrometheusBuilder;
use metrics::counter;

fn main() {
    PrometheusBuilder::new().install().expect("failed to install Prometheus recorder");
    // Khi có lỗi:
    counter!("network_error_count", 1);
}
```

2. **Cấu hình Prometheus scrape:**
```yaml
scrape_configs:
  - job_name: 'network'
    static_configs:
      - targets: ['node1.example.com:9100'] # Port metrics
```

3. **Tạo alert rule trong Prometheus:**
```yaml
- alert: TooManyNetworkErrors
  expr: network_error_count > 10
  for: 5m
  labels:
    severity: warning
  annotations:
    summary: "Node có nhiều lỗi network (>10 trong 5 phút)"
```

4. **Tạo dashboard và alert trong Grafana:**
- Thêm Prometheus datasource
- Tạo panel với query: `network_error_count`
- Cấu hình alert gửi về Slack, Email, Telegram...

---

## 3. Lưu ý bảo mật khi forwarding log
- **Luôn mã hóa kết nối** (TLS/SSL) giữa node và server log
- **Hạn chế quyền truy cập file log** (chỉ cho phép user/agent cần thiết)
- **Không gửi log chứa thông tin nhạy cảm ra ngoài nếu chưa kiểm duyệt**
- **Sử dụng xác thực (API key, JWT, IP whitelist) cho API log collector**

## 4. Độ tin cậy & tối ưu hiệu suất
- **Cấu hình retry khi gửi log thất bại**
- **Lưu log tạm thời trên node nếu server log bị gián đoạn**
- **Nén log hoặc gửi theo lô (batch) để giảm tải mạng**
- **Giới hạn dung lượng log gửi về để tránh tràn disk/server**

---

## 5. Tổng kết
- Mọi node DePIN nên forwarding log về server chỉ định để dev/ops dễ giám sát, cảnh báo và xử lý tập trung.
- Có thể chọn giải pháp phù hợp (agent, API, syslog, Prometheus, Loki, Graylog, Splunk) tùy hạ tầng.
- Đọc kỹ cảnh báo bảo mật trước khi expose log ra ngoài.

Nếu cần ví dụ cấu hình chi tiết cho từng agent hoặc hệ thống log cụ thể, liên hệ team DevOps để được hỗ trợ thêm.

---

## 6. Hướng dẫn tích hợp alert nâng cao

### 6.1. Alert nâng cao với Prometheus/Alertmanager
- **Alert rule nhiều điều kiện/phân cấp severity:**
```yaml
- alert: NetworkErrorCritical
  expr: network_error_count > 100
  for: 1m
  labels:
    severity: critical
  annotations:
    summary: "Node gặp lỗi nghiêm trọng (>100 lỗi trong 1 phút)"
    description: "Kiểm tra ngay node {{ $labels.instance }}."

- alert: NetworkErrorWarning
  expr: network_error_count > 10
  for: 5m
  labels:
    severity: warning
  annotations:
    summary: "Node có nhiều lỗi network (>10 trong 5 phút)"
```

- **Alert multi-condition:**
```yaml
- alert: HighErrorAndLowMemory
  expr: (network_error_count > 10) and (node_memory_available_bytes < 500000000)
  for: 2m
  labels:
    severity: critical
  annotations:
    summary: "Node vừa lỗi nhiều vừa thiếu RAM"
```

- **Alert silence & escalation:**
  - Dùng Alertmanager web UI để tạm tắt (silence) alert khi bảo trì.
  - Cấu hình escalation: nếu alert critical không được acknowledge sau 10 phút, escalate lên channel khác.

- **Alert routing trong Alertmanager:**
```yaml
route:
  receiver: 'slack-notifications'
  routes:
    - match:
        severity: critical
      receiver: 'telegram-critical'
    - match:
        severity: warning
      receiver: 'email-warning'
receivers:
  - name: 'slack-notifications'
    slack_configs:
      - channel: '#network-alerts'
        send_resolved: true
  - name: 'telegram-critical'
    telegram_configs:
      - bot_token: 'YOUR_BOT_TOKEN'
        chat_id: 'YOUR_CHAT_ID'
  - name: 'email-warning'
    email_configs:
      - to: 'ops@example.com'
        from: 'alert@example.com'
        smarthost: 'smtp.example.com:587'
        auth_username: 'alert@example.com'
        auth_password: 'PASSWORD'
```

### 6.2. Alert nâng cao với Loki/Grafana
- **Alert theo pattern log (regex):**
  - Trong Grafana, tạo alert rule với query:
    ```
    count_over_time({job="network"} |= `panic` or |= `FATAL` or |= `CRITICAL` [5m]) > 0
    ```
  - Alert khi xuất hiện log chứa từ khoá nghiêm trọng trong 5 phút gần nhất.
- **Alert theo tần suất lỗi:**
    ```
    sum by (instance) (rate({job="network"} |= `error` [1m])) > 5
    ```
- **Routing alert Grafana:**
  - Cấu hình contact point: Slack, Telegram, Email, Webhook...
  - Có thể gửi alert về nhiều channel tuỳ theo severity.

### 6.3. Alert nâng cao với Splunk/Graylog
- **Splunk:**
  - Tạo alert khi có log matching pattern (regex, keyword, threshold)
  - Ví dụ: alert khi có log chứa "panic" hoặc "unauthorized access"
  - Cấu hình alert action: gửi email, webhook, Slack, run script tự động.
- **Graylog:**
  - Tạo stream lọc log theo điều kiện (level, message, source...)
  - Đặt alert khi số lượng log vượt ngưỡng hoặc xuất hiện pattern đặc biệt.
  - Gửi alert qua email, Slack, HTTP notification.

### 6.4. Lưu ý bảo mật & quy trình xử lý alert
- **Không gửi thông tin nhạy cảm (token, password, private key) trong nội dung alert.**
- **Chỉ gửi alert ra ngoài (Slack, Telegram, Email) khi đã kiểm duyệt nội dung.**
- **Có quy trình phân loại, acknowledge, xử lý và ghi chú lại từng alert.**
- **Lưu lại lịch sử alert để audit và tối ưu hoá hệ thống cảnh báo.**
