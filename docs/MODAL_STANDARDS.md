# Modal Standards Documentation

This document describes the standardized modal system implemented across the M3U Proxy application.

## Overview

All modals in the application now use a consistent design pattern based on the channels modal. This ensures:
- Consistent user experience
- Proper accessibility
- Mobile-friendly design
- Standardized button placement and styling

## Standard Modal Structure

### HTML Structure

```html
<div id="modalId" class="modal">
    <div class="modal-content standard-modal">
        <div class="modal-header">
            <h3 class="modal-title">Modal Title</h3>
        </div>
        <div class="modal-body">
            <!-- Modal content goes here -->
            <!-- This section is scrollable if content overflows -->
        </div>
        <div class="modal-footer">
            <!-- Primary action button (leftmost) -->
            <button type="button" class="btn btn-primary" onclick="saveAction()">
                Save
            </button>
            <!-- Secondary actions and close button -->
            <button type="button" class="btn btn-secondary" onclick="cancelAction()">
                Cancel
            </button>
        </div>
    </div>
</div>
```

### CSS Classes

- `modal`: Base modal overlay
- `standard-modal`: Standardized modal sizing and layout
- `modal-header`: Fixed header with title only
- `modal-body`: Scrollable content area
- `modal-footer`: Fixed footer with right-aligned buttons

### Special Modal Variants

For larger modals (like preview modals), add the `preview-modal-large` class:

```html
<div class="modal-content standard-modal preview-modal-large">
```

## Design Specifications

### Sizing
- **Desktop**: 95% viewport width/height with 2.5vh margin
- **Mobile**: Full viewport (100vw x 100vh) with no margin

### Layout
- **Fixed Header**: Contains title only
- **Scrollable Body**: Main content area with custom scrollbars
- **Fixed Footer**: Right-aligned buttons with proper spacing

### Button Order (Right to Left)
1. **Cancel/Close** - Secondary action in footer, rightmost position
2. **Primary Action** - Main action button (Save, Create, etc.)
3. **Additional Actions** - Any other buttons (Preview, Test, etc.)

### Colors
- **Primary buttons**: Blue (#007bff)
- **Secondary buttons**: Gray (#6c757d)
- **All colors respect dark theme**

## JavaScript Integration

### Using SharedUtils

The `SharedUtils` class provides standardized modal functions:

```javascript
// Show modal
SharedUtils.showStandardModal('modalId');

// Hide modal
SharedUtils.hideStandardModal('modalId');

// Setup automatic close handlers (backdrop, escape key)
SharedUtils.setupStandardModalCloseHandlers('modalId');
```

### Custom Modal Functions

Each modal should have its own open/close functions:

```javascript
function openMyModal() {
    // Initialize modal data
    resetForm();
    loadData();
    
    // Show modal using standard utility
    SharedUtils.showStandardModal('myModal');
}

function closeMyModal() {
    // Clean up
    clearForm();
    
    // Hide modal using standard utility
    SharedUtils.hideStandardModal('myModal');
}
```

## Accessibility Features

### Keyboard Navigation
- **Escape key**: Closes modal
- **Tab navigation**: Focuses through modal elements
- **Focus outline**: Visible focus indicators

### Screen Readers
- Proper heading hierarchy
- Descriptive button text
- Clear modal structure

### Touch Support
- **44px minimum touch targets** on mobile
- **Touch action optimization** prevents double-tap zoom
- **Smooth scrolling** with momentum on iOS

## Mobile Responsiveness

### Viewport Adjustments
- Full screen on mobile devices
- Larger touch targets (44px minimum)
- Optimized font sizes (16px for inputs to prevent zoom)
- Improved button spacing

### Scrolling Behavior
- `-webkit-overflow-scrolling: touch` for smooth iOS scrolling
- `overscroll-behavior: contain` prevents scroll chaining
- Custom scrollbar styling for better visibility

## Examples of Implemented Modals

### Current Standard Modals
1. **Filter Modal** (`filterModal`) - Create/edit filters
2. **Examples Modal** (`examplesModal`) - Pattern examples
3. **Proxy Modal** (`proxyModal`) - Create/edit proxies  
4. **Proxy Preview Modal** (`proxyPreviewModal`) - Preview proxy content
5. **Rule Modal** (`ruleModal`) - Data mapping rules
6. **Logo Picker Modal** (`logoPickerModal`) - Choose logos
7. **Source Modal** (`sourceModal`) - Select sources
8. **Upload Modal** (`uploadModal`) - Logo uploads
9. **Edit Modal** (`editModal`) - Edit logos
10. **Create Profile Modal** (`createProfileModal`) - Relay profiles
11. **Channels Modal** (`channelsModal`) - View channels

### Button Layout Examples

**Filter Modal Footer:**
```html
<div class="modal-footer">
    <button class="btn btn-primary" onclick="saveFilter()">Save Filter</button>
    <button class="btn btn-secondary" onclick="cancelFilter()">Cancel</button>
</div>
```

**Proxy Modal Footer:**
```html
<div class="modal-footer">
    <button class="btn btn-outline-secondary" onclick="previewProxy()">👁️ Preview</button>
    <button class="btn btn-primary" onclick="saveProxy()">Save Proxy</button>
    <button class="btn btn-secondary" onclick="closeProxyModal()">Cancel</button>
</div>
```

## Dark Theme Support

All standard modals automatically support dark theme with:
- Proper background colors
- Adjusted border colors
- Input field styling
- Scrollbar theming
- Button color variations

## Migration Guide

### Converting Existing Modals

1. **Add standard-modal class**:
   ```html
   <div class="modal-content standard-modal">
   ```

2. **Reorder footer buttons** (right to left):
   - Primary action first
   - Secondary actions next
   - Cancel/Close button rightmost

4. **Update JavaScript functions** to use SharedUtils if desired

5. **Test on mobile devices** for touch interaction

### Common Pitfalls

- **Don't nest scrollable elements** within modal body
- **Don't use inline styles** for show/hide - use classes
- **Don't hardcode colors** - use CSS custom properties
- **Don't add close buttons to headers** - keep headers clean with title only

## Future Improvements

- Form validation integration
- Loading state management
- Modal stacking support
- Animation customization options
- Automated modal testing utilities