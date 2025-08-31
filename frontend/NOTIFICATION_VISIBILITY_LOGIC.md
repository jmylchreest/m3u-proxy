# Notification Visibility Logic

## Problem
Previously, once a progress event was marked as "seen" (hasBeenVisible = true), it would remain seen even when the event received updates. This meant users wouldn't be notified about important progress updates.

## Solution
Implemented smart visibility tracking that handles different scenarios:

### Visibility Rules

1. **New Events**
   - Always start with `hasBeenVisible = false`
   - Will show notification counter

2. **Updated In-Progress Events**
   - If event is still in progress (`processing`, `idle`, etc.)
   - Reset `hasBeenVisible = false` 
   - User sees the update in notification counter
   - Example: Progress goes from 20% to 80%

3. **Completed/Error Events**
   - Final states: `completed` or `error`
   - Preserve `hasBeenVisible` flag
   - Prevents re-notifying about already-seen completions
   - Example: User already saw "Operation completed"

4. **State Transitions**
   - Progress → Completed: Resets visibility (see the completion)
   - Completed → Completed: Preserves visibility (no re-notify)
   - Error → Error: Preserves visibility (no re-notify)

## Code Logic

```typescript
if (existingEvent) {
  const isFinalState = event.state === 'completed' || event.state === 'error'
  const wasFinalState = existingEvent.state === 'completed' || existingEvent.state === 'error'
  
  // Only preserve visibility if both old and new states are final
  if (isFinalState && wasFinalState) {
    notificationEvent.hasBeenVisible = existingEvent.hasBeenVisible
  } else if (!isFinalState && existingEvent.hasBeenVisible) {
    // Reset visibility for in-progress updates
    notificationEvent.hasBeenVisible = false
  }
}
```

## User Experience

### Before
- Start operation → Bell shows (1)
- User views → Bell clears
- Progress update → Bell stays clear ❌
- Operation completes → Bell stays clear ❌

### After
- Start operation → Bell shows (1)
- User views → Bell clears
- Progress update → Bell shows (1) ✅
- User views → Bell clears
- Operation completes → Bell shows (1) ✅
- User views → Bell clears
- (No more updates) → Bell stays clear ✅

## Benefits

1. **Users see important updates** - Progress changes are visible
2. **No spam** - Completed events don't re-notify
3. **Clear final states** - Users know when operations finish
4. **Better UX** - Notification counter accurately reflects unseen updates

## Testing

1. Start a long operation
2. Open notification bell (clears counter)
3. Wait for progress update → Counter should show (1)
4. View the update → Counter clears
5. Wait for completion → Counter should show (1)
6. View completion → Counter clears
7. Refresh page → Counter stays clear (completed events preserved as seen)