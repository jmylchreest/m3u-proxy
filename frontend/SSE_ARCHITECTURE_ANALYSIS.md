# SSE Architecture Analysis and Recommendations

## Current Issues

### 1. Multiple SSE Connection Systems
**Problem**: The application currently has THREE separate SSE implementations:
- `sse-singleton.ts` - New singleton for progress events (used by ProgressProvider)
- `sse-client.ts` - Old client still imported by some pages
- `logs-client.ts` - Separate connection for logs

**Impact**: 
- Multiple connections to backend (inefficient)
- Inconsistent behavior across pages
- Potential for browser resource exhaustion

### 2. Notification Bell Not Clearing
**Problem**: The notification counter doesn't clear when messages are viewed.

**Root Causes**:
- `getAllEvents()` was using an optional parameter that NotificationBell didn't pass
- Events marked as visible but counter not updating properly
- Too many completed events accumulating in memory

**Fixed**: 
- Removed optional parameter from `getAllEvents()`
- Added automatic cleanup of old completed/seen events
- Improved `markAsVisible` logic in NotificationBell component

### 3. Browser Crash When Backend Unavailable
**Problem**: Infinite reconnection loop without proper backoff causes browser to become unresponsive.

**Fixed**:
- Added exponential backoff (1s, 2s, 4s, 8s... up to 30s)
- Maximum 10 reconnection attempts before giving up
- Reset attempt counter on successful connection

### 4. Include_Completed Flag Confusion
**Issue**: SSE connection always includes completed events (`include_completed=true`) but ProgressProvider filters them locally on most pages.

**Current Behavior**:
- Events page: Shows all events including completed
- Other pages: Filter out completed events locally
- This is actually OK - keeps flexibility for different page needs

## Architecture Recommendations

### Short-term Fixes (Completed)
✅ Fixed notification bell clearing issue
✅ Added exponential backoff to prevent browser crashes
✅ Added automatic cleanup of old events to prevent memory bloat
✅ Ensured SSE connection starts properly in ProgressProvider

### Medium-term Improvements Needed

1. **Remove Old SSE Client**
   - Remove imports of `sse-client.ts` from stream-sources, epg-sources, proxies pages
   - Migrate these pages to use ProgressProvider hooks instead
   - Delete the old `sse-client.ts` file

2. **Consolidate Logs with Progress Events**
   - Consider merging logs into the same SSE stream as progress events
   - Use event types to differentiate (type: "progress" vs type: "log")
   - This would reduce to a single SSE connection

3. **Better Event Type System**
   ```typescript
   type SSEEvent = 
     | { type: 'progress', data: ProgressEvent }
     | { type: 'log', data: LogEntry }
     | { type: 'notification', data: NotificationData }
   ```

### Long-term Architecture Proposal

1. **Single SSE Manager**
   - One connection for ALL real-time events
   - Event routing based on type
   - Subscription management by event type and resource ID

2. **Event Persistence Strategy**
   - In-progress events: Keep in memory
   - Completed events: Keep last 50 for UI reference
   - Logs: Stream only, no persistence in frontend
   - Use localStorage for important notifications

3. **Connection Management**
   - Single connection initiated at app level
   - Shared across all components via context
   - Smart reconnection with backoff
   - Connection pooling prevention

## How Different Pages Should Work

### Events Page (`/events`)
- Shows ALL events including completed
- Converts ProgressEvents to ServiceEvent format for display
- Real-time updates via ProgressProvider
- No filtering of completed events

### Logs Page (`/logs`)
- Separate SSE connection for log streaming (for now)
- Could be merged into main SSE in future
- Real-time log display with filtering

### Source Pages (Stream/EPG)
- Should use ProgressProvider's `subscribeToType()`
- Filter by operation_type ('stream_ingestion', 'epg_ingestion')
- Show only relevant in-progress operations

### Notification Bell
- Shows recent activity across all pages
- Unread counter for new events
- Marks as read when popup opened or while open
- Auto-cleanup of old events

## Implementation Status

### Completed
- ✅ Fixed notification bell clearing
- ✅ Added reconnection backoff
- ✅ Event cleanup mechanism
- ✅ Proper SSE singleton initialization

### TODO
- [ ] Remove old sse-client.ts usage from remaining pages
- [ ] Consider merging logs into main SSE stream
- [ ] Add localStorage persistence for important notifications
- [ ] Implement event type discrimination system
- [ ] Add connection health monitoring UI

## Testing Recommendations

1. **Test notification clearing**: 
   - Open notification bell
   - Trigger new events
   - Verify counter clears while open

2. **Test reconnection**:
   - Stop backend
   - Verify no browser crash
   - Start backend
   - Verify reconnection works

3. **Test memory usage**:
   - Run for extended period
   - Monitor events Map size
   - Verify old events are cleaned up

4. **Test multiple tabs**:
   - Open app in multiple tabs
   - Verify each has single connection
   - Check resource usage