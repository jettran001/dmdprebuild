# Hướng dẫn auto-scaling, monitoring, service discovery cho network/docker

## 1. Auto-scaling với Kubernetes

- Sử dụng HorizontalPodAutoscaler (HPA):
```yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: network-hpa
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: network-main
  minReplicas: 2
  maxReplicas: 10
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70
```
- Đảm bảo deployment có resource requests/limits.

## 2. Auto-scaling với Docker Swarm

- Sử dụng deploy_mode: replicated:
```yaml
services:
  network:
    image: your-image
    deploy:
      mode: replicated
      replicas: 3
```
- Kết hợp với monitoring để scale thủ công hoặc dùng external scaler.

## 3. Cloud Monitoring

- **AWS CloudWatch**: Cài đặt CloudWatch Agent trong container hoặc node host, cấu hình gửi metrics/logs về AWS.
- **GCP Monitoring**: Cài đặt Stackdriver Agent, cấu hình gửi metrics/logs về Google Cloud.
- **Prometheus/Grafana**: Đã có hướng dẫn cấu hình scrape endpoint `/metrics` trong docker-compose và prometheus.yml.

## 4. Service Discovery

- **Consul**:
  - Chạy Consul agent trong cluster, các service đăng ký với Consul.
  - Service khác truy vấn Consul để lấy địa chỉ runtime.
- **etcd**:
  - Dùng cho service discovery phân tán, lưu trữ key-value endpoint.
- **Eureka**:
  - Phù hợp cho hệ thống microservice Java/Spring, có thể tích hợp với Rust qua REST API.

## 5. Lưu ý bảo mật
- Không hardcode secret trong image, dùng secret manager hoặc env.
- Đảm bảo health check, rate-limiting, validate input cho mọi service.

## 6. Nếu bạn cần sử dụng IPFS, hãy kết nối tới server IPFS ngoài (container riêng) qua HTTP API (ví dụ: http://ipfs-node:5001), không sử dụng plugin ipfs nội bộ.

---
**Mọi cấu hình production nên test kỹ trên staging trước khi rollout!** 