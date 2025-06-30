# M3U Proxy Refactoring Plan

## Overview
This document outlines the comprehensive refactoring plan for the M3U Proxy codebase to improve architecture, reduce code duplication, and implement SOLID principles.

## Current Issues Analysis

### Major Code Duplication Problems
- **DateTime parsing** replicated across 4+ modules with slight variations
- **Database row mapping** patterns repeated in every database service
- **Logo URL generation** has 3 overlapping functions in utils.rs
- **Xtream server health checks** duplicated between stream/EPG sources
- **SQL query patterns** repeated throughout database modules

### SOLID Principle Violations
- **SRP**: Massive modules (models/mod.rs: 926 lines) mixing multiple concerns
- **OCP**: Hard-coded source type matching requires modification in multiple places
- **DIP**: Services directly depend on concrete SQLite implementation

## Refactoring Phases

### Phase 1: Error Handling and Utilities ✅ COMPLETED
**Status**: Implementation complete
- [x] Create centralized error handling system
- [x] Implement DateTimeParser utility
- [x] Refactor logo URL generation
- [x] Extract common validation utilities

**Completed work:**
- Created comprehensive error handling system in `/src/errors/`
- Implemented flexible datetime parsing in `/src/utils/datetime.rs`
- Refactored logo URL generation with builder pattern in `/src/utils/logo.rs`
- Added input validation utilities in `/src/utils/validation.rs`
- Added required dependencies (urlencoding)
- Code compiles successfully with new modules

### Phase 2: Repository Pattern ✅ COMPLETED
**Status**: Implementation complete
- [x] Define repository traits
- [x] Implement repository pattern for database access
- [x] Create database abstraction layer
- [ ] Refactor existing database modules (deferred to Phase 4)

**Completed work:**
- Created comprehensive repository traits in `/src/repositories/traits.rs`
  - `Repository<T, ID>` - Core CRUD operations
  - `BulkRepository<T, ID>` - Bulk operations with transactions
  - `SoftDeleteRepository<T, ID>` - Soft deletion support
  - `PaginatedRepository<T, ID>` - Pagination support
- Implemented concrete `StreamSourceRepository` with all traits
- Created placeholder repositories for `Channel` and `Filter`
- Added query parameter structures for filtering and searching
- Included comprehensive error handling with `RepositoryError`
- Code compiles successfully with new repository pattern

### Phase 3: Service Layer Abstractions 🔄 PENDING
**Status**: Not started
- [ ] Create service layer interfaces
- [ ] Implement business logic services
- [ ] Abstract away repository dependencies
- [ ] Add service-level error handling

### Phase 4: Source Type Handlers 🔄 PENDING
**Status**: Not started
- [ ] Create SourceHandler trait
- [ ] Implement M3U source handler
- [ ] Implement Xtream source handler
- [ ] Add source capability detection

### Phase 5: Web Layer Refactoring 🔄 PENDING
**Status**: Not started
- [ ] Refactor API handlers to use services
- [ ] Implement proper HTTP error handling
- [ ] Add request/response validation
- [ ] Create middleware for common concerns

### Phase 6: Testing and Documentation 🔄 PENDING
**Status**: Not started
- [ ] Add comprehensive unit tests
- [ ] Create integration tests
- [ ] Add API documentation
- [ ] Update README and deployment docs

## Recommended Module Structure

```
src/
├── lib.rs                          # Library root
├── errors/                         # Centralized error handling
│   ├── mod.rs
│   └── types.rs
├── models/                         # Domain models only
│   ├── mod.rs
│   ├── stream_source.rs
│   ├── channel.rs
│   └── data_mapping.rs
├── repositories/                   # Data access layer
│   ├── mod.rs
│   ├── stream_source.rs
│   ├── channel.rs
│   └── traits.rs
├── services/                       # Business logic layer
│   ├── mod.rs
│   ├── stream_source.rs
│   ├── data_mapping.rs
│   └── traits.rs
├── sources/                        # Source type handlers
│   ├── mod.rs
│   ├── m3u.rs
│   ├── xtream.rs
│   └── traits.rs
├── utils/                          # Utilities
│   ├── mod.rs
│   ├── datetime.rs
│   ├── logo.rs
│   └── validation.rs
├── web/                           # HTTP layer
│   ├── mod.rs
│   ├── handlers/
│   │   ├── mod.rs
│   │   ├── stream_sources.rs
│   │   └── data_mapping.rs
│   └── middleware/
└── database/                      # Database configuration
    ├── mod.rs
    └── migrations/
```

## Implementation Notes

### Key Principles
- **DRY**: Eliminate code duplication through abstraction
- **SOLID**: Apply all five principles consistently
- **Clean Architecture**: Separate concerns into layers
- **Error Handling**: Comprehensive error types and handling
- **Testing**: Test-driven development where possible

### Dependencies to Add
- `thiserror` for error handling
- `async-trait` for async traits
- Additional serde utilities

### Migration Strategy
1. Implement new modules alongside existing code
2. Gradually migrate existing functionality
3. Remove old code once new implementation is stable
4. Maintain backward compatibility during transition

## Progress Tracking

### Completed Tasks
- [x] Initial analysis and planning
- [x] Documentation created

### Current Task
- [ ] Phase 1: Error handling and utilities implementation

---

*Last updated: 2025-06-30*
*Next review: After Phase 1 completion*