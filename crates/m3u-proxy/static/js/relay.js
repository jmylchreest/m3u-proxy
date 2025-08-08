//! Relay Management JavaScript
//!
//! This file implements the frontend functionality for managing relay profiles,
//! monitoring active relays, and displaying system health.

// Global state
let relayProfiles = [];
let activeRelays = [];
let healthData = null;
let updateInterval = null;
let currentProfileId = null;

// API endpoints
const API_BASE = '/api/v1/relay';

// Initialize when page loads
document.addEventListener('DOMContentLoaded', function() {
    console.log('Relay page loaded, initializing...');
    loadRelayProfiles();
    loadActiveRelays();
    loadHealthData();
    loadHardwareAcceleration();
    
    // Set up periodic updates
    updateInterval = setInterval(() => {
        loadActiveRelays();
        loadHealthData();
    }, 5000);
});

// Load relay profiles from API
async function loadRelayProfiles() {
    try {
        console.log('Loading relay profiles...');
        const response = await fetch(`${API_BASE}/profiles`);
        if (response.ok) {
            relayProfiles = await response.json();
            console.log('Loaded relay profiles:', relayProfiles);
            renderRelayProfiles();
        } else {
            console.error('Failed to load relay profiles:', response.status);
            renderRelayProfiles(); // Render empty state
        }
    } catch (error) {
        console.error('Error loading relay profiles:', error);
        renderRelayProfiles(); // Render empty state
    }
}

// Load active relays from API
async function loadActiveRelays() {
    try {
        console.log('Loading active relays...');
        const response = await fetch('/api/v1/active-relays');
        if (response.ok) {
            const data = await response.json();
            console.log('Loaded active relays data:', data);
            const newActiveRelays = data.data?.active_processes || [];
            
            // Check if we need to re-render (relay list changed)
            const needsRerender = !arraysEqual(
                activeRelays.map(r => r.config_id),
                newActiveRelays.map(r => r.config_id)
            );
            
            activeRelays = newActiveRelays;
            
            if (needsRerender) {
                renderActiveRelays();
            } else {
                // Just update existing stats without re-rendering
                activeRelays.forEach(relay => {
                    updateRelayStatus(relay);
                });
            }
        } else {
            console.error('Failed to load active relays:', response.status);
            renderActiveRelays(); // Render empty state
        }
    } catch (error) {
        console.error('Error loading active relays:', error);
        renderActiveRelays(); // Render empty state
    }
}

// Helper function to compare arrays
function arraysEqual(a, b) {
    if (a.length !== b.length) return false;
    return a.every((val, i) => val === b[i]);
}

// Load system health data
async function loadHealthData() {
    try {
        const response = await fetch(`${API_BASE}/health`);
        if (response.ok) {
            healthData = await response.json();
            renderHealthData();
        } else {
            console.error('Failed to load health data:', response.status);
        }
    } catch (error) {
        console.error('Error loading health data:', error);
    }
}

// Load hardware acceleration info
async function loadHardwareAcceleration() {
    try {
        const response = await fetch(`${API_BASE}/hardware-acceleration`);
        if (response.ok) {
            const hwData = await response.json();
            renderHardwareInfo(hwData);
        } else {
            console.error('Failed to load hardware acceleration data:', response.status);
        }
    } catch (error) {
        console.error('Error loading hardware acceleration data:', error);
    }
}

// Render relay profiles
function renderRelayProfiles() {
    const container = document.getElementById('relay-profiles-list');
    if (!container) return;

    if (!relayProfiles || relayProfiles.length === 0) {
        container.innerHTML = '<p class="no-data">No relay profiles available</p>';
        return;
    }

    container.innerHTML = relayProfiles.map(profile => `
        <div class="relay-profile-item" data-profile-id="${profile.id}">
            <h3>${escapeHtml(profile.name)}</h3>
            <div class="profile-details">
                <p><strong>Video:</strong> ${escapeHtml(profile.video_codec)} ${profile.video_bitrate}k</p>
                <p><strong>Audio:</strong> ${escapeHtml(profile.audio_codec)} ${profile.audio_bitrate}k</p>
                <p><strong>Resolution:</strong> ${profile.resolution_width}x${profile.resolution_height}</p>
                <p><strong>HW Accel:</strong> ${profile.enable_hardware_acceleration ? 'Yes' : 'No'}</p>
            </div>
            <div class="profile-actions">
                <button onclick="editProfile('${profile.id}')" class="btn btn-secondary">Edit</button>
                <button onclick="deleteProfile('${profile.id}')" class="btn btn-danger">Delete</button>
            </div>
        </div>
    `).join('');
}

// Render active relays
function renderActiveRelays() {
    const container = document.getElementById('active-relays-list');
    if (!container) return;

    if (!activeRelays || activeRelays.length === 0) {
        container.innerHTML = '<p class="no-data">No active relays</p>';
        return;
    }

    container.innerHTML = activeRelays.map(relay => `
        <div class="relay-item" data-config-id="${relay.config_id}">
            <div class="relay-header">
                <h3>${escapeHtml(relay.channel_name || 'Unknown Channel')}</h3>
                <span class="status-badge status-${relay.status}">${relay.status}</span>
            </div>
            
            <div class="relay-stats">
                <div class="stat-group">
                    <span class="stat-label">CPU:</span>
                    <span class="stat-value" id="cpu-${relay.config_id}">${relay.cpu_usage_percent ? relay.cpu_usage_percent.toFixed(1) + '%' : 'N/A'}</span>
                </div>
                
                <div class="stat-group">
                    <span class="stat-label">Memory:</span>
                    <span class="stat-value" id="memory-${relay.config_id}">${relay.memory_usage_mb ? relay.memory_usage_mb.toFixed(1) + 'MB' : 'N/A'}</span>
                </div>
                
                <div class="stat-group">
                    <span class="stat-label">Clients:</span>
                    <span class="stat-value" id="clients-${relay.config_id}">${relay.connected_clients ? relay.connected_clients.length : 0}</span>
                </div>
                
                <div class="stat-group">
                    <span class="stat-label">Uptime:</span>
                    <span class="stat-value" id="uptime-${relay.config_id}">${formatDuration(relay.uptime_seconds)}</span>
                </div>
            </div>
            
            <div class="relay-controls">
                <button onclick="stopRelay('${relay.config_id}')" class="btn btn-danger btn-sm">Stop</button>
                <button onclick="restartRelay('${relay.config_id}')" class="btn btn-warning btn-sm">Restart</button>
            </div>
        </div>
    `).join('');
}

// Update relay status (for periodic updates)
function updateRelayStatus(relay) {
    // Update CPU usage
    const cpuSpan = document.getElementById(`cpu-${relay.config_id}`);
    if (cpuSpan) cpuSpan.textContent = relay.cpu_usage_percent ? relay.cpu_usage_percent.toFixed(1) + '%' : 'N/A';
    
    // Update memory usage
    const memorySpan = document.getElementById(`memory-${relay.config_id}`);
    if (memorySpan) memorySpan.textContent = relay.memory_usage_mb ? relay.memory_usage_mb.toFixed(1) + 'MB' : 'N/A';
    
    // Update client count
    const clientsSpan = document.getElementById(`clients-${relay.config_id}`);
    if (clientsSpan) clientsSpan.textContent = relay.connected_clients ? relay.connected_clients.length : 0;
    
    // Update uptime
    const uptimeSpan = document.getElementById(`uptime-${relay.config_id}`);
    if (uptimeSpan) uptimeSpan.textContent = formatDuration(relay.uptime_seconds);
}

// Render health data
function renderHealthData() {
    if (!healthData) return;

    const container = document.getElementById('system-health');
    if (!container) return;

    const processes = healthData.processes || [];
    const totalProcesses = processes.length;
    const healthyProcesses = processes.filter(p => p.status === 'healthy').length;

    container.innerHTML = `
        <div class="health-summary">
            <h3>System Health</h3>
            <div class="health-stats">
                <div class="health-stat">
                    <span class="stat-label">Total Processes:</span>
                    <span class="stat-value">${totalProcesses}</span>
                </div>
                <div class="health-stat">
                    <span class="stat-label">Healthy:</span>
                    <span class="stat-value">${healthyProcesses}</span>
                </div>
                <div class="health-stat">
                    <span class="stat-label">Unhealthy:</span>
                    <span class="stat-value">${totalProcesses - healthyProcesses}</span>
                </div>
            </div>
        </div>
        
        <div class="process-list">
            ${processes.map(process => `
                <div class="process-item status-${process.status}">
                    <div class="process-info">
                        <strong>${escapeHtml(process.channel_name || 'Unknown Channel')}</strong>
                        <span class="status-badge status-${process.status}">${process.status}</span>
                    </div>
                    <div class="process-stats">
                        <span>CPU: ${process.cpu_usage_percent ? process.cpu_usage_percent.toFixed(1) + '%' : 'N/A'}</span>
                        <span>Memory: ${process.memory_usage_mb ? process.memory_usage_mb.toFixed(1) + 'MB' : 'N/A'}</span>
                        <span>Clients: ${process.connected_clients ? process.connected_clients.length : 0}</span>
                    </div>
                </div>
            `).join('')}
        </div>
    `;
}

// Render hardware acceleration info
function renderHardwareInfo(hwData) {
    const container = document.getElementById('hardware-info');
    if (!container) return;

    container.innerHTML = `
        <div class="hardware-info">
            <h3>Hardware Acceleration</h3>
            <div class="hw-stats">
                <div class="hw-stat">
                    <span class="stat-label">Available:</span>
                    <span class="stat-value ${hwData.available ? 'status-healthy' : 'status-unhealthy'}">
                        ${hwData.available ? 'Yes' : 'No'}
                    </span>
                </div>
                ${hwData.available ? `
                    <div class="hw-stat">
                        <span class="stat-label">Capabilities:</span>
                        <span class="stat-value">${hwData.capabilities?.join(', ') || 'Unknown'}</span>
                    </div>
                ` : ''}
            </div>
        </div>
    `;
}

// Utility functions
function escapeHtml(text) {
    if (typeof text !== 'string') return '';
    const map = {
        '&': '&amp;',
        '<': '&lt;',
        '>': '&gt;',
        '"': '&quot;',
        "'": '&#039;'
    };
    return text.replace(/[&<>"']/g, function(m) { return map[m]; });
}

function formatDuration(seconds) {
    if (!seconds || seconds < 0) return '0s';
    
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const secs = seconds % 60;
    
    if (hours > 0) {
        return `${hours}h ${minutes}m ${secs}s`;
    } else if (minutes > 0) {
        return `${minutes}m ${secs}s`;
    } else {
        return `${secs}s`;
    }
}

// Profile management functions
async function editProfile(profileId) {
    // Implementation for editing profiles
    console.log('Edit profile:', profileId);
}

async function deleteProfile(profileId) {
    if (!confirm('Are you sure you want to delete this profile?')) return;
    
    try {
        const response = await fetch(`${API_BASE}/profiles/${profileId}`, {
            method: 'DELETE'
        });
        
        if (response.ok) {
            await loadRelayProfiles(); // Refresh the list
        } else {
            alert('Failed to delete profile');
        }
    } catch (error) {
        console.error('Error deleting profile:', error);
        alert('Error deleting profile');
    }
}

// Relay control functions
async function stopRelay(configId) {
    if (!confirm('Are you sure you want to stop this relay?')) return;
    
    try {
        const response = await fetch(`${API_BASE}/stop/${configId}`, {
            method: 'POST'
        });
        
        if (response.ok) {
            await loadActiveRelays(); // Refresh the list
        } else {
            alert('Failed to stop relay');
        }
    } catch (error) {
        console.error('Error stopping relay:', error);
        alert('Error stopping relay');
    }
}

async function restartRelay(configId) {
    if (!confirm('Are you sure you want to restart this relay?')) return;
    
    try {
        const response = await fetch(`${API_BASE}/restart/${configId}`, {
            method: 'POST'
        });
        
        if (response.ok) {
            await loadActiveRelays(); // Refresh the list
        } else {
            alert('Failed to restart relay');
        }
    } catch (error) {
        console.error('Error restarting relay:', error);
        alert('Error restarting relay');
    }
}