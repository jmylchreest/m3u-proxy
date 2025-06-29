# DLQ Management Testing Checklist

## Prerequisites
- M3U Proxy server running
- At least one EPG source configured with some conflicts (or test data)
- Web browser with developer tools available

## 1. Navigation & UI Updates

### ✅ Navigation Changes
- [ ] Verify EPG Viewer is removed from main navigation menu
- [ ] Verify EPG Sources page loads correctly
- [ ] Verify new button layout in EPG Sources header
- [ ] Check "📺 View EPG" button is present next to "Add EPG Source"

### ✅ Quick Actions Section
- [ ] Verify "Refresh All EPG" button is removed
- [ ] Verify "⚠️ View Conflicts (DLQ)" button is present
- [ ] Verify "📺 View EPG Schedule" button is present
- [ ] Check button descriptions are accurate

## 2. DLQ Management Modal

### ✅ Modal Display
- [ ] Click "⚠️ View Conflicts (DLQ)" button
- [ ] Verify modal opens with proper styling
- [ ] Check modal header shows "EPG Conflicts (DLQ)"
- [ ] Verify loading indicator appears initially

### ✅ DLQ Content Loading
- [ ] Verify statistics section loads with conflict summary
- [ ] Check "Total Conflicts" number is displayed
- [ ] Verify "Common Patterns" section shows detected patterns
- [ ] Confirm conflict count matches statistics

### ✅ DLQ Entries Table
- [ ] Verify table headers: Channel, Conflict Type, Count, First Seen, Actions
- [ ] Check entries display channel names and IDs
- [ ] Verify conflict type badges (duplicate_identical vs duplicate_conflicting)
- [ ] Confirm occurrence counts are shown
- [ ] Check first seen dates are formatted correctly

### ✅ DLQ Filtering
- [ ] Test search functionality in conflict filter box
- [ ] Verify filtering works by channel name
- [ ] Verify filtering works by channel ID
- [ ] Check filter results update dynamically

### ✅ DLQ Resolution Actions
- [ ] Click "Suggest Mapping" button on a conflict
- [ ] Verify mapping suggestion appears in prompt
- [ ] Test accepting suggested mapping
- [ ] Verify success message appears
- [ ] Check conflict is resolved after mapping

- [ ] Click "Ignore" button on a conflict
- [ ] Verify confirmation dialog appears
- [ ] Test confirming ignore action
- [ ] Verify success message appears
- [ ] Check conflict is marked as resolved

## 3. EPG Viewer Modal

### ✅ Modal Access
- [ ] Click "📺 View EPG" button in header
- [ ] Click "📺 View EPG Schedule" in quick actions
- [ ] Verify both buttons open the same modal
- [ ] Check modal displays properly with EPG viewer content

### ✅ EPG Viewer Functionality
- [ ] Verify date selector is present and functional
- [ ] Test time range selector (6h, 12h, 24h options)
- [ ] Check start time input works
- [ ] Verify channel filter input is present
- [ ] Test "🔄 Refresh" button functionality
- [ ] Test "📍 Now" button (should set current date/time)

### ✅ EPG Data Display
- [ ] Verify loading indicator appears when fetching data
- [ ] Check "No EPG Data Available" message when no data
- [ ] Verify channel count is displayed correctly
- [ ] Test EPG data loads for available channels

### ✅ Modal Controls
- [ ] Verify close button (×) works
- [ ] Test clicking outside modal closes it
- [ ] Check modal is responsive on different screen sizes

## 4. API Endpoints Testing

### ✅ DLQ API Endpoints
- [ ] Test GET `/api/epg/dlq` endpoint
- [ ] Verify response includes both entries and statistics
- [ ] Check statistics include total_conflicts, by_source, by_conflict_type
- [ ] Verify common_patterns array is populated

- [ ] Test POST `/api/epg/dlq/resolve` endpoint
- [ ] Send "remap" action with test data
- [ ] Send "ignore" action with test data
- [ ] Verify response includes resolved_count and errors array

### ✅ EPG Viewer API
- [ ] Test GET `/api/epg/viewer` endpoint with parameters
- [ ] Verify response format matches expected structure
- [ ] Check date range filtering works
- [ ] Test channel filtering parameter

## 5. Enhanced Channel Mapping

### ✅ Automatic Mapping
- [ ] Add EPG source with HD/SD channel variants
- [ ] Verify channels are automatically mapped during ingestion
- [ ] Check "Channel HD" becomes "Channel_HD.tv"
- [ ] Verify "Channel SD" becomes "Channel_SD.tv"

### ✅ Conflict Prevention
- [ ] Add EPG source with potential conflicts
- [ ] Verify enhanced mapping reduces DLQ entries
- [ ] Check duplicate identical channels are handled
- [ ] Verify remaining conflicts are legitimate

## 6. Error Handling & Edge Cases

### ✅ Network Errors
- [ ] Test DLQ modal with network disconnected
- [ ] Verify appropriate error messages appear
- [ ] Test EPG viewer modal with network issues
- [ ] Check graceful degradation

### ✅ Empty States
- [ ] Test DLQ modal with no conflicts
- [ ] Verify empty state message is appropriate
- [ ] Test EPG viewer with no data
- [ ] Check "No EPG Data Available" message

### ✅ Large Datasets
- [ ] Test DLQ modal with many conflicts (>100)
- [ ] Verify performance is acceptable
- [ ] Test scrolling in DLQ entries table
- [ ] Check search/filter performance

## 7. Cross-Browser Compatibility

### ✅ Browser Testing
- [ ] Test in Chrome/Chromium
- [ ] Test in Firefox
- [ ] Test in Safari (if available)
- [ ] Test in Edge (if available)

### ✅ Mobile Responsiveness
- [ ] Test on mobile viewport
- [ ] Verify modals are responsive
- [ ] Check touch interactions work
- [ ] Test modal close on mobile

## 8. Integration Testing

### ✅ Workflow Integration
- [ ] Create EPG source → Check for conflicts → Resolve conflicts
- [ ] Verify resolved conflicts don't reappear
- [ ] Test EPG viewer shows resolved channels
- [ ] Check statistics update after resolution

### ✅ Data Persistence
- [ ] Resolve conflicts and restart server
- [ ] Verify resolutions persist across restarts
- [ ] Check resolution notes are saved
- [ ] Verify statistics remain accurate

## 9. Performance Testing

### ✅ Load Times
- [ ] Measure DLQ modal load time
- [ ] Measure EPG viewer modal load time
- [ ] Check API response times
- [ ] Verify no memory leaks in browser

### ✅ Concurrent Usage
- [ ] Test multiple users accessing DLQ simultaneously
- [ ] Verify no race conditions in conflict resolution
- [ ] Check database locking works correctly

## 10. Accessibility Testing

### ✅ Keyboard Navigation
- [ ] Test Tab navigation through modal elements
- [ ] Verify Enter/Space key interactions
- [ ] Test Escape key closes modals
- [ ] Check focus management

### ✅ Screen Reader Support
- [ ] Test with screen reader (if available)
- [ ] Verify modal titles are announced
- [ ] Check button labels are descriptive
- [ ] Test table headers are properly associated

## Bug Reporting Template

When issues are found, report with:
- **Browser**: Chrome/Firefox/Safari/Edge version
- **Steps to Reproduce**: Detailed steps
- **Expected Result**: What should happen
- **Actual Result**: What actually happened
- **Console Errors**: Any JavaScript errors
- **Network Tab**: Relevant API call failures
- **Screenshots**: Visual issues

## Post-Testing Cleanup

- [ ] Clear browser cache and cookies
- [ ] Reset test database to known state
- [ ] Document any configuration changes made
- [ ] Record performance benchmarks for future comparison