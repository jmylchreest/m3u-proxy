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
let relayCharts = new Map(); // Store chart instances
let relayHistory = new Map(); // Store historical data for each relay

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
                // Just update existing charts and stats without re-rendering
                activeRelays.forEach(relay => {
                    updateRelayStats(relay);
                    updateRelayHistory(relay);
                    updateCharts(relay);
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


// Render relay profiles
function renderRelayProfiles() {
    const container = document.getElementById('relay-profiles');
    
    if (relayProfiles.length === 0) {
        container.innerHTML = `
            <div class="empty-state">
                <h3>No Relay Profiles</h3>
                <p>Create relay profiles to configure FFmpeg settings for stream transcoding and optimization.</p>
                <button class="btn btn-primary" onclick="showCreateProfileModal()">
                    Create Your First Profile
                </button>
            </div>
        `;
        return;
    }

    const profilesHtml = relayProfiles.map(profile => `
        <div class="profile-card" data-profile-id="${profile.id}">
            <div class="profile-header">
                <h4>${escapeHtml(profile.name)}</h4>
                <div class="profile-badges">
                    ${profile.is_system_default ? '<span class="badge badge-info">System Default</span>' : ''}
                    ${profile.video_codec ? `<span class="badge badge-primary">${profile.video_codec.toUpperCase()}</span>` : ''}
                    ${profile.audio_codec ? `<span class="badge badge-primary">${profile.audio_codec.toUpperCase()}</span>` : ''}
                    ${profile.output_format === 'hls' ? '<span class="badge badge-success">HLS</span>' : ''}
                    ${profile.output_format === 'transport_stream' ? '<span class="badge badge-success">TS</span>' : ''}
                    ${profile.enable_hardware_acceleration ? '<span class="badge badge-warning">HW Accel</span>' : ''}
                </div>
            </div>
            <div class="profile-body">
                <p class="profile-description">${escapeHtml(profile.description || 'No description')}</p>
                <div class="profile-details">
                    <div class="detail-item">
                        <label>Video Codec:</label>
                        <span>${profile.video_codec?.toUpperCase() || 'N/A'}</span>
                    </div>
                    <div class="detail-item">
                        <label>Audio Codec:</label>
                        <span>${profile.audio_codec?.toUpperCase() || 'N/A'}</span>
                    </div>
                    <div class="detail-item">
                        <label>Output Format:</label>
                        <span>${profile.output_format?.replace('_', ' ').toUpperCase() || 'N/A'}</span>
                    </div>
                    ${profile.video_bitrate ? `
                        <div class="detail-item">
                            <label>Video Bitrate:</label>
                            <span>${profile.video_bitrate}k</span>
                        </div>
                    ` : ''}
                    ${profile.audio_bitrate ? `
                        <div class="detail-item">
                            <label>Audio Bitrate:</label>
                            <span>${profile.audio_bitrate}k</span>
                        </div>
                    ` : ''}
                    ${profile.segment_duration ? `
                        <div class="detail-item">
                            <label>Segment Duration:</label>
                            <span>${profile.segment_duration}s</span>
                        </div>
                    ` : ''}
                </div>
                <div class="profile-actions">
                    <button class="btn btn-sm btn-secondary" onclick="editProfile('${profile.id}')">
                        ${profile.is_system_default ? 'View' : 'Edit'}
                    </button>
                    ${!profile.is_system_default ? `
                        <button class="btn btn-sm btn-danger" onclick="deleteProfile('${profile.id}')">
                            Delete
                        </button>
                    ` : ''}
                </div>
            </div>
        </div>
    `).join('');

    container.innerHTML = profilesHtml;
}

// Render active relays
function renderActiveRelays() {
    const container = document.getElementById('active-relays');
    
    if (activeRelays.length === 0) {
        container.innerHTML = `
            <div class="empty-state">
                <h3>No Active Relays</h3>
                <p>No relay processes are currently running. Relays will appear here when stream proxies with relay profiles are accessed by clients.</p>
            </div>
        `;
        return;
    }

    const relaysHtml = activeRelays.map(relay => `
        <div class="relay-card" data-config-id="${relay.config_id}">
            <div class="relay-header">
                <h4>${escapeHtml(relay.profile_name)} - ${escapeHtml(relay.channel_name || 'Unknown Channel')}</h4>
                <div class="relay-status">
                    <span class="status-indicator ${relay.is_running ? 'running' : 'stopped'}"></span>
                    <span class="status-text">${relay.is_running ? 'Running' : 'Stopped'}</span>
                </div>
            </div>
            <div class="relay-body">
                <!-- Current Stats -->
                <div class="relay-stats">
                    <div class="stat-item">
                        <label>Config ID:</label>
                        <span class="monospace">${relay.config_id}</span>
                    </div>
                    <div class="stat-item">
                        <label>Active Clients:</label>
                        <span class="highlight">${relay.client_count}</span>
                    </div>
                    <div class="stat-item">
                        <label>Data Served:</label>
                        <span>${formatBytes(relay.bytes_delivered_downstream || 0)}</span>
                    </div>
                    <div class="stat-item">
                        <label>CPU Usage:</label>
                        <span>${relay.cpu_usage_percent ? relay.cpu_usage_percent.toFixed(1) + '%' : 'N/A'}</span>
                    </div>
                    <div class="stat-item">
                        <label>Memory:</label>
                        <span>${relay.memory_usage_mb ? relay.memory_usage_mb.toFixed(0) + ' MB' : 'N/A'}</span>
                    </div>
                    ${relay.uptime_seconds ? `
                        <div class="stat-item">
                            <label>Uptime:</label>
                            <span>${formatDuration(relay.uptime_seconds)}</span>
                        </div>
                    ` : ''}
                    ${relay.last_heartbeat ? `
                        <div class="stat-item">
                            <label>Last Heartbeat:</label>
                            <span>${formatDateTime(relay.last_heartbeat)}</span>
                        </div>
                    ` : ''}
                </div>
                
                <!-- Performance Graphs -->
                <div class="relay-graphs">
                    <div class="graph-section">
                        <h5>CPU Usage (%)</h5>
                        <div class="chart-container">
                            <canvas id="cpu-chart-${relay.config_id}" width="400" height="200"></canvas>
                        </div>
                    </div>
                    <div class="graph-section">
                        <h5>Memory Usage (MB)</h5>
                        <div class="chart-container">
                            <canvas id="memory-chart-${relay.config_id}" width="400" height="200"></canvas>
                        </div>
                    </div>
                    <div class="graph-section">
                        <h5>Network Traffic (MB/s)</h5>
                        <div class="chart-container">
                            <canvas id="traffic-chart-${relay.config_id}" width="400" height="200"></canvas>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    `).join('');

    container.innerHTML = relaysHtml;
    
    // Create charts for each relay after DOM is updated
    setTimeout(() => {
        activeRelays.forEach(relay => {
            updateRelayHistory(relay);
            createRelayCharts(relay);
        });
    }, 100);
}

// Render health data
function renderHealthData() {
    if (!healthData) return;
    
    document.getElementById('cpu-usage').textContent = `${healthData.system_load.toFixed(2)}%`;
    document.getElementById('memory-usage').textContent = `${(healthData.memory_usage_mb / 1024).toFixed(1)} GB`;
    
    // Calculate total active processes
    const totalProcesses = activeRelays.length;
    document.getElementById('total-bandwidth').textContent = totalProcesses;
}

// Update relay stats without re-rendering
function updateRelayStats(relay) {
    const configId = relay.config_id;
    const card = document.querySelector(`[data-config-id="${configId}"]`);
    if (!card) return;
    
    // Update individual stat values
    const clientCountSpan = card.querySelector('.stat-item:nth-child(2) span.highlight');
    if (clientCountSpan) clientCountSpan.textContent = relay.client_count;
    
    const dataServedSpan = card.querySelector('.stat-item:nth-child(3) span');
    if (dataServedSpan) dataServedSpan.textContent = formatBytes(relay.bytes_delivered_downstream || 0);
    
    const cpuUsageSpan = card.querySelector('.stat-item:nth-child(4) span');
    if (cpuUsageSpan) cpuUsageSpan.textContent = relay.cpu_usage_percent ? relay.cpu_usage_percent.toFixed(1) + '%' : 'N/A';
    
    const memorySpan = card.querySelector('.stat-item:nth-child(5) span');
    if (memorySpan) memorySpan.textContent = relay.memory_usage_mb ? relay.memory_usage_mb.toFixed(0) + ' MB' : 'N/A';
    
    const uptimeSpan = card.querySelector('.stat-item:nth-child(6) span');
    if (uptimeSpan && relay.uptime_seconds) uptimeSpan.textContent = formatDuration(relay.uptime_seconds);
    
    const heartbeatSpan = card.querySelector('.stat-item:nth-child(7) span');
    if (heartbeatSpan && relay.last_heartbeat) heartbeatSpan.textContent = formatDateTime(relay.last_heartbeat);
}

// Chart creation and management functions
function createRelayCharts(relay) {
    const configId = relay.config_id;
    const chartKey = `charts-${configId}`;
    
    // Skip if charts already exist for this relay
    if (relayCharts.has(chartKey)) {
        updateCharts(relay);
        return;
    }
    
    const charts = {};
    
    // CPU Usage Chart
    const cpuCanvas = document.getElementById(`cpu-chart-${configId}`);
    if (cpuCanvas) {
        charts.cpu = new Chart(cpuCanvas, {
            type: 'line',
            data: {
                labels: [],
                datasets: [{
                    label: 'CPU Usage (%)',
                    data: [],
                    borderColor: '#ff6b6b',
                    backgroundColor: 'rgba(255, 107, 107, 0.1)',
                    fill: true,
                    tension: 0.4
                }]
            },
            options: getChartOptions('CPU Usage (%)', 0, 400)
        });
    }
    
    // Memory Usage Chart
    const memoryCanvas = document.getElementById(`memory-chart-${configId}`);
    if (memoryCanvas) {
        charts.memory = new Chart(memoryCanvas, {
            type: 'line',
            data: {
                labels: [],
                datasets: [{
                    label: 'Memory Usage (MB)',
                    data: [],
                    borderColor: '#4ecdc4',
                    backgroundColor: 'rgba(78, 205, 196, 0.1)',
                    fill: true,
                    tension: 0.4
                }]
            },
            options: getChartOptions('Memory Usage (MB)', 0, null)
        });
    }
    
    // Network Traffic Chart
    const trafficCanvas = document.getElementById(`traffic-chart-${configId}`);
    if (trafficCanvas) {
        charts.traffic = new Chart(trafficCanvas, {
            type: 'line',
            data: {
                labels: [],
                datasets: [{
                    label: 'Bytes In (MB/s)',
                    data: [],
                    borderColor: '#45b7d1',
                    backgroundColor: 'rgba(69, 183, 209, 0.1)',
                    fill: false,
                    tension: 0.4
                }, {
                    label: 'Bytes Out (MB/s)',
                    data: [],
                    borderColor: '#f39c12',
                    backgroundColor: 'rgba(243, 156, 18, 0.1)',
                    fill: false,
                    tension: 0.4
                }]
            },
            options: getChartOptions('Network Traffic (MB/s)', 0, null)
        });
    }
    
    relayCharts.set(chartKey, charts);
}

function getChartOptions(title, minY = 0, maxY = null) {
    return {
        responsive: true,
        maintainAspectRatio: false,
        scales: {
            x: {
                display: true,
                title: {
                    display: true,
                    text: 'Time'
                },
                grid: {
                    color: 'rgba(0, 0, 0, 0.1)'
                }
            },
            y: {
                display: true,
                title: {
                    display: true,
                    text: title
                },
                min: minY,
                max: maxY,
                grid: {
                    color: 'rgba(0, 0, 0, 0.1)'
                }
            }
        },
        plugins: {
            legend: {
                display: true,
                position: 'top'
            }
        },
        elements: {
            point: {
                radius: 2,
                hoverRadius: 4
            }
        }
    };
}

function updateRelayHistory(relay) {
    const configId = relay.config_id;
    const now = new Date();
    const timeLabel = now.toLocaleTimeString();
    
    // Initialize history if not exists
    if (!relayHistory.has(configId)) {
        relayHistory.set(configId, {
            timestamps: [],
            cpu: [],
            memory: [],
            bytesIn: [],
            bytesOut: [],
            lastBytesReceived: 0,
            lastBytesDelivered: 0,
            lastUpdate: now
        });
    }
    
    const history = relayHistory.get(configId);
    const timeDiff = (now - history.lastUpdate) / 1000; // seconds
    
    // Calculate rates (bytes per second)
    let bytesInRate = 0;
    let bytesOutRate = 0;
    
    if (history.lastBytesReceived > 0 && timeDiff > 0) {
        bytesInRate = (relay.bytes_received_upstream - history.lastBytesReceived) / timeDiff;
        bytesOutRate = (relay.bytes_delivered_downstream - history.lastBytesDelivered) / timeDiff;
    }
    
    // Add new data point
    history.timestamps.push(timeLabel);
    history.cpu.push(relay.cpu_usage_percent || 0);
    history.memory.push(relay.memory_usage_mb || 0);
    history.bytesIn.push(bytesInRate / (1024 * 1024)); // Convert to MB/s
    history.bytesOut.push(bytesOutRate / (1024 * 1024)); // Convert to MB/s
    
    // Update last values
    history.lastBytesReceived = relay.bytes_received_upstream || 0;
    history.lastBytesDelivered = relay.bytes_delivered_downstream || 0;
    history.lastUpdate = now;
    
    // Keep only last 20 data points
    const maxPoints = 20;
    if (history.timestamps.length > maxPoints) {
        history.timestamps.shift();
        history.cpu.shift();
        history.memory.shift();
        history.bytesIn.shift();
        history.bytesOut.shift();
    }
    
    relayHistory.set(configId, history);
}

function updateCharts(relay) {
    const configId = relay.config_id;
    const chartKey = `charts-${configId}`;
    const charts = relayCharts.get(chartKey);
    const history = relayHistory.get(configId);
    
    if (!charts || !history) return;
    
    // Update CPU chart
    if (charts.cpu) {
        charts.cpu.data.labels = [...history.timestamps];
        charts.cpu.data.datasets[0].data = [...history.cpu];
        charts.cpu.update('none');
    }
    
    // Update Memory chart
    if (charts.memory) {
        charts.memory.data.labels = [...history.timestamps];
        charts.memory.data.datasets[0].data = [...history.memory];
        charts.memory.update('none');
    }
    
    // Update Traffic chart
    if (charts.traffic) {
        charts.traffic.data.labels = [...history.timestamps];
        charts.traffic.data.datasets[0].data = [...history.bytesIn];
        charts.traffic.data.datasets[1].data = [...history.bytesOut];
        charts.traffic.update('none');
    }
}


// Profile management functions
function showCreateProfileModal() {
    currentProfileId = null;
    setupProfileModal('create');
}

function showEditProfileModal(profileId) {
    currentProfileId = profileId;
    setupProfileModal('edit');
}

function hideProfileModal() {
    const modal = document.getElementById('profileModal');
    if (modal) {
        modal.classList.remove('show');
        SharedUtils.handleModalClose();
    }
    resetProfileForm();
}

function setupProfileModal(mode) {
    const modal = document.getElementById('profileModal');
    const modalTitle = document.getElementById('modalTitle');
    const saveButton = document.getElementById('saveProfileButton');
    
    if (mode === 'create') {
        modalTitle.textContent = 'Create Relay Profile';
        saveButton.textContent = 'Create Profile';
        saveButton.onclick = createProfile;
        resetProfileForm();
    } else if (mode === 'edit') {
        const profile = relayProfiles.find(p => p.id === currentProfileId);
        if (!profile) {
            showAlert('Profile not found', 'error');
            return;
        }
        
        modalTitle.textContent = profile.is_system_default ? 'View System Profile' : 'Edit Relay Profile';
        saveButton.textContent = profile.is_system_default ? 'Close' : 'Save Changes';
        saveButton.onclick = profile.is_system_default ? hideProfileModal : saveProfile;
        
        populateProfileForm(profile);
    }
    
    modal.classList.add('show');
    SharedUtils.setupStandardModalCloseHandlers(modal, hideProfileModal);
    updateFFmpegPreview();
}

function resetProfileForm() {
    document.getElementById('profileName').value = '';
    document.getElementById('profileDescription').value = '';
    document.getElementById('videoCodec').value = 'h264';
    document.getElementById('audioCodec').value = 'aac';
    document.getElementById('videoProfile').value = 'main';
    document.getElementById('videoPreset').value = 'fast';
    document.getElementById('videoBitrate').value = '2000';
    document.getElementById('audioBitrate').value = '128';
    document.getElementById('enableHardwareAcceleration').checked = false;
    document.getElementById('preferredHwaccel').value = 'auto';
    document.getElementById('manualArgs').value = '';
    document.getElementById('outputFormat').value = 'transport_stream';
    document.getElementById('segmentDuration').value = '30';
    document.getElementById('maxSegments').value = '10';
    document.getElementById('inputTimeout').value = '30';
    
    // Reset form validation
    document.querySelectorAll('.form-control').forEach(field => {
        field.classList.remove('is-valid', 'is-invalid');
    });
    
    updateFFmpegPreview();
}

function populateProfileForm(profile) {
    document.getElementById('profileName').value = profile.name || '';
    document.getElementById('profileDescription').value = profile.description || '';
    document.getElementById('videoCodec').value = profile.video_codec?.toLowerCase() || 'h264';
    document.getElementById('audioCodec').value = profile.audio_codec?.toLowerCase() || 'aac';
    document.getElementById('videoProfile').value = profile.video_profile || 'main';
    document.getElementById('videoPreset').value = profile.video_preset || 'fast';
    document.getElementById('videoBitrate').value = profile.video_bitrate || '2000';
    document.getElementById('audioBitrate').value = profile.audio_bitrate || '128';
    document.getElementById('enableHardwareAcceleration').checked = profile.enable_hardware_acceleration || false;
    document.getElementById('preferredHwaccel').value = profile.preferred_hwaccel || 'auto';
    document.getElementById('manualArgs').value = profile.manual_args || '';
    document.getElementById('outputFormat').value = profile.output_format?.toLowerCase() || 'transport_stream';
    document.getElementById('segmentDuration').value = profile.segment_duration || '30';
    document.getElementById('maxSegments').value = profile.max_segments || '10';
    document.getElementById('inputTimeout').value = profile.input_timeout || '30';
    
    // Set read-only for system defaults
    const isSystemDefault = profile.is_system_default;
    document.querySelectorAll('.form-control, .form-check-input').forEach(field => {
        field.disabled = isSystemDefault;
    });
    
    if (isSystemDefault) {
        document.getElementById('systemDefaultNotice').style.display = 'block';
    } else {
        document.getElementById('systemDefaultNotice').style.display = 'none';
    }
    
    updateFFmpegPreview();
}

async function createProfile() {
    if (!validateProfileForm()) {
        return;
    }

    const profileData = getProfileDataFromForm();
    
    try {
        const response = await fetch(`${API_BASE}/profiles`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify(profileData)
        });

        if (response.ok) {
            hideProfileModal();
            loadRelayProfiles();
            showAlert('Profile created successfully', 'success');
        } else {
            const error = await response.text();
            showAlert(`Failed to create profile: ${error}`, 'error');
        }
    } catch (error) {
        showAlert(`Error creating profile: ${error.message}`, 'error');
    }
}

async function saveProfile() {
    if (!validateProfileForm()) {
        return;
    }

    const profileData = getProfileDataFromForm();
    
    try {
        const response = await fetch(`${API_BASE}/profiles/${currentProfileId}`, {
            method: 'PUT',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify(profileData)
        });

        if (response.ok) {
            hideProfileModal();
            loadRelayProfiles();
            showAlert('Profile updated successfully', 'success');
        } else {
            const error = await response.text();
            showAlert(`Failed to update profile: ${error}`, 'error');
        }
    } catch (error) {
        showAlert(`Error updating profile: ${error.message}`, 'error');
    }
}

function getProfileDataFromForm() {
    return {
        name: document.getElementById('profileName').value,
        description: document.getElementById('profileDescription').value,
        video_codec: document.getElementById('videoCodec').value,
        audio_codec: document.getElementById('audioCodec').value,
        video_profile: document.getElementById('videoProfile').value || null,
        video_preset: document.getElementById('videoPreset').value || null,
        video_bitrate: parseInt(document.getElementById('videoBitrate').value) || null,
        audio_bitrate: parseInt(document.getElementById('audioBitrate').value) || null,
        enable_hardware_acceleration: document.getElementById('enableHardwareAcceleration').checked,
        preferred_hwaccel: document.getElementById('preferredHwaccel').value || null,
        manual_args: document.getElementById('manualArgs').value || null,
        output_format: document.getElementById('outputFormat').value,
        segment_duration: parseInt(document.getElementById('segmentDuration').value) || null,
        max_segments: parseInt(document.getElementById('maxSegments').value) || null,
        input_timeout: parseInt(document.getElementById('inputTimeout').value) || 30,
        is_system_default: false,
        is_active: true
    };
}

function validateProfileForm() {
    const name = document.getElementById('profileName').value.trim();
    if (!name) {
        showAlert('Profile name is required', 'error');
        document.getElementById('profileName').focus();
        return false;
    }
    
    const videoBitrate = document.getElementById('videoBitrate').value;
    if (videoBitrate && (isNaN(videoBitrate) || parseInt(videoBitrate) <= 0)) {
        showAlert('Video bitrate must be a positive number', 'error');
        document.getElementById('videoBitrate').focus();
        return false;
    }
    
    const audioBitrate = document.getElementById('audioBitrate').value;
    if (audioBitrate && (isNaN(audioBitrate) || parseInt(audioBitrate) <= 0)) {
        showAlert('Audio bitrate must be a positive number', 'error');
        document.getElementById('audioBitrate').focus();
        return false;
    }
    
    return true;
}

function updateFFmpegPreview() {
    const ffmpegPreview = document.getElementById('ffmpegPreview');
    if (!ffmpegPreview) return;
    
    const data = getProfileDataFromForm();
    const command = generateFFmpegCommand(data);
    
    ffmpegPreview.innerHTML = `<pre><code>${escapeHtml(command)}</code></pre>`;
}

function generateFFmpegCommand(data) {
    let command = [];
    
    // Input arguments
    command.push('ffmpeg');
    command.push('-reconnect 1');
    command.push('-reconnect_streamed 1');
    command.push('-reconnect_delay_max 5');
    command.push('-i {input_url}');
    
    // Hardware acceleration setup
    if (data.enable_hardware_acceleration && data.preferred_hwaccel && data.preferred_hwaccel !== 'auto') {
        command.push(`-init_hw_device ${data.preferred_hwaccel}`);
        if (data.video_codec !== 'copy') {
            command.push(`-vf "format=nv12,hwupload"`);
        }
    }
    
    // Video codec
    if (data.video_codec === 'copy') {
        command.push('-c:v copy');
    } else {
        let videoEncoder = data.video_codec;
        
        // Add hardware acceleration suffix if enabled
        if (data.enable_hardware_acceleration && data.preferred_hwaccel && data.preferred_hwaccel !== 'auto') {
            const hwaccel = data.preferred_hwaccel;
            if (hwaccel === 'vaapi') {
                videoEncoder = `${data.video_codec}_vaapi`;
            } else if (hwaccel === 'nvenc') {
                videoEncoder = `${data.video_codec}_nvenc`;
            } else if (hwaccel === 'qsv') {
                videoEncoder = `${data.video_codec}_qsv`;
            } else if (hwaccel === 'amf') {
                videoEncoder = `${data.video_codec}_amf`;
            }
        }
        
        command.push(`-c:v ${videoEncoder}`);
        
        if (data.video_profile) {
            command.push(`-profile:v ${data.video_profile}`);
        }
        if (data.video_preset) {
            command.push(`-preset ${data.video_preset}`);
        }
        if (data.video_bitrate) {
            command.push(`-b:v ${data.video_bitrate}k`);
        }
    }
    
    // Audio codec
    if (data.audio_codec === 'copy') {
        command.push('-c:a copy');
    } else {
        command.push(`-c:a ${data.audio_codec}`);
        if (data.audio_bitrate) {
            command.push(`-b:a ${data.audio_bitrate}k`);
        }
    }
    
    // Output format
    if (data.output_format === 'hls') {
        command.push('-f hls');
        if (data.segment_duration) {
            command.push(`-hls_time ${data.segment_duration}`);
        }
        if (data.max_segments) {
            command.push(`-hls_list_size ${data.max_segments}`);
        }
        command.push('-hls_segment_filename {output_path}/segment_%03d.ts');
        command.push('-hls_flags delete_segments+temp_file');
        command.push('{output_path}/playlist.m3u8');
    } else {
        command.push('-f mpegts');
        command.push('{output_path}/stream.ts');
    }
    
    // Manual arguments override
    if (data.manual_args) {
        return `# Manual Arguments Override:\n${data.manual_args}`;
    }
    
    return command.join(' \\\n    ');
}

// Setup form change listeners for live preview
document.addEventListener('DOMContentLoaded', function() {
    setTimeout(() => {
        const formElements = document.querySelectorAll('#profileModal input, #profileModal select, #profileModal textarea');
        formElements.forEach(element => {
            element.addEventListener('change', updateFFmpegPreview);
            element.addEventListener('input', updateFFmpegPreview);
        });
    }, 100);
});

// Profile management functions
function editProfile(profileId) {
    showEditProfileModal(profileId);
}

async function deleteProfile(profileId) {
    if (!confirm('Are you sure you want to delete this relay profile?')) {
        return;
    }
    
    try {
        const response = await fetch(`${API_BASE}/profiles/${profileId}`, {
            method: 'DELETE'
        });
        
        if (response.ok) {
            showAlert('Profile deleted successfully', 'success');
            loadRelayProfiles();
        } else {
            const error = await response.text();
            showAlert(`Failed to delete profile: ${error}`, 'error');
        }
    } catch (error) {
        showAlert(`Error deleting profile: ${error.message}`, 'error');
    }
}

// Relay control functions
async function stopRelay(configId) {
    if (!confirm('Are you sure you want to stop this relay?')) {
        return;
    }

    try {
        const response = await fetch(`${API_BASE}/${configId}/stop`, {
            method: 'POST'
        });

        if (response.ok) {
            showAlert('Relay stopped successfully', 'success');
            loadActiveRelays();
        } else {
            const error = await response.text();
            showAlert(`Failed to stop relay: ${error}`, 'error');
        }
    } catch (error) {
        showAlert(`Error stopping relay: ${error.message}`, 'error');
    }
}

async function restartRelay(configId) {
    if (!confirm('Are you sure you want to restart this relay?')) {
        return;
    }

    try {
        // Stop first
        await fetch(`${API_BASE}/${configId}/stop`, { method: 'POST' });
        
        // Wait a moment
        await new Promise(resolve => setTimeout(resolve, 1000));
        
        // Start again
        const response = await fetch(`${API_BASE}/${configId}/start`, {
            method: 'POST'
        });

        if (response.ok) {
            showAlert('Relay restarted successfully', 'success');
            loadActiveRelays();
        } else {
            const error = await response.text();
            showAlert(`Failed to restart relay: ${error}`, 'error');
        }
    } catch (error) {
        showAlert(`Error restarting relay: ${error.message}`, 'error');
    }
}

// Utility functions
function formatBytes(bytes) {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}

function formatDuration(seconds) {
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

function formatDateTime(dateString) {
    const date = new Date(dateString);
    return date.toLocaleString();
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

function showAlert(message, type) {
    const alertsContainer = document.getElementById('alertsContainer');
    const alertDiv = document.createElement('div');
    alertDiv.className = `alert alert-${type}`;
    alertDiv.textContent = message;
    
    alertsContainer.appendChild(alertDiv);
    
    // Remove alert after 5 seconds
    setTimeout(() => {
        alertDiv.remove();
    }, 5000);
}

// Refresh function for manual updates
function refreshRelayStats() {
    loadActiveRelays();
    loadHealthData();
    showAlert('Stats refreshed', 'info');
}


// Load hardware acceleration information
async function loadHardwareAcceleration() {
    try {
        console.log('Loading hardware acceleration information...');
        const response = await fetch(`${API_BASE}/health`);
        
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}`);
        }
        
        const health = await response.json();
        displayHardwareAcceleration(health);
        
    } catch (error) {
        console.error('Error loading hardware acceleration:', error);
        document.getElementById('hwaccel-status').innerHTML = 
            '<span style="color: #dc3545;">Error loading hardware acceleration information</span>';
    }
}

// Display hardware acceleration information
function displayHardwareAcceleration(health) {
    const statusElement = document.getElementById('hwaccel-status');
    const detailsElement = document.getElementById('hwaccel-details');
    const tableElement = document.getElementById('hwaccel-table');
    const examplesElement = document.getElementById('hwaccel-examples');
    
    if (!health.ffmpeg_available) {
        statusElement.innerHTML = '<span style="color: #dc3545;">FFmpeg not available</span>';
        return;
    }
    
    if (!health.hwaccel_available) {
        statusElement.innerHTML = 
            '<span style="color: #ffc107;">Hardware acceleration not available</span>' +
            '<br><small>FFmpeg is available but no hardware accelerators are working</small>';
        return;
    }
    
    // Show hardware acceleration is available
    statusElement.innerHTML = 
        '<span style="color: #28a745;">Hardware acceleration available</span>' +
        `<br><small>FFmpeg ${health.ffmpeg_version} with ${health.hwaccel_capabilities.accelerators.length} accelerators</small>`;
    
    // Build capabilities table
    buildHwAccelTable(health.hwaccel_capabilities, tableElement);
    
    // Build example commands
    buildHwAccelExamples(health.hwaccel_capabilities, examplesElement);
    
    // Show details section
    detailsElement.style.display = 'block';
}

// Build hardware acceleration capabilities table
function buildHwAccelTable(capabilities, tableElement) {
    if (!capabilities.accelerators || capabilities.accelerators.length === 0) {
        tableElement.innerHTML = '<p class="hwaccel-unavailable">No hardware accelerators available</p>';
        return;
    }
    
    let tableHTML = '<div class="hwaccel-table">';
    
    // Header
    tableHTML += '<div class="table-header">';
    tableHTML += '<div class="table-row">';
    tableHTML += '<div class="table-cell">Accelerator</div>';
    
    // Add codec columns
    const codecs = capabilities.codecs || [];
    codecs.forEach(codec => {
        tableHTML += `<div class="table-cell">${codec.toUpperCase()}</div>`;
    });
    
    tableHTML += '</div></div>';
    
    // Data rows
    capabilities.accelerators.forEach(accelerator => {
        if (accelerator.available) {
            tableHTML += '<div class="table-row">';
            tableHTML += `<div class="table-cell">${accelerator.name.toUpperCase()}</div>`;
            
            codecs.forEach(codec => {
                const supported = accelerator.supported_codecs.includes(codec);
                const cellClass = supported ? 'hwaccel-available' : 'hwaccel-unavailable';
                const cellText = supported ? '✓' : '✗';
                tableHTML += `<div class="table-cell ${cellClass}">${cellText}</div>`;
            });
            
            tableHTML += '</div>';
        }
    });
    
    tableHTML += '</div>';
    tableElement.innerHTML = tableHTML;
}

// Build hardware acceleration example commands
function buildHwAccelExamples(capabilities, examplesElement) {
    if (!capabilities.accelerators || capabilities.accelerators.length === 0) {
        examplesElement.innerHTML = '<p class="hwaccel-unavailable">No hardware accelerators available</p>';
        return;
    }
    
    let examplesHTML = '';
    
    // Create examples for each available accelerator
    capabilities.accelerators.forEach(accelerator => {
        if (accelerator.available && accelerator.supported_codecs.length > 0) {
            const hwaccel = accelerator.name.toLowerCase();
            const codec = accelerator.supported_codecs[0]; // Use first supported codec
            
            examplesHTML += '<div class="hwaccel-example">';
            examplesHTML += `<div class="example-title">${accelerator.name.toUpperCase()} - ${codec.toUpperCase()}</div>`;
            examplesHTML += '<code>';
            examplesHTML += getExampleCommand(hwaccel, codec);
            examplesHTML += '</code>';
            examplesHTML += '</div>';
        }
    });
    
    if (examplesHTML === '') {
        examplesHTML = '<p class="hwaccel-unavailable">No working hardware accelerators found</p>';
    }
    
    examplesElement.innerHTML = examplesHTML;
}

// Get example FFmpeg command for hwaccel and codec
function getExampleCommand(hwaccel, codec) {
    const encoderMap = {
        'vaapi': {
            'h264': 'h264_vaapi',
            'hevc': 'hevc_vaapi',
            'av1': 'av1_vaapi',
            'vp9': 'vp9_vaapi'
        },
        'nvenc': {
            'h264': 'h264_nvenc',
            'hevc': 'hevc_nvenc',
            'av1': 'av1_nvenc'
        },
        'qsv': {
            'h264': 'h264_qsv',
            'hevc': 'hevc_qsv',
            'av1': 'av1_qsv'
        },
        'videotoolbox': {
            'h264': 'h264_videotoolbox',
            'hevc': 'hevc_videotoolbox'
        },
        'amf': {
            'h264': 'h264_amf',
            'hevc': 'hevc_amf',
            'av1': 'av1_amf'
        }
    };
    
    const encoder = encoderMap[hwaccel]?.[codec] || `${codec}_${hwaccel}`;
    
    let filterChain = '';
    switch (hwaccel) {
        case 'vaapi':
            filterChain = 'format=nv12,hwupload';
            break;
        case 'nvenc':
            filterChain = 'format=nv12,hwupload_cuda';
            break;
        case 'qsv':
            filterChain = 'format=nv12,hwupload=extra_hw_frames=64';
            break;
        default:
            filterChain = 'format=nv12,hwupload';
    }
    
    // Example command that reflects actual relay usage:
    // - Input: HTTP/HTTPS transport stream from upstream
    // - Output: HLS segments to sandboxed temp directory
    return `ffmpeg -init_hw_device ${hwaccel} \\
    -reconnect 1 -reconnect_streamed 1 -reconnect_delay_max 5 \\
    -i {input_url} \\
    -vf "${filterChain}" \\
    -c:v ${encoder} -preset fast -b:v 4M \\
    -c:a aac -b:a 128k \\
    -f hls -hls_time 4 -hls_list_size 6 \\
    -hls_segment_filename {output_path}/segment_%03d.ts \\
    -hls_flags delete_segments+temp_file \\
    {output_path}/playlist.m3u8`;
}

// Cleanup when page unloads
window.addEventListener('beforeunload', function() {
    if (updateInterval) {
        clearInterval(updateInterval);
    }
});