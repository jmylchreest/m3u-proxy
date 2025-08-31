# Circuit Breaker System

The M3U Proxy includes a configurable circuit breaker system for resilience against service failures.

## Configuration

Add circuit breaker settings to your `config.toml`:

```toml
[circuitbreaker]
# Global defaults for all services
[circuitbreaker.global]
implementation_type = "rssafe"    # Options: "rssafe", "noop"
failure_threshold = 3             # Failures before opening circuit
operation_timeout = "5s"          # Operation timeout
reset_timeout = "30s"             # Wait time before half-open
success_threshold = 2             # Successes to close circuit

# Service-specific overrides
[circuitbreaker.profiles.database]
implementation_type = "rssafe"
failure_threshold = 2
operation_timeout = "3s"
reset_timeout = "60s"
success_threshold = 3
```

## Usage in Code

```rust
use crate::services::CircuitBreakerManager;
use crate::utils::circuit_breaker::create_circuit_breaker_for_service;

// Get circuit breaker for a service
let circuit_breaker = manager.get_circuit_breaker("database").await?;

// Execute operation through circuit breaker
let result = circuit_breaker.execute(|| async {
    // Your risky operation here
    database.execute_query().await
        .map_err(|e| e.to_string())
}).await;
```

## API Endpoints

- `GET /api/v1/circuit-breakers` - Get all circuit breaker statistics
- `GET /api/v1/circuit-breakers/config` - Get current configuration
- `PUT /api/v1/circuit-breakers/config` - Update configuration
- `PUT /api/v1/circuit-breakers/services/{name}` - Update service profile
- `POST /api/v1/circuit-breakers/services/{name}/force` - Force open/closed

## Configuration Options

- `implementation_type`: "rssafe" (production) or "noop" (testing)
- `failure_threshold`: Number of failures before opening circuit
- `operation_timeout`: Individual operation timeout (e.g., "5s", "30s")
- `reset_timeout`: Wait time before trying half-open state
- `success_threshold`: Successes needed to close from half-open

## States

- **Closed**: Normal operation, requests pass through
- **Open**: Circuit is broken, requests are blocked
- **Half-Open**: Testing if service has recovered

See `config.circuit-breaker-example.toml` for complete examples.