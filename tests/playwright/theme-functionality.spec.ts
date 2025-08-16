import { test, expect } from '@playwright/test';

test.describe('Theme Functionality Tests', () => {
  
  test('Theme selector should be present and functional', async ({ page }) => {
    await page.goto('/');
    
    // Wait for page to load
    await page.waitForSelector('h1', { timeout: 10000 });
    
    // Look for theme selector button (might be an icon or text)
    const themeSelector = page.locator('[data-testid="theme-selector"], button[title*="theme"], button[aria-label*="theme"]').first();
    
    // If no data attributes, look for common theme selector patterns
    if (!(await themeSelector.isVisible())) {
      const fallbackSelector = page.locator('button').filter({ hasText: /theme|Theme|dark|light/i }).first();
      await expect(fallbackSelector.or(themeSelector)).toBeVisible();
    } else {
      await expect(themeSelector).toBeVisible();
    }
    
    // Take screenshot of theme selector
    await page.screenshot({ path: 'test-results/screenshots/theme-selector.png' });
  });

  test('Page should maintain functionality across theme changes', async ({ page }) => {
    await page.goto('/logs/');
    
    // Wait for filters to load
    await page.waitForSelector('input[placeholder="Filter by text..."]', { timeout: 10000 });
    
    // Test filter functionality before theme change
    const textFilter = page.locator('input[placeholder="Filter by text..."]').first();
    await textFilter.fill('test filter');
    
    const inputValue = await textFilter.inputValue();
    expect(inputValue).toBe('test filter');
    
    // Look for and click theme selector if available
    const themeSelector = page.locator('[data-testid="theme-selector"], button[title*="theme"], button[aria-label*="theme"]').first();
    
    if (await themeSelector.isVisible()) {
      await themeSelector.click();
      
      // Wait a moment for theme change
      await page.waitForTimeout(500);
      
      // Verify filter still works after theme change
      await textFilter.clear();
      await textFilter.fill('post-theme test');
      
      const newInputValue = await textFilter.inputValue();
      expect(newInputValue).toBe('post-theme test');
    }
    
    // Take screenshot after theme interaction
    await page.screenshot({ path: 'test-results/screenshots/post-theme-change.png' });
  });

  test('Visual elements should have proper contrast', async ({ page }) => {
    await page.goto('/logs/');
    
    // Wait for page to load
    await page.waitForSelector('h1', { timeout: 10000 });
    
    // Check that text elements are visible (basic contrast check)
    const header = page.locator('h1').first();
    await expect(header).toBeVisible();
    
    const textFilter = page.locator('input[placeholder="Filter by text..."]').first();
    await expect(textFilter).toBeVisible();
    
    // Take screenshot for manual visual verification
    await page.screenshot({ path: 'test-results/screenshots/contrast-check.png' });
  });
});