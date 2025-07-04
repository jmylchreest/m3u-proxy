// Theme Toggle JavaScript
class ThemeToggle {
    constructor() {
        this.button = document.getElementById('themeToggle');
        this.icon = this.button?.querySelector('.theme-icon');
        this.currentTheme = localStorage.getItem('theme') || 'light';
        
        this.init();
    }

    init() {
        if (!this.button) return;
        
        // Apply saved theme
        this.applyTheme(this.currentTheme);
        
        // Add click listener
        this.button.addEventListener('click', () => this.toggleTheme());
    }

    applyTheme(theme) {
        if (theme === 'dark') {
            document.body.classList.add('dark-theme');
            this.icon.textContent = '☀️';
            this.button.setAttribute('aria-label', 'Switch to light mode');
        } else {
            document.body.classList.remove('dark-theme');
            this.icon.textContent = '🌙';
            this.button.setAttribute('aria-label', 'Switch to dark mode');
        }
        
        this.currentTheme = theme;
        localStorage.setItem('theme', theme);
    }

    toggleTheme() {
        const newTheme = this.currentTheme === 'light' ? 'dark' : 'light';
        this.applyTheme(newTheme);
    }
}

// Initialize theme toggle when DOM is loaded
function initializeThemeToggle() {
    new ThemeToggle();
}

// Check if DOM is already loaded
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initializeThemeToggle);
} else {
    initializeThemeToggle();
}