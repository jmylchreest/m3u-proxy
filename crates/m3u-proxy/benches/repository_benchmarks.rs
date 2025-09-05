//! Performance benchmarks for new features
//!
//! These benchmarks measure the performance of newly implemented features
//! to ensure they meet performance requirements and detect regressions.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::hint::black_box;

// =============================================================================
// DATABASE RETRY PERFORMANCE BENCHMARKS
// =============================================================================

/// Benchmark database retry mechanism performance impact
fn bench_database_retry_performance(c: &mut Criterion) {
    use m3u_proxy::utils::database_retry::{RetryConfig, with_retry};
    use m3u_proxy::errors::{RepositoryError, RepositoryResult};
    
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("database_retry_performance");
    
    // Benchmark retry configuration overhead for successful operations
    group.bench_function("successful_operation_no_retry", |b| {
        b.iter(|| {
            rt.block_on(async {
                // Direct operation call (no retry wrapper)
                async fn dummy_operation() -> RepositoryResult<i32> {
                    Ok(42)
                }
                black_box(dummy_operation().await.unwrap())
            })
        })
    });
    
    group.bench_function("successful_operation_with_retry", |b| {
        b.iter(|| {
            rt.block_on(async {
                let config = RetryConfig::for_reads();
                let result = with_retry(
                    &config,
                    || async { Ok::<i32, RepositoryError>(42) },
                    "benchmark_operation"
                ).await;
                black_box(result.unwrap())
            })
        })
    });
    
    // Benchmark different retry configurations
    for (name, config) in [
        ("read_config", RetryConfig::for_reads()),
        ("write_config", RetryConfig::for_writes()),
        ("critical_config", RetryConfig::for_critical()),
    ] {
        group.bench_with_input(BenchmarkId::new("retry_configs", name), &config, |b, config| {
            b.iter(|| {
                rt.block_on(async {
                    let result = with_retry(
                        config,
                        || async { Ok::<String, RepositoryError>("success".to_string()) },
                        "config_test"
                    ).await;
                    black_box(result.unwrap())
                })
            })
        });
    }
    
    group.finish();
}

// =============================================================================
// EXPRESSION PARSER BENCHMARKS
// =============================================================================

/// Benchmark expression parser with new comparison operators
fn bench_expression_parser_comparison_operators(c: &mut Criterion) {
    use m3u_proxy::expression_parser::FilterParser;
    
    let mut group = c.benchmark_group("expression_parser_comparison");
    let parser = FilterParser::new();
    
    // Test different complexity levels
    let simple_expressions = [
        "tvg_chno > \"100\"",
        "tvg_chno < \"500\"",
        "tvg_chno >= \"200\"",
        "tvg_chno <= \"300\"",
    ];
    
    let complex_expressions = [
        "tvg_chno >= \"100\" AND tvg_chno <= \"200\"",
        "channel_name contains \"HD\" AND tvg_chno > \"100\"",
        "(tvg_chno >= \"100\" OR channel_name contains \"Sport\") AND group_title not_equals \"\"",
        "tvg_chno >= \"100\" AND tvg_chno <= \"500\" AND channel_name contains \"BBC\"",
    ];
    
    // Benchmark simple comparison expressions
    for (i, expression) in simple_expressions.iter().enumerate() {
        group.bench_with_input(
            BenchmarkId::new("simple_comparison", i),
            expression,
            |b, expr| {
                b.iter(|| {
                    black_box(parser.parse(expr).unwrap())
                })
            }
        );
    }
    
    // Benchmark complex expressions with comparisons
    for (i, expression) in complex_expressions.iter().enumerate() {
        group.bench_with_input(
            BenchmarkId::new("complex_comparison", i),
            expression,
            |b, expr| {
                b.iter(|| {
                    black_box(parser.parse(expr).unwrap())
                })
            }
        );
    }
    
    // Benchmark data mapping expressions with comparisons
    let data_mapping_expressions = [
        "tvg_chno > \"500\" SET group_title = \"High Channels\"",
        "tvg_chno >= \"100\" AND tvg_chno <= \"200\" SET group_title = \"Standard Channels\"",
        "(channel_name contains \"Sport\" OR tvg_chno >= \"400\") SET group_title = \"Sports & Premium\"",
    ];
    
    for (i, expression) in data_mapping_expressions.iter().enumerate() {
        group.bench_with_input(
            BenchmarkId::new("data_mapping_comparison", i),
            expression,
            |b, expr| {
                b.iter(|| {
                    black_box(parser.parse_extended(expr).unwrap())
                })
            }
        );
    }
    
    // Benchmark time helper parsing
    let time_expressions = [
        "@time:now",
        "@time:2024-01-01T12:00:00Z",
        "@time:2024-01-01 12:00:00",
        "@time:1704110400", // epoch timestamp
    ];
    
    for (i, expression) in time_expressions.iter().enumerate() {
        group.bench_with_input(
            BenchmarkId::new("time_helper", i),
            expression,
            |b, expr| {
                b.iter(|| {
                    // Test time helper parsing performance
                    use m3u_proxy::utils::datetime::DateTimeParser;
                    let time_str = expr.strip_prefix("@time:").unwrap_or(expr);
                    black_box(DateTimeParser::parse_flexible(time_str))
                })
            }
        );
    }
    
    group.finish();
}

// =============================================================================
// TIME PARSING BENCHMARKS  
// =============================================================================

/// Benchmark time parsing performance with parse_flexible
fn bench_time_parsing_performance(c: &mut Criterion) {
    use m3u_proxy::utils::datetime::DateTimeParser;
    
    let mut group = c.benchmark_group("time_parsing_performance");
    
    let test_cases = [
        ("rfc3339", "2024-01-01T12:00:00Z"),
        ("sqlite_datetime", "2024-01-01 12:00:00"),
        ("iso8601_with_tz", "2024-01-01T12:00:00+01:00"),
        ("xmltv_format", "20240101120000 +0000"),
        ("european_format", "01/01/2024 12:00:00"),
        ("us_format", "01/01/2024 12:00:00 PM"),
        ("epoch_timestamp", "1704110400"),
    ];
    
    for (name, time_str) in test_cases {
        group.bench_with_input(
            BenchmarkId::new("parse_flexible", name),
            &time_str,
            |b, time_str| {
                b.iter(|| {
                    black_box(DateTimeParser::parse_flexible(time_str))
                })
            }
        );
    }
    
    // Benchmark throughput for batch time parsing
    group.bench_function("batch_time_parsing", |b| {
        let time_strings: Vec<&str> = vec![
            "2024-01-01T12:00:00Z",
            "2024-01-01 12:00:00", 
            "1704110400",
            "20240101120000 +0000",
        ];
        
        b.iter(|| {
            for _ in 0..250 { // 250 * 4 = 1000 total operations
                for time_str in &time_strings {
                    let _ = black_box(DateTimeParser::parse_flexible(time_str));
                }
            }
        })
    });
    
    group.finish();
}

// =============================================================================
// EXPRESSION VALIDATION BENCHMARKS
// =============================================================================

/// Benchmark expression validation performance
fn bench_expression_validation_performance(c: &mut Criterion) {
    use m3u_proxy::expression_parser::FilterParser;
    
    let mut group = c.benchmark_group("expression_validation_performance");
    let parser = FilterParser::new();
    
    let expressions = [
        "tvg_chno > \"100\"",
        "tvg_chno >= \"100\" AND tvg_chno <= \"200\"",
        "channel_name contains \"HD\" AND tvg_chno > \"100\"",
        "(tvg_chno >= \"100\" OR channel_name contains \"Sport\") AND group_title not_equals \"\"",
        "tvg_chno >= \"100\" AND tvg_chno <= \"500\" AND channel_name contains \"BBC\"",
    ];
    
    group.bench_function("batch_expression_validation", |b| {
        b.iter(|| {
            for expr in &expressions {
                let _ = black_box(parser.parse(expr));
            }
        })
    });
    
    // Individual expression complexity benchmarks
    for (i, expression) in expressions.iter().enumerate() {
        group.bench_with_input(
            BenchmarkId::new("validate_expression", i),
            expression,
            |b, expr| {
                b.iter(|| {
                    black_box(parser.parse(expr))
                })
            }
        );
    }
    
    group.finish();
}

// =============================================================================
// BENCHMARK GROUPS
// =============================================================================

criterion_group!(
    new_features_benchmarks,
    bench_database_retry_performance,
    bench_expression_parser_comparison_operators,
    bench_time_parsing_performance,
    bench_expression_validation_performance
);

criterion_main!(new_features_benchmarks);