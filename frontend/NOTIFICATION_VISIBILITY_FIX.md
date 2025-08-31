# Notification Visibility Fix

## Problem
The notification bell was not clearing correctly because of complex state comparison logic that tried to be "smart" about when to reset visibility.

## Solution
Simplified the visibility tracking logic:

### Core Principle
**Any actual change to an event should reset its visibility** so users are notified of updates.

### Implementation

1. **Event Updates (ProgressProvider)**
   - Check if event actually changed (state, percentage, stage, message)
   - If changed: Reset `hasBeenVisible = false`
   - If unchanged: Preserve existing visibility (including for historical events from SSE)
   - Limit to 100 events maximum (matching backend)

2. **SSE Connection**
   - `include_completed` is a backend query parameter for SSE connection
   - Returns historical completed events when SSE connects
   - Previously seen events maintain their visibility status

### Code Changes

```typescript
// ProgressProvider.tsx
const hasChanged = 
  existingEvent.state !== event.state ||
  existingEvent.overall_percentage !== event.overall_percentage ||
  existingEvent.current_stage !== event.current_stage ||
  existingEvent.message !== event.message

if (hasChanged) {
  notificationEvent.hasBeenVisible = false
}
```

```typescript
// Event limit matches backend (100 events max)
if (newEvents.size > 100) {
  const sortedEvents = Array.from(newEvents.entries())
    .sort((a, b) => new Date(a[1].last_update).getTime() - new Date(b[1].last_update).getTime())
  
  const toRemove = sortedEvents.slice(0, newEvents.size - 100)
  toRemove.forEach(([id]) => newEvents.delete(id))
}
```

## Benefits

1. **Simpler Logic** - No complex state transition tracking
2. **Consistent Behavior** - Any change triggers notification
3. **Flexible Filtering** - Pages can choose what to show
4. **Better UX** - Users see all important updates

## Testing

1. Start an operation → Bell shows (1)
2. View notification → Bell clears
3. Operation updates → Bell shows (1) again
4. View update → Bell clears
5. Operation completes → Bell shows (1)
6. View completion → Bell clears
7. No duplicate events → Bell stays clear