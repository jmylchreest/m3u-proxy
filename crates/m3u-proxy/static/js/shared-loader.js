// Shared Template Loader
class TemplateLoader {
    constructor() {
        this.loadedTemplates = new Map();
    }

    async loadTemplate(templatePath) {
        if (this.loadedTemplates.has(templatePath)) {
            return this.loadedTemplates.get(templatePath);
        }

        try {
            const response = await fetch(templatePath);
            if (!response.ok) {
                throw new Error(`Failed to load template: ${templatePath}`);
            }
            const html = await response.text();
            this.loadedTemplates.set(templatePath, html);
            return html;
        } catch (error) {
            console.error('Template loading error:', error);
            return '';
        }
    }

    async loadIntoElement(templatePath, elementId) {
        const html = await this.loadTemplate(templatePath);
        const element = document.getElementById(elementId);
        if (element) {
            element.innerHTML = html;
        } else {
            console.warn(`Element with id '${elementId}' not found`);
        }
    }

    async loadMultiple(templates) {
        const promises = templates.map(async ({ templatePath, elementId }) => {
            await this.loadIntoElement(templatePath, elementId);
        });
        await Promise.all(promises);
    }
}

// Global template loader instance
const templateLoader = new TemplateLoader();

// Convenience function to load common page templates
async function loadPageTemplates() {
    await templateLoader.loadMultiple([
        { templatePath: '/static/html/shared/header.html', elementId: 'headerContainer' },
        { templatePath: '/static/html/shared/nav.html', elementId: 'navContainer' }
    ]);
}

// Auto-load templates when DOM is ready
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', async () => {
        await loadPageTemplates();
        // Load theme toggle after templates are loaded
        const themeScript = document.createElement('script');
        themeScript.src = '/static/js/theme-toggle.js';
        document.head.appendChild(themeScript);
    });
} else {
    loadPageTemplates().then(() => {
        // Load theme toggle after templates are loaded
        const themeScript = document.createElement('script');
        themeScript.src = '/static/js/theme-toggle.js';
        document.head.appendChild(themeScript);
    });
}
