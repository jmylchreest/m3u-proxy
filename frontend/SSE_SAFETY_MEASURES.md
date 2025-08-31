# SSE Connection Safety Measures

## Overview
This document describes the safety measures implemented to prevent SSE connections from hanging the browser when the backend is unavailable.

## Key Safety Features Implemented

### 1. Backend Connectivity Check Before SSE
**Location**: `providers/backend-connectivity-provider.tsx`
- Health check to `/live` endpoint with 10-second timeout
- Periodic checks every 60 seconds when connected
- Retry every 30 seconds when disconnected
- Prevents app from rendering when backend is down

### 2. SSE Connection Prevention When Backend Unavailable
**Location**: `providers/ProgressProvider.tsx`
```typescript
// Don't attempt SSE connection if backend is not available
if (!backendConnected) {
  debug.log('ProgressProvider: Backend not connected, skipping SSE setup')
  setConnected(false)
  // Destroy any existing SSE connection when backend goes down
  sseManager.destroy()
  return
}
```

### 3. Exponential Backoff for SSE Reconnection
**Location**: `lib/sse-singleton.ts`
- Exponential backoff: 1s, 2s, 4s, 8s... up to 30s max
- Maximum 10 reconnection attempts before giving up
- Resets attempt counter on successful connection
```typescript
const reconnectDelay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 30000)
if (this.reconnectAttempts > 10) {
  this.debug.error('Too many reconnection attempts, stopping')
  return
}
```

### 4. Logs Client Safety
**Location**: `components/logs.tsx` & `lib/logs-client.ts`
- Only connects when backend is available
- Exponential backoff for reconnection
- Maximum 5 attempts before giving up
- Resets on manual reconnection

### 5. Backend Unavailable Page
**Location**: `components/backend-unavailable.tsx`
- Shows when backend is down
- Auto-retry every 30 seconds (first 5 attempts)
- Manual retry button
- Health endpoint test link
- No SSE connections attempted on this page

## Architecture Flow

```
App Start
    ↓
BackendConnectivityProvider
    ↓
Check /live endpoint (10s timeout)
    ↓
If Failed → Show BackendUnavailable page (NO SSE)
    ↓
If Success → Render App
    ↓
ProgressProvider checks backendConnected
    ↓
If Connected → Initialize SSE with safety measures
If Not Connected → Destroy SSE, skip initialization
```

## Connection States

### When Backend Available
1. BackendConnectivityProvider: Connected ✓
2. ProgressProvider: Initializes SSE ✓
3. SSE Singleton: Attempts connection with backoff ✓
4. Logs Client: Connects with backoff ✓

### When Backend Unavailable
1. BackendConnectivityProvider: Not Connected ✗
2. BackendUnavailable page shown ✓
3. ProgressProvider: Destroys SSE, skips init ✓
4. SSE Singleton: Not created ✓
5. Logs Client: Disconnected ✓

### Recovery Process
1. Backend comes back online
2. BackendConnectivityProvider detects (30s retry)
3. App re-renders with normal layout
4. ProgressProvider re-initializes SSE
5. Connections established with fresh attempt counters

## Testing Checklist

- [ ] Start app with backend down → Should show error page, no browser hang
- [ ] Stop backend while app running → SSE should stop reconnecting after 10 attempts
- [ ] Start backend after being down → Should reconnect automatically
- [ ] Network interruption → Should handle with exponential backoff
- [ ] Multiple tabs open → Each should handle independently
- [ ] Check browser DevTools Network tab → No infinite connection attempts
- [ ] Check browser performance → No memory leaks from failed connections

## Browser Resource Protection

### Memory Management
- Events Map cleaned up (keeps max 100 events)
- Old completed events removed when seen
- SSE connections properly closed on destroy

### Network Protection
- Single SSE connection per type (progress, logs)
- Connection pooling prevented
- Proper cleanup on component unmount
- No connections during error state

### CPU Protection
- Exponential backoff prevents tight reconnection loops
- Maximum attempt limits prevent infinite retries
- Polling intervals reasonable (30s, 60s)

## Failure Modes Handled

1. **Backend completely down**: Show error page, no SSE attempts
2. **Backend intermittent**: Exponential backoff, max attempts
3. **Network timeout**: 10s timeout on health check, SSE timeout handling
4. **Browser resource exhaustion**: Prevented by connection limits
5. **Infinite reconnection loop**: Prevented by max attempts + backoff
6. **Memory bloat**: Event cleanup + connection destruction

## Monitoring

To verify safety measures are working:

1. **Browser Console**: Check for connection attempt logs
2. **Network Tab**: Verify no excessive SSE attempts
3. **Performance Tab**: Monitor memory usage
4. **Backend Logs**: Check for connection spam

## Future Improvements

1. Add connection quality indicator in UI
2. User-configurable retry settings
3. Connection pooling for multiple SSE types
4. WebSocket fallback option
5. Service Worker for offline handling