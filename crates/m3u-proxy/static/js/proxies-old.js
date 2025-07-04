// Stream Proxies Management JavaScript

let currentProxies = [];
let editingProxy = null;
let previewData = null;
let availableSources = [];
let availableFilters = [];

// Initialize page
function initializeProxiesPage() {
    console.log('Initializing stream proxies page...');
    loadProxies();
    loadSources();
    loadFilters();
}

// Check if DOM is already loaded
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initializeProxiesPage);
} else {
    initializeProxiesPage();
}

// Load all stream proxies
async function loadProxies() {
    try {
        const response = await fetch('/api/proxies?' + new Date().getTime());
        console.log('Proxies API response status:', response.status);
        if (!response.ok) throw new Error('Failed to load proxies');

        const data = await response.json();
        console.log('Proxies API response:', data);
        currentProxies = Array.isArray(data) ? data : [];
        console.log('Current proxies count:', currentProxies.length);
        renderProxies();
    } catch (error) {
        console.error('Error loading proxies:', error);
        currentProxies = [];
        renderProxies();
        showError('Failed to load stream proxies');
    }
}

// Load sources for dropdown
async function loadSources() {
    try {
        const response = await fetch('/api/sources');
        if (!response.ok) throw new Error('Failed to load sources');
        
        availableSources = await response.json();
        populateSourcesDropdown();
    } catch (error) {
        console.error('Error loading sources:', error);
    }
}

// Load filters for checkboxes
async function loadFilters() {
    try {
        const response = await fetch('/api/filters');
        if (!response.ok) throw new Error('Failed to load filters');
        
        availableFilters = await response.json();
    } catch (error) {
        console.error('Error loading filters:', error);
    }
}

// Render proxies list
function renderProxies() {
    const container = document.getElementById('proxiesContainer');

    if (currentProxies.length === 0) {
        container.innerHTML = `
            <div class="empty-state">
                <h3>No Stream Proxies</h3>
                <p>Create your first proxy to generate M3U playlists</p>
                <button class="btn btn-primary" onclick="createProxy()">
                    ➕ Create Your First Proxy
                </button>
            </div>
        `;
        return;
    }

    let html = '<div class="proxies-grid">';
    
    currentProxies.forEach(proxy => {
        const lastGenerated = proxy.last_generated 
            ? new Date(proxy.last_generated).toLocaleDateString()
            : 'Never';
        
        const status = proxy.is_active ? 'Active' : 'Inactive';
        const statusClass = proxy.is_active ? 'success' : 'secondary';

        html += `
            <div class="proxy-card" data-proxy-id="${proxy.id}">
                <div class="proxy-card-header">
                    <h4 class="proxy-name">${escapeHtml(proxy.name)}</h4>
                    <span class="badge badge-${statusClass}">${status}</span>
                </div>
                <div class="proxy-card-body">
                    ${proxy.description ? `<p class="proxy-description">${escapeHtml(proxy.description)}</p>` : ''}
                    <div class="proxy-meta">
                        <small class="text-muted">
                            <strong>Source:</strong> ${proxy.source_name || 'Unknown'}<br>
                            <strong>Filters:</strong> ${proxy.filter_count || 0}<br>
                            <strong>Last Generated:</strong> ${lastGenerated}
                        </small>
                    </div>
                </div>
                <div class="proxy-card-actions">
                    <button class="btn btn-outline-primary btn-sm" onclick="previewProxy('${proxy.id}')">
                        👁️ Preview
                    </button>
                    <button class="btn btn-outline-secondary btn-sm" onclick="editProxy('${proxy.id}')">
                        ✏️ Edit
                    </button>
                    <button class="btn btn-outline-success btn-sm" onclick="regenerateProxy('${proxy.id}')">
                        🔄 Regenerate
                    </button>
                    <button class="btn btn-outline-danger btn-sm" onclick="deleteProxy('${proxy.id}')">
                        🗑️ Delete
                    </button>
                </div>
            </div>
        `;
    });

    html += '</div>';
    container.innerHTML = html;
}

// Create new proxy
function createProxy() {
    editingProxy = null;
    document.getElementById('proxyModalTitle').textContent = 'Create Stream Proxy';
    clearProxyForm();
    showModal('proxyModal');
}

// Edit existing proxy
function editProxy(proxyId) {
    const proxy = currentProxies.find(p => p.id === proxyId);
    if (!proxy) return;

    editingProxy = proxy;
    document.getElementById('proxyModalTitle').textContent = 'Edit Stream Proxy';
    populateProxyForm(proxy);
    showModal('proxyModal');
}

// Regenerate proxy
async function regenerateProxy(proxyId) {
    try {
        showInfo('Regenerating proxy...');
        
        const response = await fetch(`/api/proxies/${proxyId}/regenerate`, {
            method: 'POST'
        });

        if (!response.ok) throw new Error('Failed to regenerate proxy');

        const result = await response.json();
        showSuccess(`Proxy regenerated successfully. Generated ${result.channel_count} channels.`);
        
        // Reload proxies to get updated info
        loadProxies();
    } catch (error) {
        console.error('Error regenerating proxy:', error);
        showError('Failed to regenerate proxy');
    }
}

// Regenerate all proxies
async function regenerateAllProxies() {
    if (!confirm('Are you sure you want to regenerate all active proxies?')) {
        return;
    }

    try {
        showInfo('Regenerating all proxies...');
        
        const response = await fetch('/api/proxies/regenerate-all', {
            method: 'POST'
        });

        if (!response.ok) throw new Error('Failed to regenerate proxies');

        const result = await response.json();
        showSuccess(`Regenerated ${result.count} proxies successfully.`);
        
        // Reload proxies to get updated info
        loadProxies();
    } catch (error) {
        console.error('Error regenerating all proxies:', error);
        showError('Failed to regenerate proxies');
    }
}

    createProxy() {
        this.editingProxy = null;
        this.showModal('Create Stream Proxy');
        this.populateSourcesSelection();
        this.populateFiltersSelection();
        this.clearForm();
    }

    editProxy(proxyId) {
        // TODO: Implement edit proxy functionality
        this.showAlert('info', 'Edit proxy functionality coming soon');
    }

    async deleteProxy(proxyId, proxyName) {
        if (!confirm(`Delete proxy "${proxyName}"?\\n\\nThis action cannot be undone.`)) {
            return;
        }

        try {
            const response = await fetch(`/api/proxies/${proxyId}`, {
                method: 'DELETE',
            });

            if (response.ok) {
                this.showAlert('success', `Proxy "${proxyName}" deleted successfully`);
                await this.loadProxies();
            } else {
                this.showAlert('error', 'Failed to delete proxy');
            }
        } catch (error) {
            console.error('Error deleting proxy:', error);
            this.showAlert('error', 'Failed to delete proxy');
        }
    }

    previewProxy(proxyId) {
        // TODO: Implement preview functionality
        this.showAlert('info', 'Preview functionality coming soon');
    }

    showModal(title) {
        document.getElementById('modalTitle').textContent = title;
        document.getElementById('proxyModal').style.display = 'block';
    }

    hideModal() {
        document.getElementById('proxyModal').style.display = 'none';
    }

    hidePreviewModal() {
        document.getElementById('previewModal').style.display = 'none';
    }

    populateSourcesSelection() {
        const container = document.getElementById('sourcesSelection');
        container.innerHTML = this.sources.map(source => `
            <div class="form-check">
                <input type="checkbox" id="source_${source.id}" class="form-check-input" value="${source.id}">
                <label for="source_${source.id}" class="form-label">
                    ${source.name} 
                    <small class="text-muted">(${source.source_type.toUpperCase()})</small>
                </label>
            </div>
        `).join('');
    }

    populateFiltersSelection() {
        const select = document.getElementById('availableFilters');
        select.innerHTML = '<option value="">Select a filter to add...</option>' + 
            this.filters.map(filter => `<option value="${filter.id}">${filter.name}</option>`).join('');
    }

    toggleAddFilterBtn() {
        const select = document.getElementById('availableFilters');
        const button = document.getElementById('addFilterBtn');
        button.disabled = !select.value;
    }

    addFilter() {
        // TODO: Implement add filter functionality
        this.showAlert('info', 'Add filter functionality coming soon');
    }

    clearForm() {
        document.getElementById('proxyForm').reset();
        document.getElementById('selectedFilters').innerHTML = '';
        this.toggleAddFilterBtn();
    }

    async saveProxy() {
        // TODO: Implement save proxy functionality
        this.showAlert('info', 'Save proxy functionality coming soon');
    }

    copyUrl() {
        const urlInput = document.getElementById('proxyUrl');
        urlInput.select();
        urlInput.setSelectionRange(0, 99999); // For mobile devices
        document.execCommand('copy');
        this.showAlert('success', 'URL copied to clipboard');
    }

    generateProxy() {
        // TODO: Implement generate proxy functionality
        this.showAlert('info', 'Generate proxy functionality coming soon');
    }

    showAlert(type, message) {
        const alertsContainer = document.getElementById('alertsContainer');
        const alertId = 'alert-' + Date.now();
        
        const alertHtml = `
            <div id="${alertId}" class="alert alert-${type} alert-dismissible fade show" role="alert">
                ${message}
                <button type="button" class="btn-close" data-bs-dismiss="alert" aria-label="Close"></button>
            </div>
        `;
        
        alertsContainer.insertAdjacentHTML('beforeend', alertHtml);
        
        // Auto-dismiss after 5 seconds
        setTimeout(() => {
            const alertElement = document.getElementById(alertId);
            if (alertElement) {
                alertElement.remove();
            }
        }, 5000);
    }
}

// Initialize the proxies manager when the DOM is loaded
let proxiesManager;

function initializeProxiesPage() {
    console.log('Initializing proxies page...');
    proxiesManager = new ProxiesManager();
}

// Check if DOM is already loaded, if so initialize immediately
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initializeProxiesPage);
} else {
    initializeProxiesPage();
}