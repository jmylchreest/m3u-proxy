# Data Mapping Test Fix Summary

## Overview
This document summarizes the fixes applied to resolve issues with the data mapping rule test functionality in the modal interface.

## 🐛 Problems Identified

### **1. Missing `source_type` Field Error**
**Error**: `Failed to deserialize the JSON body into the target type: missing field 'source_type' at line 1 column 555`

**Root Cause**: The frontend was sending incomplete payload to the `/api/data-mapping/test` endpoint.

**Expected Backend Structure**:
```rust
pub struct DataMappingTestRequest {
    pub source_id: Uuid,
    pub source_type: DataMappingSourceType,  // <-- Missing!
    pub conditions: Vec<DataMappingConditionRequest>,
    pub actions: Vec<DataMappingActionRequest>,
}
```

**Actual Frontend Payload**:
```json
{
  "source_id": "dbb6c8dd-2064-445d-8505-945eb2b0dfc0",
  "conditions": [...],
  "actions": [...]
  // Missing "source_type" field
}
```

### **2. Missing `tvg_shift` Field in Test Results**
**Issue**: The `tvg_shift` field was not included in the test results, so changes to this field were not displayed.

**Root Cause**: The backend test endpoint was not including `tvg_shift` in the `original_values` and `mapped_values` response.

## ✅ Solutions Implemented

### **1. Fixed Missing `source_type` Field**

**File**: `m3u-proxy/static/js/data-mapping.js`
**Function**: `testRuleWithSource()`

**Before**:
```javascript
const testData = {
  source_id: sourceId,
  conditions: formData.conditions,
  actions: formData.actions,
};
```

**After**:
```javascript
// Get the source type from the form
const sourceTypeSelect = document.getElementById("ruleSourceType");
const sourceType = sourceTypeSelect ? sourceTypeSelect.value : "stream";

const testData = {
  source_id: sourceId,
  source_type: sourceType,  // <-- Added missing field
  conditions: formData.conditions,
  actions: formData.actions,
};
```

**Source Type Values**: 
- `"stream"` for Stream sources
- `"epg"` for EPG sources

**Serialization**: The Rust enum uses `#[serde(rename_all = "lowercase")]`, so it correctly deserializes the lowercase values.

### **2. Fixed Missing `tvg_shift` Field in Test Results**

**File**: `m3u-proxy/src/web/api.rs`
**Function**: `test_data_mapping_rule()`

**Before**:
```rust
// Original values missing tvg_shift
original_values.insert("channel_name".to_string(), Some(mc.original.channel_name.clone()));
original_values.insert("tvg_id".to_string(), mc.original.tvg_id.clone());
original_values.insert("tvg_name".to_string(), mc.original.tvg_name.clone());
original_values.insert("tvg_logo".to_string(), mc.original.tvg_logo.clone());
original_values.insert("group_title".to_string(), mc.original.group_title.clone());

// Mapped values missing tvg_shift
mapped_values.insert("channel_name".to_string(), Some(mc.mapped_channel_name.clone()));
mapped_values.insert("tvg_id".to_string(), mc.mapped_tvg_id.clone());
mapped_values.insert("tvg_name".to_string(), mc.mapped_tvg_name.clone());
mapped_values.insert("tvg_logo".to_string(), mc.mapped_tvg_logo.clone());
mapped_values.insert("group_title".to_string(), mc.mapped_group_title.clone());
```

**After**:
```rust
// Original values with tvg_shift
original_values.insert("channel_name".to_string(), Some(mc.original.channel_name.clone()));
original_values.insert("tvg_id".to_string(), mc.original.tvg_id.clone());
original_values.insert("tvg_name".to_string(), mc.original.tvg_name.clone());
original_values.insert("tvg_logo".to_string(), mc.original.tvg_logo.clone());
original_values.insert("tvg_shift".to_string(), mc.original.tvg_shift.clone());  // <-- Added
original_values.insert("group_title".to_string(), mc.original.group_title.clone());

// Mapped values with tvg_shift
mapped_values.insert("channel_name".to_string(), Some(mc.mapped_channel_name.clone()));
mapped_values.insert("tvg_id".to_string(), mc.mapped_tvg_id.clone());
mapped_values.insert("tvg_name".to_string(), mc.mapped_tvg_name.clone());
mapped_values.insert("tvg_logo".to_string(), mc.mapped_tvg_logo.clone());
mapped_values.insert("tvg_shift".to_string(), mc.mapped_tvg_shift.clone());  // <-- Added
mapped_values.insert("group_title".to_string(), mc.mapped_group_title.clone());
```

## 🧪 Testing & Validation

### **Test Scenario**
**Rule Configuration**:
- **Conditions**: Channel name matches regex pattern for timeshift detection
- **Actions**: Set `tvg_shift` field value
- **Source Type**: Stream source with channels

### **Expected Behavior**
1. ✅ Modal opens without JavaScript errors
2. ✅ Source selector loads available sources
3. ✅ Test executes successfully when source is selected
4. ✅ Results show `tvg_shift` field changes
5. ✅ UI displays: `"TVG Shift: "" → "+1"` (or similar)

### **Validation Results**
- ✅ **API Deserialization**: No more "missing field" errors
- ✅ **Test Execution**: Rules execute successfully against test data
- ✅ **Field Display**: `tvg_shift` changes now visible in results
- ✅ **Source Type Support**: Works with both Stream and EPG sources

## 📊 Impact Analysis

### **Functional Improvements**
- **Data Mapping Testing**: ✅ Fully functional individual rule testing
- **User Experience**: ✅ Clear visibility of all field changes including timeshift
- **Error Handling**: ✅ Eliminated deserialization errors
- **Field Coverage**: ✅ Complete field set now included in test results

### **Technical Consistency**
- **API Contract Compliance**: Frontend now sends complete required payload
- **Field Parity**: Test results include all available channel fields
- **Source Type Awareness**: Test properly handles both Stream and EPG source types

## 🎯 Key Technical Details

### **Data Flow**
1. **Frontend**: User selects rule parameters and test source
2. **Frontend**: Determines source type from form (`ruleSourceType` select)
3. **Frontend**: Sends complete payload including `source_type`
4. **Backend**: Deserializes request successfully with all required fields
5. **Backend**: Executes rule against source channels
6. **Backend**: Returns complete field set including `tvg_shift`
7. **Frontend**: Displays all field changes in test results

### **Source Type Mapping**
| Frontend Value | Backend Enum | Use Case |
|----------------|--------------|----------|
| `"stream"` | `DataMappingSourceType::Stream` | M3U/Xtream stream sources |
| `"epg"` | `DataMappingSourceType::Epg` | XMLTV EPG sources |

### **Field Support**
**Included Fields**:
- `channel_name` - Channel display name
- `tvg_id` - Channel identifier for EPG matching
- `tvg_name` - Alternative channel name for EPG
- `tvg_logo` - Channel logo URL
- `tvg_shift` - Timeshift offset (e.g., "+1", "+24") ✅ **Now included**
- `group_title` - Channel category/group

## 🚀 Current Status

### **✅ Fully Operational**
- Individual rule testing in modal interface
- Complete field change visibility including timeshift
- Support for both Stream and EPG source types
- Proper error handling and user feedback

### **✅ Integration Points**
- Works with existing data mapping engine
- Compatible with current rule creation workflow  
- Integrates with source selection interface
- Supports all available channel fields

---

**Summary**: Successfully resolved both the API deserialization issue and missing field display problem, restoring full functionality to the data mapping rule test feature with complete field coverage including the critical `tvg_shift` field.