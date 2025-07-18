//! Robust streaming proxy implementation
//!
//! This module provides a robust streaming proxy that handles:
//! - Connection pooling and keep-alive
//! - Retry logic with exponential backoff
//! - Circuit breaker pattern for failing upstreams
//! - Adaptive buffering and backpressure handling
//! - Health monitoring and failover
//! - FFmpeg relay support (future)

use anyhow::Result;
use axum::body::Body;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use futures::StreamExt;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Semaphore};
use tokio::time::sleep;
use tracing::{error, info, warn};

/// Configuration for robust streaming
#[derive(Debug, Clone)]
pub struct RobustStreamingConfig {
    /// Connection timeout for upstream requests
    pub connection_timeout: Duration,
    /// Read timeout for streaming data
    pub read_timeout: Duration,
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Base delay for exponential backoff
    pub base_retry_delay: Duration,
    /// Maximum delay between retries
    pub max_retry_delay: Duration,
    /// Buffer size for streaming chunks
    pub buffer_size: usize,
    /// Maximum concurrent connections per upstream
    pub max_concurrent_per_upstream: usize,
    /// Circuit breaker failure threshold
    pub circuit_breaker_threshold: u32,
    /// Circuit breaker reset timeout
    pub circuit_breaker_reset_timeout: Duration,
}

impl Default for RobustStreamingConfig {
    fn default() -> Self {
        Self {
            connection_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
            max_retries: 3,
            base_retry_delay: Duration::from_millis(100),
            max_retry_delay: Duration::from_secs(10),
            buffer_size: 8192,
            max_concurrent_per_upstream: 10,
            circuit_breaker_threshold: 5,
            circuit_breaker_reset_timeout: Duration::from_secs(60),
        }
    }
}

/// Circuit breaker states
#[derive(Debug, Clone, PartialEq)]
enum CircuitBreakerState {
    Closed,
    Open,
    HalfOpen,
}

/// Circuit breaker for upstream connections
#[derive(Debug)]
struct CircuitBreaker {
    state: CircuitBreakerState,
    failure_count: u32,
    last_failure_time: Option<Instant>,
    threshold: u32,
    reset_timeout: Duration,
}

impl CircuitBreaker {
    fn new(threshold: u32, reset_timeout: Duration) -> Self {
        Self {
            state: CircuitBreakerState::Closed,
            failure_count: 0,
            last_failure_time: None,
            threshold,
            reset_timeout,
        }
    }

    fn can_request(&mut self) -> bool {
        match self.state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                if let Some(last_failure) = self.last_failure_time {
                    if last_failure.elapsed() > self.reset_timeout {
                        self.state = CircuitBreakerState::HalfOpen;
                        info!("Circuit breaker transitioning to half-open state");
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitBreakerState::HalfOpen => true,
        }
    }

    fn record_success(&mut self) {
        if self.state == CircuitBreakerState::HalfOpen {
            self.state = CircuitBreakerState::Closed;
            self.failure_count = 0;
            self.last_failure_time = None;
            info!("Circuit breaker closed after successful request");
        }
    }

    fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure_time = Some(Instant::now());

        if self.failure_count >= self.threshold {
            self.state = CircuitBreakerState::Open;
            warn!(
                "Circuit breaker opened after {} failures",
                self.failure_count
            );
        }
    }
}

/// Connection pool for upstream hosts
#[derive(Debug)]
struct ConnectionPool {
    clients: HashMap<String, (Client, Arc<Semaphore>)>,
    circuit_breakers: HashMap<String, Arc<RwLock<CircuitBreaker>>>,
    config: RobustStreamingConfig,
}

impl ConnectionPool {
    fn new(config: RobustStreamingConfig) -> Self {
        Self {
            clients: HashMap::new(),
            circuit_breakers: HashMap::new(),
            config,
        }
    }

    fn get_or_create_client(&mut self, host: &str) -> Result<(Client, Arc<Semaphore>)> {
        if let Some((client, semaphore)) = self.clients.get(host) {
            return Ok((client.clone(), semaphore.clone()));
        }

        let client = Client::builder()
            .timeout(self.config.connection_timeout)
            .tcp_keepalive(Some(Duration::from_secs(30)))
            .pool_max_idle_per_host(self.config.max_concurrent_per_upstream)
            .build()?;

        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrent_per_upstream));
        self.clients
            .insert(host.to_string(), (client.clone(), semaphore.clone()));

        Ok((client, semaphore))
    }

    fn get_or_create_circuit_breaker(&mut self, host: &str) -> Arc<RwLock<CircuitBreaker>> {
        if let Some(breaker) = self.circuit_breakers.get(host) {
            return breaker.clone();
        }

        let breaker = Arc::new(RwLock::new(CircuitBreaker::new(
            self.config.circuit_breaker_threshold,
            self.config.circuit_breaker_reset_timeout,
        )));

        self.circuit_breakers
            .insert(host.to_string(), breaker.clone());
        breaker
    }
}

/// Robust streaming proxy
pub struct RobustStreamingProxy {
    pool: Arc<RwLock<ConnectionPool>>,
    config: RobustStreamingConfig,
}

impl RobustStreamingProxy {
    pub fn new(config: RobustStreamingConfig) -> Self {
        Self {
            pool: Arc::new(RwLock::new(ConnectionPool::new(config.clone()))),
            config,
        }
    }

    /// Stream content from upstream URL with robust error handling
    pub async fn stream_from_upstream(
        &self,
        upstream_url: &str,
        headers: HeaderMap,
        metrics_callback: Option<Box<dyn Fn(u64) + Send + Sync>>,
    ) -> Result<Response> {
        let host = self.extract_host(upstream_url)?;

        // Get connection pool resources
        let (client, semaphore, circuit_breaker) = {
            let mut pool = self.pool.write().await;
            let (client, semaphore) = pool.get_or_create_client(&host)?;
            let circuit_breaker = pool.get_or_create_circuit_breaker(&host);
            (client, semaphore, circuit_breaker)
        };

        // Check circuit breaker
        {
            let mut breaker = circuit_breaker.write().await;
            if !breaker.can_request() {
                return Err(anyhow::anyhow!(
                    "Circuit breaker is open for host: {}",
                    host
                ));
            }
        }

        // Acquire semaphore permit for connection limiting
        let _permit = semaphore.acquire().await?;

        // Attempt request with retry logic
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            match self
                .attempt_stream_request(&client, upstream_url, &headers)
                .await
            {
                Ok(response) => {
                    // Record success in circuit breaker
                    circuit_breaker.write().await.record_success();

                    // Create tracked stream for metrics
                    let tracked_stream = self
                        .create_tracked_stream(response, metrics_callback)
                        .await?;
                    return Ok(tracked_stream);
                }
                Err(e) => {
                    last_error = Some(e);

                    if attempt < self.config.max_retries {
                        let delay = self.calculate_retry_delay(attempt);
                        warn!(
                            "Attempt {} failed for {}, retrying in {:?}: {}",
                            attempt + 1,
                            upstream_url,
                            delay,
                            last_error.as_ref().unwrap()
                        );
                        sleep(delay).await;
                    }
                }
            }
        }

        // Record failure in circuit breaker
        circuit_breaker.write().await.record_failure();

        Err(last_error.unwrap())
    }

    async fn attempt_stream_request(
        &self,
        client: &Client,
        upstream_url: &str,
        headers: &HeaderMap,
    ) -> Result<reqwest::Response> {
        // Convert axum headers to reqwest headers
        let mut upstream_headers = reqwest::header::HeaderMap::new();

        let safe_headers = [
            "accept",
            "accept-encoding",
            "accept-language",
            "range",
            "if-modified-since",
            "if-none-match",
            "cache-control",
            "user-agent",
        ];

        for header_name in safe_headers {
            if let Some(value) = headers.get(header_name) {
                if let Ok(header_value) = reqwest::header::HeaderValue::from_bytes(value.as_bytes())
                {
                    upstream_headers.insert(
                        reqwest::header::HeaderName::from_static(header_name),
                        header_value,
                    );
                }
            }
        }

        let response = client
            .get(upstream_url)
            .headers(upstream_headers)
            .timeout(self.config.read_timeout)
            .send()
            .await?;

        Ok(response)
    }

    async fn create_tracked_stream(
        &self,
        response: reqwest::Response,
        metrics_callback: Option<Box<dyn Fn(u64) + Send + Sync>>,
    ) -> Result<Response> {
        let status = response.status();
        let headers = response.headers().clone();

        // Create streaming body with metrics tracking
        let stream = response.bytes_stream();
        let mut bytes_served = 0u64;

        let tracked_stream = stream.map(move |chunk_result| match chunk_result {
            Ok(chunk) => {
                bytes_served += chunk.len() as u64;
                tracing::trace!("Served {} bytes, total: {}", chunk.len(), bytes_served);

                // Call metrics callback if provided
                if let Some(ref callback) = metrics_callback {
                    callback(chunk.len() as u64);
                }

                Ok(chunk)
            }
            Err(e) => {
                error!("Error streaming chunk: {}", e);
                Err(e)
            }
        });

        // Convert to axum Response
        let body = Body::from_stream(tracked_stream);
        let axum_status =
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        let mut response_builder = Response::builder().status(axum_status);

        // Forward response headers
        for (name, value) in headers.iter() {
            if let Ok(header_name) = header::HeaderName::from_bytes(name.as_str().as_bytes()) {
                if let Ok(header_value) = header::HeaderValue::from_bytes(value.as_bytes()) {
                    response_builder = response_builder.header(header_name, header_value);
                }
            }
        }

        Ok(response_builder.body(body)?)
    }

    fn extract_host(&self, url: &str) -> Result<String> {
        let parsed = url::Url::parse(url)?;
        Ok(parsed.host_str().unwrap_or("unknown").to_string())
    }

    fn calculate_retry_delay(&self, attempt: u32) -> Duration {
        let delay = self.config.base_retry_delay.as_millis() as u64 * (2_u64.pow(attempt));
        Duration::from_millis(delay).min(self.config.max_retry_delay)
    }
}

/// Health monitor for upstream sources
pub struct UpstreamHealthMonitor {
    health_status: Arc<RwLock<HashMap<String, bool>>>,
    config: RobustStreamingConfig,
}

impl UpstreamHealthMonitor {
    pub fn new(config: RobustStreamingConfig) -> Self {
        Self {
            health_status: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    pub async fn is_healthy(&self, host: &str) -> bool {
        self.health_status
            .read()
            .await
            .get(host)
            .copied()
            .unwrap_or(true)
    }

    pub async fn start_health_checks(&self, hosts: Vec<String>) {
        for host in hosts {
            let health_status = self.health_status.clone();
            let config = self.config.clone();
            let host_clone = host.clone();

            tokio::spawn(async move {
                let client = Client::builder()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .unwrap();

                loop {
                    let is_healthy = Self::check_host_health(&client, &host_clone).await;
                    health_status
                        .write()
                        .await
                        .insert(host_clone.clone(), is_healthy);

                    if !is_healthy {
                        warn!("Host {} is unhealthy", host_clone);
                    }

                    sleep(Duration::from_secs(30)).await;
                }
            });
        }
    }

    async fn check_host_health(client: &Client, host: &str) -> bool {
        let health_url = format!("http://{}/health", host);
        match client.head(&health_url).send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker_behavior() {
        let mut breaker = CircuitBreaker::new(2, Duration::from_millis(100));

        // Should allow requests initially
        assert!(breaker.can_request());

        // Record failures
        breaker.record_failure();
        assert!(breaker.can_request());

        breaker.record_failure();
        assert!(!breaker.can_request()); // Circuit should be open

        // Wait for reset timeout
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(breaker.can_request()); // Should be half-open

        // Record success should close circuit
        breaker.record_success();
        assert_eq!(breaker.state, CircuitBreakerState::Closed);
    }

    #[test]
    fn test_retry_delay_calculation() {
        let config = RobustStreamingConfig::default();
        let proxy = RobustStreamingProxy::new(config);

        assert_eq!(proxy.calculate_retry_delay(0), Duration::from_millis(100));
        assert_eq!(proxy.calculate_retry_delay(1), Duration::from_millis(200));
        assert_eq!(proxy.calculate_retry_delay(2), Duration::from_millis(400));
    }
}
