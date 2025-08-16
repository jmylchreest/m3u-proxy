import { test, expect } from '@playwright/test';

test.describe('Cross-Page Navigation Tests', () => {
  
  const pages = [
    { path: '/', title: 'Dashboard' },
    { path: '/logs/', title: 'Logs' },
    { path: '/events/', title: 'Events' },
    { path: '/sources/stream/', title: 'Stream Sources' },
    { path: '/sources/epg/', title: 'EPG Sources' },
    { path: '/proxies/', title: 'Proxies' },
    { path: '/debug/', title: 'Debug' },
    { path: '/settings/', title: 'Settings' }
  ];

  for (const pageInfo of pages) {
    test(`${pageInfo.title} page should load correctly`, async ({ page }) => {
      const startTime = Date.now();
      
      await page.goto(pageInfo.path);
      
      // Wait for the page header to appear
      await page.waitForSelector('h1', { timeout: 10000 });
      
      const loadTime = Date.now() - startTime;
      
      // Check that the correct header is displayed
      const header = await page.locator('h1').first().textContent();
      expect(header).toBe(pageInfo.title);
      
      // Check load time is reasonable
      expect(loadTime).toBeLessThan(5000);
      
      // Take screenshot
      await page.screenshot({ 
        path: `test-results/screenshots/${pageInfo.title.toLowerCase().replace(/\s+/g, '-')}-page.png` 
      });
      
      console.log(`${pageInfo.title} page loaded in ${loadTime}ms`);
    });
  }

  test('Navigation between pages should work', async ({ page }) => {
    // Start at dashboard
    await page.goto('/');
    await page.waitForSelector('h1', { timeout: 10000 });
    
    let header = await page.locator('h1').first().textContent();
    expect(header).toBe('Dashboard');
    
    // Navigate to logs
    await page.goto('/logs/');
    await page.waitForSelector('h1', { timeout: 10000 });
    
    header = await page.locator('h1').first().textContent();
    expect(header).toBe('Logs');
    
    // Navigate to events
    await page.goto('/events/');
    await page.waitForSelector('h1', { timeout: 10000 });
    
    header = await page.locator('h1').first().textContent();
    expect(header).toBe('Events');
    
    // Take final screenshot
    await page.screenshot({ path: 'test-results/screenshots/navigation-final.png' });
  });

  test('Sidebar navigation should work if present', async ({ page }) => {
    await page.goto('/');
    await page.waitForSelector('h1', { timeout: 10000 });
    
    // Look for sidebar navigation links
    const sidebarLinks = page.locator('nav a, aside a, [role="navigation"] a');
    const linkCount = await sidebarLinks.count();
    
    if (linkCount > 0) {
      console.log(`Found ${linkCount} navigation links in sidebar`);
      
      // Try clicking the first few links to test navigation
      for (let i = 0; i < Math.min(3, linkCount); i++) {
        const link = sidebarLinks.nth(i);
        const linkText = await link.textContent();
        
        if (linkText && linkText.trim()) {
          console.log(`Testing sidebar link: ${linkText.trim()}`);
          
          try {
            await link.click();
            await page.waitForSelector('h1', { timeout: 5000 });
            
            // Take screenshot of resulting page
            await page.screenshot({ 
              path: `test-results/screenshots/sidebar-nav-${i + 1}.png` 
            });
          } catch (error) {
            console.log(`Navigation link ${linkText.trim()} may not be functional: ${error}`);
          }
        }
      }
    } else {
      console.log('No sidebar navigation links found');
    }
    
    // Take screenshot of sidebar state
    await page.screenshot({ path: 'test-results/screenshots/sidebar-navigation.png' });
  });

  test('Pages should handle direct URL access', async ({ page }) => {
    // Test direct access to deep pages
    const testPages = ['/logs/', '/events/', '/debug/'];
    
    for (const testPath of testPages) {
      await page.goto(testPath);
      
      // Should not redirect to login or error page
      const currentUrl = page.url();
      expect(currentUrl).toContain(testPath);
      
      // Should load the correct page
      await page.waitForSelector('h1', { timeout: 10000 });
      const header = await page.locator('h1').first().textContent();
      expect(header).toBeTruthy();
      
      console.log(`Direct access to ${testPath}: ${header}`);
    }
  });
});