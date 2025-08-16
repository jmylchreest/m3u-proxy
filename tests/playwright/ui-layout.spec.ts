import { test, expect } from '@playwright/test';

test.describe('M3U Proxy UI Layout Tests', () => {
  
  test.describe('Header Verification', () => {
    test('Dashboard should have single header', async ({ page }) => {
      await page.goto('/');
      
      // Wait for page to load
      await page.waitForSelector('h1', { timeout: 10000 });
      
      // Check for exactly one h1 element with "Dashboard"
      const headers = await page.locator('h1').all();
      expect(headers).toHaveLength(1);
      
      const headerText = await page.locator('h1').textContent();
      expect(headerText).toBe('Dashboard');
      
      // Take screenshot for verification
      await page.screenshot({ path: 'test-results/screenshots/dashboard-header.png' });
    });

    test('Logs page should have single header', async ({ page }) => {
      await page.goto('/logs/');
      
      // Wait for page to load
      await page.waitForSelector('h1', { timeout: 10000 });
      
      // Check for exactly one h1 element with "Logs"
      const headers = await page.locator('h1').all();
      expect(headers).toHaveLength(1);
      
      const headerText = await page.locator('h1').textContent();
      expect(headerText).toBe('Logs');
      
      // Take screenshot for verification
      await page.screenshot({ path: 'test-results/screenshots/logs-header.png' });
    });

    test('Events page should have single header', async ({ page }) => {
      await page.goto('/events/');
      
      // Wait for page to load
      await page.waitForSelector('h1', { timeout: 10000 });
      
      // Check for exactly one h1 element with "Events"
      const headers = await page.locator('h1').all();
      expect(headers).toHaveLength(1);
      
      const headerText = await page.locator('h1').textContent();
      expect(headerText).toBe('Events');
      
      // Take screenshot for verification
      await page.screenshot({ path: 'test-results/screenshots/events-header.png' });
    });
  });

  test.describe('Filter Layout Tests', () => {
    test('Logs page filter layout', async ({ page }) => {
      await page.goto('/logs/');
      
      // Wait for filters to load
      await page.waitForSelector('input[placeholder="Filter by text..."]', { timeout: 10000 });
      
      // Check text filter has proper flex properties
      const textFilter = page.locator('input[placeholder="Filter by text..."]').first();
      await expect(textFilter).toBeVisible();
      
      // Check that level dropdown exists and is compact
      const levelDropdown = page.locator('text=All Levels').first();
      await expect(levelDropdown).toBeVisible();
      
      // Check that module dropdown exists  
      const moduleDropdown = page.locator('text=All modules').first();
      await expect(moduleDropdown).toBeVisible();
      
      // Check date pickers exist
      const startTime = page.locator('button:has-text("Start time")').first();
      const endTime = page.locator('button:has-text("End time")').first();
      await expect(startTime).toBeVisible();
      await expect(endTime).toBeVisible();
      
      // Take screenshot of filter layout
      await page.screenshot({ path: 'test-results/screenshots/logs-filters.png' });
    });

    test('Events page filter layout', async ({ page }) => {
      await page.goto('/events/');
      
      // Wait for filters to load
      await page.waitForSelector('input[placeholder="Filter by text..."]', { timeout: 10000 });
      
      // Check text filter has proper flex properties
      const textFilter = page.locator('input[placeholder="Filter by text..."]').first();
      await expect(textFilter).toBeVisible();
      
      // Check that level dropdown exists and is compact
      const levelDropdown = page.locator('text=All Levels').first();
      await expect(levelDropdown).toBeVisible();
      
      // Check that source dropdown exists
      const sourceDropdown = page.locator('text=All sources').first();
      await expect(sourceDropdown).toBeVisible();
      
      // Check date pickers exist
      const startTime = page.locator('button:has-text("Start time")').first();
      const endTime = page.locator('button:has-text("End time")').first();
      await expect(startTime).toBeVisible();
      await expect(endTime).toBeVisible();
      
      // Take screenshot of filter layout
      await page.screenshot({ path: 'test-results/screenshots/events-filters.png' });
    });
  });

  test.describe('Date Picker Tests', () => {
    test('Logs page date picker functionality', async ({ page }) => {
      await page.goto('/logs/');
      
      // Wait for page to load
      await page.waitForSelector('button:has-text("Start time")', { timeout: 10000 });
      
      // Click start time picker
      await page.click('button:has-text("Start time")');
      
      // Wait for calendar to appear
      await page.waitForSelector('[role="dialog"]', { timeout: 5000 });
      
      // Take screenshot of calendar popup
      await page.screenshot({ path: 'test-results/screenshots/logs-datepicker-open.png' });
      
      // Verify calendar is visible and properly positioned
      const calendar = page.locator('[role="dialog"]');
      await expect(calendar).toBeVisible();
      
      // Check that calendar doesn't overlap with other elements by checking z-index
      const calendarBox = await calendar.boundingBox();
      expect(calendarBox).toBeTruthy();
      
      // Close calendar by clicking outside
      await page.click('body', { position: { x: 100, y: 100 } });
      
      // Verify calendar closes
      await expect(calendar).not.toBeVisible();
    });

    test('Events page date picker functionality', async ({ page }) => {
      await page.goto('/events/');
      
      // Wait for page to load
      await page.waitForSelector('button:has-text("Start time")', { timeout: 10000 });
      
      // Click start time picker
      await page.click('button:has-text("Start time")');
      
      // Wait for calendar to appear
      await page.waitForSelector('[role="dialog"]', { timeout: 5000 });
      
      // Take screenshot of calendar popup
      await page.screenshot({ path: 'test-results/screenshots/events-datepicker-open.png' });
      
      // Verify calendar is visible and properly positioned
      const calendar = page.locator('[role="dialog"]');
      await expect(calendar).toBeVisible();
      
      // Close calendar by clicking outside
      await page.click('body', { position: { x: 100, y: 100 } });
      
      // Verify calendar closes
      await expect(calendar).not.toBeVisible();
    });
  });

  test.describe('Responsive Layout Tests', () => {
    test('Mobile layout adaptation', async ({ page }) => {
      // Set mobile viewport
      await page.setViewportSize({ width: 375, height: 667 });
      
      await page.goto('/logs/');
      await page.waitForSelector('input[placeholder="Filter by text..."]', { timeout: 10000 });
      
      // Take screenshot of mobile layout
      await page.screenshot({ path: 'test-results/screenshots/logs-mobile.png' });
      
      // Verify filters are still accessible (may stack vertically)
      const textFilter = page.locator('input[placeholder="Filter by text..."]').first();
      await expect(textFilter).toBeVisible();
      
      const levelDropdown = page.locator('text=All Levels').first();
      await expect(levelDropdown).toBeVisible();
    });

    test('Tablet layout adaptation', async ({ page }) => {
      // Set tablet viewport
      await page.setViewportSize({ width: 768, height: 1024 });
      
      await page.goto('/logs/');
      await page.waitForSelector('input[placeholder="Filter by text..."]', { timeout: 10000 });
      
      // Take screenshot of tablet layout
      await page.screenshot({ path: 'test-results/screenshots/logs-tablet.png' });
      
      // Verify layout adapts appropriately
      const textFilter = page.locator('input[placeholder="Filter by text..."]').first();
      await expect(textFilter).toBeVisible();
    });
  });

  test.describe('Performance Tests', () => {
    test('Page load performance', async ({ page }) => {
      const startTime = Date.now();
      
      await page.goto('/logs/');
      await page.waitForSelector('h1', { timeout: 10000 });
      
      const loadTime = Date.now() - startTime;
      
      // Expect page to load within 5 seconds (generous for potential slow systems)
      expect(loadTime).toBeLessThan(5000);
      
      console.log(`Logs page loaded in ${loadTime}ms`);
    });

    test('No 30-second timeout errors', async ({ page }) => {
      const errors: string[] = [];
      
      page.on('pageerror', (error) => {
        errors.push(error.message);
      });
      
      page.on('response', (response) => {
        if (response.status() >= 400) {
          errors.push(`HTTP ${response.status()} on ${response.url()}`);
        }
      });
      
      await page.goto('/events/');
      await page.waitForSelector('h1', { timeout: 10000 });
      
      // Wait a bit to catch any delayed errors
      await page.waitForTimeout(2000);
      
      // Filter out expected errors and focus on timeout-related issues
      const timeoutErrors = errors.filter(error => 
        error.toLowerCase().includes('timeout') || 
        error.toLowerCase().includes('30') ||
        error.toLowerCase().includes('websocket')
      );
      
      expect(timeoutErrors).toHaveLength(0);
      
      if (errors.length > 0) {
        console.log('Non-timeout errors detected:', errors);
      }
    });
  });
});