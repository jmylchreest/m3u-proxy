# M3U Proxy Kubernetes Deployment

This directory contains Kubernetes manifests for deploying m3u-proxy with Intel GPU acceleration support.

## Prerequisites

### 1. Intel GPU Device Plugin
For Intel GPU acceleration, you need the Intel GPU device plugin installed in your cluster:

```bash
# Apply the GPU device plugin
kubectl apply -f gpu-device-plugin.yaml

# Verify GPU nodes are detected
kubectl get nodes -o json | jq '.items[].metadata.labels' | grep gpu
```

### 2. Node Requirements
Ensure your Kubernetes nodes have:
- Intel integrated graphics (iGPU) or discrete GPU
- Proper GPU drivers installed
- `/dev/dri` devices available
- Node feature discovery (NFD) for automatic GPU detection (recommended)

### 3. Storage Class (Optional)
For better performance, configure a fast storage class:
```yaml
# Example storage class for NVMe/SSD
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: fast-ssd
provisioner: kubernetes.io/no-provisioner
volumeBindingMode: WaitForFirstConsumer
```

## Quick Deployment

Deploy everything at once:
```bash
# Apply all manifests
kubectl apply -f .

# Check deployment status
kubectl get all -n m3u-proxy

# Check GPU resources
kubectl describe nodes | grep -A5 -B5 "gpu.intel.com"
```

## Step-by-Step Deployment

1. **Create namespace and storage:**
   ```bash
   kubectl apply -f namespace.yaml
   kubectl apply -f pvc.yaml
   ```

2. **Configure application:**
   ```bash
   kubectl apply -f configmap.yaml
   ```

3. **Deploy the application:**
   ```bash
   kubectl apply -f deployment.yaml
   kubectl apply -f service.yaml
   ```

4. **Set up external access (choose one):**
   ```bash
   # Option A: Ingress (recommended)
   kubectl apply -f ingress.yaml
   
   # Option B: Port forwarding (development)
   kubectl port-forward -n m3u-proxy service/m3u-proxy 8080:8080
   
   # Option C: LoadBalancer service (if available)
   # Uncomment LoadBalancer section in service.yaml
   ```

## Configuration

### Environment Variables
Edit `configmap.yaml` to customize:
- `M3U_PROXY_HOST`: Bind address (default: 0.0.0.0)
- `M3U_PROXY_PORT`: Port number (default: 8080)
- `M3U_PROXY_LOG_LEVEL`: Logging level (debug, info, warn, error)
- `M3U_PROXY_DATABASE_URL`: Database location

### Resource Limits
Adjust in `deployment.yaml`:
```yaml
resources:
  requests:
    memory: "512Mi"
    cpu: "500m"
    gpu.intel.com/i915: 1  # Request 1 GPU
  limits:
    memory: "2Gi"
    cpu: "2000m"
    gpu.intel.com/i915: 1  # Limit to 1 GPU
```

### GPU Group Permissions
The deployment includes supplemental groups for GPU access:
```yaml
supplementalGroups:
  - 44    # video group
  - 109   # render group
```
**Note:** These GIDs may vary by Linux distribution. Check your nodes:
```bash
kubectl debug node/YOUR-NODE -it --image=busybox -- getent group video render
```

## Troubleshooting

### Check GPU Availability
```bash
# List GPU resources on nodes
kubectl get nodes -o yaml | grep -A10 -B10 gpu.intel.com

# Check if GPU device plugin is running
kubectl get pods -n kube-system | grep intel-gpu

# Debug GPU access in pod
kubectl exec -n m3u-proxy deployment/m3u-proxy -- ls -la /dev/dri
```

### Check Application Logs
```bash
# View application logs
kubectl logs -n m3u-proxy deployment/m3u-proxy -f

# Look for GPU detection messages
kubectl logs -n m3u-proxy deployment/m3u-proxy | grep -i "hardware\|gpu\|vaapi"
```

### Common Issues

1. **GPU Not Detected:**
   - Ensure GPU device plugin is running
   - Check node has GPU drivers installed
   - Verify `/dev/dri` exists on nodes

2. **Permission Denied on GPU:**
   - Check supplementalGroups in deployment
   - Verify video/render group GIDs match your nodes
   - Consider running with privileged: true (not recommended for production)

3. **Database Issues:**
   - Verify PVC is bound: `kubectl get pvc -n m3u-proxy`
   - Check volume permissions: `kubectl exec -n m3u-proxy deployment/m3u-proxy -- ls -la /app/data`

4. **Pod Scheduling Issues:**
   - Remove nodeSelector if GPU nodes aren't labeled
   - Check tolerations match node taints
   - Verify resource requests are available

## Monitoring

### Basic Health Checks
```bash
# Check service endpoints
kubectl get endpoints -n m3u-proxy

# Test HTTP health check
kubectl exec -n m3u-proxy deployment/m3u-proxy -- wget -qO- http://localhost:8080/health
```

### Resource Usage
```bash
# View resource consumption
kubectl top pods -n m3u-proxy

# Check GPU usage (requires GPU metrics)
kubectl get --raw "/api/v1/nodes/YOUR-NODE/proxy/metrics" | grep gpu
```

## Production Considerations

1. **Security:**
   - Enable TLS/HTTPS in ingress
   - Set up authentication (basic auth, OAuth, etc.)
   - Use network policies to restrict traffic
   - Regular security updates for base images

2. **Backup:**
   - Implement database backup strategy
   - Consider using external database (PostgreSQL)
   - Backup configuration and secrets

3. **High Availability:**
   - For read replicas, consider StatefulSet
   - External database for true multi-replica setup
   - Health checks and proper resource limits

4. **Monitoring:**
   - Prometheus metrics scraping
   - Grafana dashboards for visualization
   - Log aggregation (ELK, Fluentd)
   - GPU utilization monitoring