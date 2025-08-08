//! Performance benchmarks for repository operations
//!
//! These benchmarks measure the performance of critical repository operations
//! to ensure they meet performance requirements and detect regressions.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::time::Duration;
use tokio::runtime::Runtime;
use uuid::Uuid;

use m3u_proxy::{
    repositories::{
        ChannelRepository, RelayRepository, StreamProxyRepository,
        ChannelCreateRequest, traits::Repository,
    },
};

mod common;
use common::{
    fixtures::presets::*, fixtures::*,
    setup_test, test_uuid, time_utils,
};

/// Benchmark context with shared database and repositories
struct BenchmarkContext {
    relay_repo: RelayRepository,
    channel_repo: ChannelRepository,
    stream_proxy_repo: StreamProxyRepository,
    test_source_id: Uuid,
    test_profile_id: Uuid,
    rt: Runtime,
}

impl BenchmarkContext {
    fn new() -> Self {
        let rt = Runtime::new().unwrap();
        
        let (relay_repo, channel_repo, stream_proxy_repo, test_source_id, test_profile_id) = rt.block_on(async {
            let db = setup_test().await.unwrap();
            let repos = db.repositories();
            
            let test_source_id = test_uuid();
            let now_str = time_utils::now().to_rfc3339();
            
            // Setup test source
            sqlx::query(
                r#"INSERT INTO stream_sources (id, name, source_type, url, created_at, updated_at, is_active) 
                   VALUES (?, 'Benchmark Source', 'm3u', 'http://benchmark.com/playlist.m3u8', ?, ?, 1)"#
            )
            .bind(test_source_id.to_string())
            .bind(&now_str)
            .bind(&now_str)
            .execute(&db.pool)
            .await
            .unwrap();
            
            // Setup test relay profile
            let profile_request = basic_relay_profile();
            let test_profile = repos.relay.create(profile_request).await.unwrap();
            
            (repos.relay, repos.channel, repos.stream_proxy, test_source_id, test_profile.id)
        });
        
        Self {
            relay_repo,
            channel_repo,
            stream_proxy_repo,
            test_source_id,
            test_profile_id,
            rt,
        }
    }
}

// =============================================================================
// RELAY REPOSITORY BENCHMARKS
// =============================================================================

fn bench_relay_profile_create(c: &mut Criterion) {
    let ctx = BenchmarkContext::new();
    
    c.bench_function("relay_profile_create", |b| {
        b.to_async(&ctx.rt).iter(|| async {
            let mut request = basic_relay_profile();
            request.name = format!("Benchmark Profile {}", test_uuid());
            
            black_box(ctx.relay_repo.create(request).await.unwrap())
        })
    });
}

fn bench_relay_profile_find_by_id(c: &mut Criterion) {
    let ctx = BenchmarkContext::new();
    
    // Pre-create profiles for lookup
    let profile_ids: Vec<Uuid> = ctx.rt.block_on(async {
        let mut ids = Vec::new();
        for i in 0..100 {
            let mut request = basic_relay_profile();
            request.name = format!("Lookup Profile {}", i);
            let profile = ctx.relay_repo.create(request).await.unwrap();
            ids.push(profile.id);
        }
        ids
    });
    
    let mut counter = 0usize;
    c.bench_function("relay_profile_find_by_id", |b| {
        b.to_async(&ctx.rt).iter(|| {
            let id = profile_ids[counter % profile_ids.len()];
            counter += 1;
            async move {
                black_box(ctx.relay_repo.find_by_id(id).await.unwrap())
            }
        })
    });
}

fn bench_relay_profile_find_all(c: &mut Criterion) {
    let ctx = BenchmarkContext::new();
    
    // Pre-create varying numbers of profiles
    let sizes = vec![10, 50, 100, 500];
    
    for &size in &sizes {
        ctx.rt.block_on(async {
            // Clean previous profiles
            // Create profiles for this size
            for i in 0..size {
                let mut request = basic_relay_profile();
                request.name = format!("FindAll Profile {} Size {}", i, size);
                ctx.relay_repo.create(request).await.unwrap();
            }
        });
        
        c.bench_with_input(
            BenchmarkId::new("relay_profile_find_all", size),
            &size,
            |b, &_size| {
                b.to_async(&ctx.rt).iter(|| async {
                    let query = common::repositories::traits::QueryParams::new();
                    black_box(ctx.relay_repo.find_all(query).await.unwrap())
                })
            },
        );
    }
}

fn bench_relay_profile_update(c: &mut Criterion) {
    let ctx = BenchmarkContext::new();
    
    // Pre-create profiles for updating
    let profile_ids: Vec<Uuid> = ctx.rt.block_on(async {
        let mut ids = Vec::new();
        for i in 0..50 {
            let mut request = basic_relay_profile();
            request.name = format!("Update Profile {}", i);
            let profile = ctx.relay_repo.create(request).await.unwrap();
            ids.push(profile.id);
        }
        ids
    });
    
    let mut counter = 0usize;
    c.bench_function("relay_profile_update", |b| {
        b.to_async(&ctx.rt).iter(|| {
            let id = profile_ids[counter % profile_ids.len()];
            counter += 1;
            async move {
                let update_request = common::fixtures::presets::updates::relay_profile_name_update(
                    &format!("Updated Profile {}", test_uuid())
                );
                black_box(ctx.relay_repo.update(id, update_request).await.unwrap())
            }
        })
    });
}

// =============================================================================
// CHANNEL REPOSITORY BENCHMARKS
// =============================================================================

fn bench_channel_create(c: &mut Criterion) {
    let ctx = BenchmarkContext::new();
    
    c.bench_function("channel_create", |b| {
        b.to_async(&ctx.rt).iter(|| async {
            let mut request = basic_channel(ctx.test_source_id);
            request.channel_name = format!("Benchmark Channel {}", test_uuid());
            request.tvg_id = Some(format!("bench-{}", test_uuid()));
            
            black_box(ctx.channel_repo.create(request).await.unwrap())
        })
    });
}

fn bench_channel_bulk_create(c: &mut Criterion) {
    let ctx = BenchmarkContext::new();
    let mut group = c.benchmark_group("channel_bulk_create");
    
    for &size in &[10, 50, 100, 500] {
        group.throughput(Throughput::Elements(size));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.to_async(&ctx.rt).iter(|| async {
                let requests = multiple_channels(ctx.test_source_id, size as usize);
                black_box(ctx.channel_repo.create_bulk(requests).await.unwrap())
            })
        });
    }
    group.finish();
}

fn bench_channel_find_paginated(c: &mut Criterion) {
    let ctx = BenchmarkContext::new();
    
    // Pre-create channels for pagination
    ctx.rt.block_on(async {
        let channels = multiple_channels(ctx.test_source_id, 1000);
        ctx.channel_repo.create_bulk(channels).await.unwrap();
    });
    
    let mut group = c.benchmark_group("channel_find_paginated");
    
    for &page_size in &[10, 25, 50, 100] {
        group.bench_with_input(
            BenchmarkId::new("page_size", page_size),
            &page_size,
            |b, &page_size| {
                b.to_async(&ctx.rt).iter(|| async {
                    let query = common::repositories::ChannelQuery::new();
                    black_box(
                        ctx.channel_repo
                            .find_paginated(query, 1, page_size)
                            .await
                            .unwrap()
                    )
                })
            },
        );
    }
    group.finish();
}

fn bench_channel_source_replacement(c: &mut Criterion) {
    let ctx = BenchmarkContext::new();
    
    let mut group = c.benchmark_group("channel_source_replacement");
    group.sample_size(20); // Smaller sample size for expensive operations
    
    for &size in &[10, 50, 100, 200] {
        group.throughput(Throughput::Elements(size));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.to_async(&ctx.rt).iter(|| async {
                // Create initial channels
                let initial_channels = multiple_channels(ctx.test_source_id, size as usize);
                ctx.channel_repo.create_bulk(initial_channels).await.unwrap();
                
                // Create replacement channels
                let replacement_channels: Vec<m3u_proxy::models::Channel> = (0..size).map(|i| {
                    m3u_proxy::models::Channel {
                        id: test_uuid(),
                        source_id: ctx.test_source_id,
                        tvg_id: Some(format!("repl-{}", i)),
                        tvg_name: Some(format!("Replacement {}", i)),
                        tvg_chno: Some(format!("{:03}", i)),
                        tvg_logo: None,
                        tvg_shift: None,
                        group_title: Some("Replacement".to_string()),
                        channel_name: format!("Replacement Channel {}", i),
                        stream_url: format!("http://replacement.com/stream-{}.m3u8", i),
                        created_at: time_utils::now(),
                        updated_at: time_utils::now(),
                    }
                }).collect();
                
                black_box(
                    ctx.channel_repo
                        .update_source_channels(ctx.test_source_id, &replacement_channels)
                        .await
                        .unwrap()
                )
            })
        });
    }
    group.finish();
}

// =============================================================================
// STREAM PROXY REPOSITORY BENCHMARKS
// =============================================================================

fn bench_stream_proxy_create_with_relationships(c: &mut Criterion) {
    let ctx = BenchmarkContext::new();
    
    let mut group = c.benchmark_group("stream_proxy_create_with_relationships");
    
    for &relationship_count in &[1, 5, 10, 25] {
        // Pre-create sources for relationships
        let source_ids: Vec<Uuid> = ctx.rt.block_on(async {
            let mut ids = Vec::new();
            for i in 0..relationship_count {
                let source_id = test_uuid();
                let now_str = time_utils::now().to_rfc3339();
                
                sqlx::query(
                    r#"INSERT INTO stream_sources (id, name, source_type, url, created_at, updated_at, is_active) 
                       VALUES (?, ?, 'm3u', ?, ?, ?, 1)"#
                )
                .bind(source_id.to_string())
                .bind(format!("Benchmark Source {}", i))
                .bind(format!("http://bench{}.com/playlist.m3u8", i))
                .bind(&now_str)
                .bind(&now_str)
                .execute(&common::setup_test().await.unwrap().pool)
                .await
                .unwrap();
                
                ids.push(source_id);
            }
            ids
        });
        
        group.throughput(Throughput::Elements(relationship_count));
        group.bench_with_input(
            BenchmarkId::from_parameter(relationship_count),
            &relationship_count,
            |b, &_count| {
                b.to_async(&ctx.rt).iter(|| async {
                    let mut fixture = StreamProxyFixture::new()
                        .name(format!("Benchmark Proxy {}", test_uuid()));
                    
                    for (i, &source_id) in source_ids.iter().enumerate() {
                        fixture = fixture.add_stream_source(source_id, i as i32 + 1);
                    }
                    
                    let request = fixture.build();
                    black_box(
                        ctx.stream_proxy_repo
                            .create_with_relationships(request)
                            .await
                            .unwrap()
                    )
                })
            },
        );
    }
    group.finish();
}

fn bench_stream_proxy_get_relationships(c: &mut Criterion) {
    let ctx = BenchmarkContext::new();
    
    // Pre-create proxy with many relationships
    let (proxy_id, _source_count) = ctx.rt.block_on(async {
        let source_count = 100;
        let mut source_ids = Vec::new();
        let now_str = time_utils::now().to_rfc3339();
        
        // Create sources
        for i in 0..source_count {
            let source_id = test_uuid();
            
            sqlx::query(
                r#"INSERT INTO stream_sources (id, name, source_type, url, created_at, updated_at, is_active) 
                   VALUES (?, ?, 'm3u', ?, ?, ?, 1)"#
            )
            .bind(source_id.to_string())
            .bind(format!("Relation Source {}", i))
            .bind(format!("http://rel{}.com/playlist.m3u8", i))
            .bind(&now_str)
            .bind(&now_str)
            .execute(&common::setup_test().await.unwrap().pool)
            .await
            .unwrap();
            
            source_ids.push(source_id);
        }
        
        // Create proxy with all sources
        let mut fixture = StreamProxyFixture::new()
            .name("Relationship Benchmark Proxy");
        
        for (i, &source_id) in source_ids.iter().enumerate() {
            fixture = fixture.add_stream_source(source_id, i as i32 + 1);
        }
        
        let request = fixture.build();
        let proxy = ctx.stream_proxy_repo.create_with_relationships(request).await.unwrap();
        
        (proxy.id, source_count)
    });
    
    c.bench_function("stream_proxy_get_sources", |b| {
        b.to_async(&ctx.rt).iter(|| async {
            black_box(
                ctx.stream_proxy_repo
                    .get_proxy_sources(proxy_id)
                    .await
                    .unwrap()
            )
        })
    });
    
    c.bench_function("stream_proxy_get_sources_with_details", |b| {
        b.to_async(&ctx.rt).iter(|| async {
            black_box(
                ctx.stream_proxy_repo
                    .get_proxy_sources_with_details(proxy_id)
                    .await
                    .unwrap()
            )
        })
    });
}

// =============================================================================
// CROSS-REPOSITORY BENCHMARKS
// =============================================================================

fn bench_complete_workflow(c: &mut Criterion) {
    let ctx = BenchmarkContext::new();
    
    let mut group = c.benchmark_group("complete_workflow");
    group.sample_size(10); // Smaller sample for complex workflows
    group.measurement_time(Duration::from_secs(30));
    
    group.bench_function("full_setup_workflow", |b| {
        b.to_async(&ctx.rt).iter(|| async {
            // Create relay profile
            let mut profile_request = basic_relay_profile();
            profile_request.name = format!("Workflow Profile {}", test_uuid());
            let profile = ctx.relay_repo.create(profile_request).await.unwrap();
            
            // Create channels
            let channel_requests = multiple_channels(ctx.test_source_id, 20);
            let channels = ctx.channel_repo.create_bulk(channel_requests).await.unwrap();
            
            // Create proxy with relationships
            let proxy_request = StreamProxyFixture::new()
                .name(format!("Workflow Proxy {}", test_uuid()))
                .relay_mode()
                .with_relay_profile(profile.id)
                .add_stream_source(ctx.test_source_id, 1)
                .build();
            
            let proxy = ctx.stream_proxy_repo
                .create_with_relationships(proxy_request)
                .await
                .unwrap();
            
            // Create relay configs for first 5 channels
            for channel in channels.iter().take(5) {
                let config_request = ChannelRelayConfigFixture::new(profile.id)
                    .name(format!("Workflow Config {}", test_uuid()))
                    .build();
                
                ctx.relay_repo
                    .create_channel_config(proxy.id, channel.id, config_request)
                    .await
                    .unwrap();
            }
            
            black_box((profile, channels, proxy))
        })
    });
    
    group.finish();
}

// =============================================================================
// BENCHMARK GROUPS
// =============================================================================

criterion_group!(
    relay_benchmarks,
    bench_relay_profile_create,
    bench_relay_profile_find_by_id,
    bench_relay_profile_find_all,
    bench_relay_profile_update
);

criterion_group!(
    channel_benchmarks,
    bench_channel_create,
    bench_channel_bulk_create,
    bench_channel_find_paginated,
    bench_channel_source_replacement
);

criterion_group!(
    stream_proxy_benchmarks,
    bench_stream_proxy_create_with_relationships,
    bench_stream_proxy_get_relationships
);

criterion_group!(
    workflow_benchmarks,
    bench_complete_workflow
);

criterion_main!(
    relay_benchmarks,
    channel_benchmarks,
    stream_proxy_benchmarks,
    workflow_benchmarks
);