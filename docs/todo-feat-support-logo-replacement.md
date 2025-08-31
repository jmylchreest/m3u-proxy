# Feature Request: Logo Image Replacement Support

## Overview
Add support for replacing the actual image file in existing logo assets, not just updating metadata (name/description).

## Current Limitation
The existing `PUT /api/v1/logos/{id}` endpoint only supports updating metadata:
- Name (display name)
- Description (optional text)

To replace an image currently requires:
1. Delete existing logo asset (`DELETE /api/v1/logos/{id}`)
2. Upload new logo asset (`POST /api/v1/logos`)

This loses the asset ID and any references to it.

## Proposed Solution

### Option A: New Dedicated Endpoint
Create `PUT /api/v1/logos/{id}/image` endpoint that:
- Accepts multipart file upload (like the upload endpoint)
- Validates image file type and size
- Replaces the existing file on disk
- Updates database fields: `file_name`, `file_size`, `mime_type`, `width`, `height`, `updated_at`
- Preserves the asset ID and metadata (name, description)
- Optionally keeps old file as backup before replacement

### Option B: Enhanced Update Endpoint
Modify existing `PUT /api/v1/logos/{id}` to handle both:
- JSON payload for metadata updates (current behavior)
- Multipart form data for image + metadata updates (new behavior)

## Implementation Details

### API Specification
```yaml
PUT /api/v1/logos/{id}/image
Content-Type: multipart/form-data

Parameters:
- file: (required) Image file (PNG, JPEG, GIF, WebP, SVG)
- name: (optional) Update display name
- description: (optional) Update description

Responses:
- 200: Logo image replaced successfully
- 400: Invalid file type or missing file
- 404: Logo asset not found
- 500: Server error
```

### Backend Changes Required

1. **New API Handler**
   - `update_logo_asset_image()` in `src/web/api.rs`
   - Handle multipart form parsing
   - Validate uploaded file
   - Call service method

2. **Service Method**
   - `LogoAssetService::replace_asset_image()` in `src/logo_assets/service.rs`
   - Backup existing file (optional)
   - Save new file using existing `save_uploaded_file()` logic
   - Update database record with new file metadata
   - Clean up old file

3. **Database Updates**
   - Update `logo_assets` table fields:
     - `file_name`
     - `file_path` 
     - `file_size`
     - `mime_type`
     - `width`
     - `height`
     - `updated_at`

4. **File Management**
   - Handle file replacement in storage
   - Maintain consistent file naming scheme
   - Optional backup of replaced files

### Error Handling
- Validate file type matches supported formats
- Check file size limits
- Handle disk space issues
- Rollback on database update failure
- Preserve original file if replacement fails

### Security Considerations
- Same image validation as upload endpoint
- File size limits
- MIME type validation
- Prevent path traversal attacks

### Backward Compatibility
- Existing metadata-only update endpoint remains unchanged
- New image replacement is additive functionality
- No breaking changes to current API

## Benefits
- Preserve asset IDs and references
- Atomic image replacement operation
- Better user experience
- Maintain data relationships

## Files to Modify
- `src/web/api.rs` - New handler function
- `src/web/mod.rs` - Add route
- `src/web/openapi.rs` - Add OpenAPI documentation
- `src/logo_assets/service.rs` - New service method
- `src/models/logo_asset.rs` - Request/response models (if needed)

## Testing Requirements
- Unit tests for service method
- Integration tests for API endpoint
- File upload validation tests
- Error handling tests
- File cleanup tests

## Priority
Medium - Quality of life improvement that eliminates need for delete/re-upload workflow.