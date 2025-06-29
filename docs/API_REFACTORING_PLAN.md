# API Refactoring Plan - Hierarchical Endpoint Structure

## Overview
This document outlines the comprehensive API refactoring plan for the m3u-proxy system, focusing on creating a consistent hierarchical endpoint structure that follows REST principles and improves maintainability.

## Design Principles

### Path Parameters vs Query Parameters
- **Path Parameters**: Used for resource identification and hierarchical relationships (essential to resource identity)
- **Query Parameters**: Used for optional filtering, searching, pagination, and non-essential modifiers

### Hierarchical Resource Structure
Resources should follow clear parent-child relationships:
```
/api/sources/{type}/{id}/filters        # "filters that belong to this source"
/api/sources/{type}/{id}/channels       # "channels that belong to this source"  
/api/sources/{type}/{id}/progress       # "progress of this source"
```

## Refactoring Recommendations

### **1. Data Mapping Preview Consolidation** (High Priority - IMPLEMENT FIRST)

**Current Structure:**
```
GET /api/data-mapping/preview/:source_id
GET /api/data-mapping/preview
```

**Recommended Changes:**
```
GET /api/sources/stream/{id}/data-mapping/preview   # Preview for specific stream source
GET /api/sources/epg/{id}/data-mapping/preview      # Preview for specific EPG source
POST /api/data-mapping/rules/test                   # Test individual rules (for rule modal)
```

**Implementation Notes:**
- The preview endpoint should use the same apply logic as the generator
- Apply all active rules to all channels for the specified source
- Return formatted preview data for the preview modal
- The test endpoint is separate for testing individual rules in the UI modal

### **2. Filters API Reorganization** (High Priority)

**Current Structure:**
```
GET /api/filters
POST /api/filters
GET /api/filters/:id
PUT /api/filters/:id
DELETE /api/filters/:id
GET /api/filters/fields
POST /api/filters/test
```

**Recommended Changes:**
```
# Source-specific filters
GET /api/sources/stream/{id}/filters     # Filters for specific stream source
POST /api/sources/stream/{id}/filters    # Create filter for specific stream source
GET /api/sources/epg/{id}/filters        # Filters for specific EPG source
POST /api/sources/epg/{id}/filters       # Create filter for specific EPG source

# Cross-source filter operations
GET /api/filters/stream                  # All stream filters (across sources)
GET /api/filters/epg                     # All EPG filters (across sources)
GET /api/filters/:id                     # Get specific filter
PUT /api/filters/:id                     # Update specific filter
DELETE /api/filters/:id                  # Delete specific filter
GET /api/filters/stream/fields           # Available fields for stream filters
GET /api/filters/epg/fields              # Available fields for EPG filters
POST /api/filters/test                   # Test filter logic
```

### **3. Progress API Consolidation** (Medium Priority - IMPLEMENT SECOND)

**Current Scattered Structure:**
```
GET /api/sources/stream/:id/progress
GET /api/progress
```

**Recommended Changes:**
```
# Hierarchical progress endpoints
GET /api/progress                           # All progress across system
GET /api/sources/stream/{id}/progress       # Progress for specific stream source  
GET /api/sources/epg/{id}/progress          # Progress for specific EPG source

# Specialized progress endpoints
GET /api/progress/operations                # Background operations
GET /api/progress/jobs                      # Scheduled jobs
GET /api/progress/sources                   # All source-related progress
```

### **4. Channel Mappings RESTful Structure** (Medium Priority)

**Current Structure:**
```
GET /api/channel-mappings
POST /api/channel-mappings
DELETE /api/channel-mappings/:id
POST /api/channel-mappings/auto-map
```

**Recommended Changes:**
```
# RESTful CRUD operations
GET /api/channel-mappings?stream_source_id={id}&epg_source_id={id}
POST /api/channel-mappings
GET /api/channel-mappings/:id
PUT /api/channel-mappings/:id
DELETE /api/channel-mappings/:id

# Bulk and utility operations
POST /api/channel-mappings/bulk-create
POST /api/channel-mappings/auto-generate
GET /api/channel-mappings/suggestions?stream_source_id={id}&epg_source_id={id}

# Source-specific mappings
GET /api/sources/stream/{id}/channel-mappings
GET /api/sources/epg/{id}/channel-mappings
```

### **5. Logo Assets API Standardization** (Lower Priority)

**Current Structure:** (Already mostly good)
```
GET /api/logos
POST /api/logos/upload
GET /api/logos/:id
PUT /api/logos/:id
DELETE /api/logos/:id
GET /api/logos/:id/formats
GET /api/logos/search
GET /api/logos/stats
```

**Minor Improvements:**
```
# Enhanced search and filtering
GET /api/logos?search={query}&format={format}&source_id={id}
POST /api/logos/upload
POST /api/logos/bulk-upload                 # Future enhancement
GET /api/logos/:id
PUT /api/logos/:id
DELETE /api/logos/:id
GET /api/logos/:id/formats
GET /api/logos/stats
```

### **6. EPG Data API Expansion** (Future Enhancement)

**Current Minimal Structure:**
```
GET /api/epg/viewer
```

**Proposed Future Structure:**
```
GET /api/epg/data?source_id={id}&channel_id={id}&date={date}
GET /api/epg/programs?channel_id={id}&start={datetime}&end={datetime}
GET /api/epg/channels?source_id={id}
GET /api/epg/guide?date={date}&channels={ids}
POST /api/epg/refresh
GET /api/epg/stats

# Source-specific EPG data
GET /api/sources/epg/{id}/programs
GET /api/sources/epg/{id}/guide
```

## Complete Hierarchical Endpoint Structure

### **Sources & Their Resources**
```
# Stream Sources
GET /api/sources/stream
POST /api/sources/stream  
GET /api/sources/stream/{id}
PUT /api/sources/stream/{id}
DELETE /api/sources/stream/{id}
POST /api/sources/stream/{id}/refresh
POST /api/sources/stream/{id}/cancel
GET /api/sources/stream/{id}/progress
GET /api/sources/stream/{id}/processing
GET /api/sources/stream/{id}/channels
GET /api/sources/stream/{id}/filters
POST /api/sources/stream/{id}/filters
GET /api/sources/stream/{id}/data-mapping/preview
GET /api/sources/stream/{id}/channel-mappings

# EPG Sources (same hierarchical pattern)
GET /api/sources/epg
POST /api/sources/epg
GET /api/sources/epg/{id}
PUT /api/sources/epg/{id}
DELETE /api/sources/epg/{id}
POST /api/sources/epg/{id}/refresh
GET /api/sources/epg/{id}/progress
GET /api/sources/epg/{id}/channels
GET /api/sources/epg/{id}/filters
POST /api/sources/epg/{id}/filters
GET /api/sources/epg/{id}/data-mapping/preview
GET /api/sources/epg/{id}/channel-mappings
GET /api/sources/epg/{id}/programs
GET /api/sources/epg/{id}/guide

# All Sources
GET /api/sources                        # List all sources (stream + EPG)
```

### **Cross-Source Resources**
```
# Filters
GET /api/filters/stream                 # All stream filters
GET /api/filters/epg                   # All EPG filters  
GET /api/filters/:id
PUT /api/filters/:id
DELETE /api/filters/:id
GET /api/filters/stream/fields
GET /api/filters/epg/fields
POST /api/filters/test

# Progress
GET /api/progress                      # All progress
GET /api/progress/operations           # Background operations
GET /api/progress/jobs                 # Scheduled jobs
GET /api/progress/sources              # All source-related progress

# Data Mapping
GET /api/data-mapping/rules
POST /api/data-mapping/rules
GET /api/data-mapping/rules/:id
PUT /api/data-mapping/rules/:id
DELETE /api/data-mapping/rules/:id
POST /api/data-mapping/rules/reorder
POST /api/data-mapping/rules/test      # Test individual rules

# Channel Mappings
GET /api/channel-mappings
POST /api/channel-mappings
GET /api/channel-mappings/:id
PUT /api/channel-mappings/:id
DELETE /api/channel-mappings/:id
POST /api/channel-mappings/bulk-create
POST /api/channel-mappings/auto-generate
GET /api/channel-mappings/suggestions
```

## Implementation Priority

### **Phase 1: High Priority (Implement First)**
1. **Data Mapping Preview Consolidation**
   - Move to hierarchical structure under sources
   - Separate test endpoint for individual rule testing
   - Use same apply logic as generator for preview

2. **Progress API Consolidation**
   - Consolidate scattered progress endpoints
   - Create clear hierarchy under sources
   - Add specialized progress categories

### **Phase 2: Medium Priority**
3. **Filters API Reorganization**
   - Add source-type organization
   - Implement hierarchical structure
   - Maintain backward compatibility during transition

4. **Channel Mappings RESTful Structure**
   - Make fully RESTful with proper CRUD
   - Add bulk operations
   - Improve suggestion system

### **Phase 3: Lower Priority**
5. **Logo Assets API Standardization**
6. **EPG Data API Expansion**

## Benefits of This Approach

### **1. Clear Resource Hierarchy**
- Parent-child relationships are explicit in the URL structure
- Easy to understand what belongs to what
- Logical navigation through the API

### **2. RESTful Design**
- Proper use of HTTP methods
- Clear resource identification
- Consistent patterns across all endpoints

### **3. Type Safety**
- Source types are explicit in the path
- No ambiguity about what type of resource is being accessed
- Better error handling and validation

### **4. Maintainability**
- Consistent patterns reduce code duplication
- Clear separation of concerns
- Easier to add new source types or operations

### **5. API Discoverability**
- Users can logically navigate the API structure
- Self-documenting through URL hierarchy
- Clear relationship between resources

## Migration Strategy

### **Backward Compatibility**
- Keep existing endpoints during transition period
- Add deprecation warnings to old endpoints
- Provide clear migration documentation

### **Frontend Updates**
- Update API calls to use new hierarchical structure
- Handle both old and new response formats during transition
- Update documentation and examples

### **Testing**
- Comprehensive testing of new endpoints
- Regression testing for existing functionality
- Performance testing for hierarchical queries

## Files to Modify

### **Backend**
- `src/web/mod.rs` - Route definitions
- `src/web/api.rs` - API implementations
- `src/models/mod.rs` - Any new model structures needed

### **Frontend**
- `static/js/data-mapping.js` - Update preview API calls
- `static/js/sources.js` - Update progress API calls
- `static/js/filters.js` - Update filter API calls (if exists)

### **Documentation**
- `README.md` - Update API documentation
- Create migration guides
- Update OpenAPI/Swagger documentation