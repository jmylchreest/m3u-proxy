// Stream Proxies Management JavaScript

let currentProxies = [];
let editingProxy = null;
let previewData = null;
let availableStreamSources = [];
let availableEpgSources = [];
let availableFilters = [];

// Current proxy configuration arrays
let selectedStreamSources = [];
let selectedEpgSources = [];
let selectedFilters = [];

// Initialize page
function initializeProxiesPage() {
  console.log("Initializing stream proxies page...");
  loadProxies();
  loadStreamSources();
  loadEpgSources();
  loadFilters();

  // Setup standard modal close handlers
  SharedUtils.setupStandardModalCloseHandlers("proxyModal");
  SharedUtils.setupStandardModalCloseHandlers("proxyPreviewModal");
  SharedUtils.setupStandardModalCloseHandlers("addStreamSourceModal");
  SharedUtils.setupStandardModalCloseHandlers("addEpgSourceModal");
  SharedUtils.setupStandardModalCloseHandlers("addFilterModal");
}

// Check if DOM is already loaded
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", initializeProxiesPage);
} else {
  initializeProxiesPage();
}

// Load all stream proxies
async function loadProxies() {
  try {
    const response = await fetch("/api/v1/proxies?" + new Date().getTime());
    console.log("Proxies API response status:", response.status);
    if (!response.ok) throw new Error("Failed to load proxies");

    const data = await response.json();
    console.log("Proxies API response:", data);
    currentProxies = Array.isArray(data) ? data : [];
    console.log("Current proxies count:", currentProxies.length);
    renderProxies();
  } catch (error) {
    console.error("Error loading proxies:", error);
    currentProxies = [];
    renderProxies();
    SharedUtils.showError("Failed to load stream proxies");
  }
}

// Load stream sources
async function loadStreamSources() {
  try {
    console.log("Loading stream sources...");
    const response = await fetch("/api/v1/sources/stream");
    if (!response.ok) throw new Error("Failed to load stream sources");

    const data = await response.json();
    console.log("Stream sources API response:", data);
    availableStreamSources = Array.isArray(data) ? data : (Array.isArray(data.data) ? data.data : []);
    console.log("Available stream sources loaded:", availableStreamSources.length);
  } catch (error) {
    console.error("Error loading stream sources:", error);
    availableStreamSources = [];
  }
}

// Load EPG sources
async function loadEpgSources() {
  try {
    const response = await fetch("/api/v1/sources/epg");
    if (!response.ok) throw new Error("Failed to load EPG sources");

    const data = await response.json();
    availableEpgSources = Array.isArray(data) ? data : (Array.isArray(data.data) ? data.data : []);
  } catch (error) {
    console.error("Error loading EPG sources:", error);
    availableEpgSources = [];
  }
}

// Load filters
async function loadFilters() {
  try {
    const response = await fetch("/api/v1/filters");
    if (!response.ok) throw new Error("Failed to load filters");

    const data = await response.json();
    availableFilters = Array.isArray(data) ? data : (Array.isArray(data.data) ? data.data : []);
  } catch (error) {
    console.error("Error loading filters:", error);
    availableFilters = [];
  }
}

// Render proxies list
function renderProxies() {
  const container = document.getElementById("proxiesContainer");

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

  currentProxies.forEach((proxy) => {
    const lastGenerated = proxy.last_generated
      ? new Date(proxy.last_generated).toLocaleDateString()
      : "Never";

    const status = proxy.is_active ? "Active" : "Inactive";
    const statusClass = proxy.is_active ? "success" : "secondary";

    html += `
            <div class="proxy-card" data-proxy-id="${proxy.id}">
                <div class="proxy-card-header">
                    <h4 class="proxy-name">${SharedUtils.escapeHtml(proxy.name)}</h4>
                    <span class="badge badge-${statusClass}">${status}</span>
                </div>
                <div class="proxy-card-body">
                    ${proxy.description ? `<p class="proxy-description">${SharedUtils.escapeHtml(proxy.description)}</p>` : ""}
                    <div class="proxy-meta">
                        <small class="text-muted">
                            <strong>Source:</strong> ${proxy.source_name || "Unknown"}<br>
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

  html += "</div>";
  container.innerHTML = html;
}

// Create new proxy
function createProxy() {
  editingProxy = null;
  document.getElementById("proxyModalTitle").textContent =
    "Create Stream Proxy";
  clearProxyForm();
  SharedUtils.showStandardModal("proxyModal");
}

// Edit existing proxy
function editProxy(proxyId) {
  const proxy = currentProxies.find((p) => p.id === proxyId);
  if (!proxy) return;

  editingProxy = proxy;
  document.getElementById("proxyModalTitle").textContent = "Edit Stream Proxy";
  populateProxyForm(proxy);
  SharedUtils.showStandardModal("proxyModal");
}

// Clear proxy form (updated for priority system)
function clearProxyFormOld() {
  document.getElementById("proxyForm").reset();
  document.getElementById("proxyActive").checked = true;
  document.getElementById("proxyAutoRegenerate").checked = true;
  
  // Reset priority selections
  selectedStreamSources = [];
  selectedEpgSources = [];
  selectedFilters = [];
  
  // Re-render all priority lists
  renderStreamSources();
  renderEpgSources();
  renderFilters();
}

// Populate proxy form with data (updated for priority system)
function populateProxyForm(proxy) {
  document.getElementById("proxyName").value = proxy.name;
  document.getElementById("proxyDescription").value = proxy.description || "";
  document.getElementById("proxyActive").checked = proxy.is_active;
  document.getElementById("proxyAutoRegenerate").checked = proxy.auto_regenerate || false;
  
  // Set streaming configuration
  document.getElementById("proxyMode").value = proxy.proxy_mode || "redirect";
  document.getElementById("upstreamTimeout").value = proxy.upstream_timeout || 30;
  document.getElementById("bufferSize").value = proxy.buffer_size || 8192;
  document.getElementById("maxConcurrentStreams").value = proxy.max_concurrent_streams || 1000;

  // Reset and populate priority selections
  selectedStreamSources = proxy.stream_sources || [];
  selectedEpgSources = proxy.epg_sources || [];
  selectedFilters = proxy.filters || [];
  
  // Re-render all priority lists
  renderStreamSources();
  renderEpgSources();
  renderFilters();
}

// Save proxy
async function saveProxy() {
  const form = document.getElementById("proxyForm");
  const formData = new FormData(form);

  // Get selected filter IDs
  const filterIds = Array.from(
    form.querySelectorAll('input[name="filter_ids"]:checked'),
  ).map((cb) => cb.value);

  const proxyData = {
    name: formData.get("name"),
    description: formData.get("description"),
    source_id: formData.get("source_id"),
    filter_ids: filterIds,
    is_active: formData.has("is_active"),
    auto_regenerate: formData.has("auto_regenerate"),
  };

  try {
    const url = editingProxy
      ? `/api/v1/proxies/${editingProxy.id}`
      : "/api/v1/proxies";
    const method = editingProxy ? "PUT" : "POST";

    const response = await fetch(url, {
      method: method,
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(proxyData),
    });

    if (!response.ok) {
      const error = await response.text();
      throw new Error(error);
    }

    const savedProxy = await response.json();

    if (editingProxy) {
      // Update existing proxy in the list
      const index = currentProxies.findIndex((p) => p.id === editingProxy.id);
      if (index !== -1) {
        currentProxies[index] = savedProxy;
      }
      SharedUtils.showSuccess("Proxy updated successfully");
    } else {
      // Add new proxy to the list
      currentProxies.push(savedProxy);
      SharedUtils.showSuccess("Proxy created successfully");
    }

    renderProxies();
    closeProxyModal();
  } catch (error) {
    console.error("Error saving proxy:", error);
    SharedUtils.showError("Failed to save proxy: " + error.message);
  }
}

// Delete proxy
async function deleteProxy(proxyId) {
  const proxy = currentProxies.find((p) => p.id === proxyId);
  if (!proxy) return;

  if (!confirm(`Are you sure you want to delete the proxy "${proxy.name}"?`)) {
    return;
  }

  try {
    const response = await fetch(`/api/v1/proxies/${proxyId}`, {
      method: "DELETE",
    });

    if (!response.ok) throw new Error("Failed to delete proxy");

    // Remove from local list
    currentProxies = currentProxies.filter((p) => p.id !== proxyId);
    renderProxies();
    SharedUtils.showSuccess("Proxy deleted successfully");
  } catch (error) {
    console.error("Error deleting proxy:", error);
    SharedUtils.showError("Failed to delete proxy");
  }
}

// Regenerate proxy
async function regenerateProxy(proxyId) {
  try {
    SharedUtils.showInfo("Regenerating proxy...");

    const response = await fetch(`/api/v1/proxies/${proxyId}/regenerate`, {
      method: "POST",
    });

    if (!response.ok) throw new Error("Failed to regenerate proxy");

    const result = await response.json();
    SharedUtils.showSuccess(
      `Proxy regenerated successfully. Generated ${result.channel_count} channels.`,
    );

    // Reload proxies to get updated info
    loadProxies();
  } catch (error) {
    console.error("Error regenerating proxy:", error);
    SharedUtils.showError("Failed to regenerate proxy");
  }
}

// Regenerate all proxies
async function regenerateAllProxies() {
  if (!confirm("Are you sure you want to regenerate all active proxies?")) {
    return;
  }

  try {
    SharedUtils.showInfo("Regenerating all proxies...");

    const response = await fetch("/api/v1/proxies/regenerate-all", {
      method: "POST",
    });

    if (!response.ok) throw new Error("Failed to regenerate proxies");

    const result = await response.json();
    SharedUtils.showSuccess(`Regenerated ${result.count} proxies successfully.`);

    // Reload proxies to get updated info
    loadProxies();
  } catch (error) {
    console.error("Error regenerating all proxies:", error);
    SharedUtils.showError("Failed to regenerate proxies");
  }
}

// Preview proxy
async function previewProxy(proxyId) {
  if (!proxyId && editingProxy) {
    // Preview from form data
    const form = document.getElementById("proxyForm");
    const formData = new FormData(form);

    const filterIds = Array.from(
      form.querySelectorAll('input[name="filter_ids"]:checked'),
    ).map((cb) => cb.value);

    const previewData = {
      source_id: formData.get("source_id"),
      filter_ids: filterIds,
    };

    await generatePreview(previewData);
  } else {
    // Preview existing proxy
    const proxy = currentProxies.find((p) => p.id === proxyId);
    if (!proxy) return;

    await generatePreview(proxy);
  }
}

// Generate preview data
async function generatePreview(proxyData) {
  try {
    SharedUtils.showInfo("Generating preview...");

    const response = await fetch("/api/v1/proxies/preview", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(proxyData),
    });

    if (!response.ok) throw new Error("Failed to generate preview");

    previewData = await response.json();
    showPreviewModal();
  } catch (error) {
    console.error("Error generating preview:", error);
    SharedUtils.showError("Failed to generate preview");
  }
}

// Show preview modal
function showPreviewModal() {
  if (!previewData) return;

  // Show channels tab by default
  showPreviewTab("channels");
  SharedUtils.showStandardModal("proxyPreviewModal");
}

// Show preview tab
function showPreviewTab(tabName) {
  // Update tab buttons
  document.querySelectorAll(".tab-button").forEach((btn) => {
    btn.classList.remove("active");
  });
  document
    .querySelector(`[onclick="showPreviewTab('${tabName}')"]`)
    .classList.add("active");

  // Update tab content
  document.querySelectorAll(".tab-content").forEach((content) => {
    content.classList.remove("active");
  });
  document.getElementById(tabName + "Tab").classList.add("active");

  // Load tab content
  switch (tabName) {
    case "channels":
      renderChannelsPreview();
      break;
    case "m3u":
      renderM3uPreview();
      break;
    case "stats":
      renderStatsPreview();
      break;
  }
}

// Render channels preview
function renderChannelsPreview() {
  if (!previewData || !previewData.channels) return;

  const container = document.getElementById("channelsPreview");
  const channels = previewData.channels;

  if (channels.length === 0) {
    container.innerHTML = '<p class="text-muted">No channels found</p>';
    return;
  }

  let html = '<div class="channels-table"><table class="table"><thead><tr>';
  html += "<th>Channel Name</th><th>Group</th><th>Logo</th><th>Stream URL</th>";
  html += "</tr></thead><tbody>";

  channels.forEach((channel) => {
    html += `
            <tr>
                <td>${SharedUtils.escapeHtml(channel.channel_name)}</td>
                <td>${SharedUtils.escapeHtml(channel.group_title || "Uncategorized")}</td>
                <td>${channel.tvg_logo ? `<img src="${channel.tvg_logo}" alt="Logo" class="channel-logo-small">` : "No logo"}</td>
                <td><code>${SharedUtils.escapeHtml(channel.stream_url.substring(0, 50))}...</code></td>
            </tr>
        `;
  });

  html += "</tbody></table></div>";
  container.innerHTML = html;

  // Populate group filter
  const groups = [
    ...new Set(channels.map((ch) => ch.group_title || "Uncategorized")),
  ].sort();
  const groupFilter = document.getElementById("groupFilter");
  groupFilter.innerHTML = '<option value="">All Groups</option>';
  groups.forEach((group) => {
    const option = document.createElement("option");
    option.value = group;
    option.textContent = group;
    groupFilter.appendChild(option);
  });
}

// Render M3U preview
function renderM3uPreview() {
  if (!previewData || !previewData.m3u_content) return;

  const container = document.getElementById("m3uContent");
  container.textContent = previewData.m3u_content;
}

// Render stats preview
function renderStatsPreview() {
  if (!previewData || !previewData.stats) return;

  const container = document.getElementById("proxyStats");
  const stats = previewData.stats;

  let html = `
        <div class="stat-card">
            <h4>Total Channels</h4>
            <div class="stat-value">${stats.total_channels}</div>
        </div>
        <div class="stat-card">
            <h4>Groups</h4>
            <div class="stat-value">${stats.total_groups}</div>
        </div>
        <div class="stat-card">
            <h4>With Logos</h4>
            <div class="stat-value">${stats.channels_with_logos}</div>
        </div>
        <div class="stat-card">
            <h4>File Size</h4>
            <div class="stat-value">${formatFileSize(stats.m3u_size)}</div>
        </div>
    `;

  container.innerHTML = html;
}

// Filter preview channels
function filterPreviewChannels() {
  const searchTerm = document
    .getElementById("channelSearch")
    .value.toLowerCase();
  const selectedGroup = document.getElementById("groupFilter").value;

  const rows = document.querySelectorAll("#channelsPreview tbody tr");

  rows.forEach((row) => {
    const channelName = row.children[0].textContent.toLowerCase();
    const groupName = row.children[1].textContent;

    const matchesSearch = !searchTerm || channelName.includes(searchTerm);
    const matchesGroup = !selectedGroup || groupName === selectedGroup;

    row.style.display = matchesSearch && matchesGroup ? "" : "none";
  });
}

// Copy M3U content
function copyM3uContent() {
  const content = document.getElementById("m3uContent").textContent;
  navigator.clipboard
    .writeText(content)
    .then(() => {
      SharedUtils.showSuccess("M3U content copied to clipboard");
    })
    .catch(() => {
      SharedUtils.showError("Failed to copy to clipboard");
    });
}

// Download M3U
function downloadM3u() {
  const content = document.getElementById("m3uContent").textContent;
  const blob = new Blob([content], { type: "application/x-mpegurl" });
  const url = URL.createObjectURL(blob);

  const a = document.createElement("a");
  a.href = url;
  a.download = "proxy.m3u";
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

// Modal management
function closeProxyModal() {
  SharedUtils.hideStandardModal("proxyModal");
}

function closeProxyPreview() {
  SharedUtils.hideStandardModal("proxyPreviewModal");
}

// Utility functions
function formatFileSize(bytes) {
  if (bytes === 0) return "0 Bytes";
  const k = 1024;
  const sizes = ["Bytes", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

// ============================================================================
// PRIORITY MANAGEMENT FUNCTIONS
// ============================================================================

// Render priority cards for stream sources
function renderStreamSources() {
  const container = document.getElementById("streamSourcesList");
  if (!container) return;

  if (selectedStreamSources.length === 0) {
    container.innerHTML = '<p class="text-muted small">No stream sources selected</p>';
    return;
  }

  let html = '';
  selectedStreamSources.forEach((source, index) => {
    const stats = `${source.channel_count || 0} channels`;
    html += `
      <div class="priority-card" draggable="true" data-type="stream" data-id="${source.id}" data-index="${index}">
        <div class="priority-card-header">
          <span class="drag-handle">⋯</span>
          <span class="priority-card-title">${SharedUtils.escapeHtml(source.name)}</span>
          <span class="priority-card-stats">${stats}</span>
          <button class="detach-btn" onclick="removeStreamSource(${index})" title="Remove source">×</button>
        </div>
      </div>
    `;
  });

  container.innerHTML = html;
  setupDragAndDrop('streamSourcesList', 'stream');
}

// Render priority cards for EPG sources
function renderEpgSources() {
  const container = document.getElementById("epgSourcesList");
  if (!container) return;

  if (selectedEpgSources.length === 0) {
    container.innerHTML = '<p class="text-muted small">No EPG sources selected</p>';
    return;
  }

  let html = '';
  selectedEpgSources.forEach((source, index) => {
    const stats = `${source.program_count || 0} programs`;
    html += `
      <div class="priority-card" draggable="true" data-type="epg" data-id="${source.id}" data-index="${index}">
        <div class="priority-card-header">
          <span class="drag-handle">⋯</span>
          <span class="priority-card-title">${SharedUtils.escapeHtml(source.name)}</span>
          <span class="priority-card-stats">${stats}</span>
          <button class="detach-btn" onclick="removeEpgSource(${index})" title="Remove source">×</button>
        </div>
      </div>
    `;
  });

  container.innerHTML = html;
  setupDragAndDrop('epgSourcesList', 'epg');
}

// Render priority cards for filters
function renderFilters() {
  const container = document.getElementById("filtersList");
  if (!container) return;

  if (selectedFilters.length === 0) {
    container.innerHTML = '<p class="text-muted small">No filters selected</p>';
    return;
  }

  let html = '';
  selectedFilters.forEach((item, index) => {
    const filter = item.filter || item; // Handle nested filter object
    const title = filter.description ? `${SharedUtils.escapeHtml(filter.name)}: ${SharedUtils.escapeHtml(filter.description)}` : SharedUtils.escapeHtml(filter.name);
    const type = filter.is_inverse ? 'EXCLUDE' : 'INCLUDE';
    html += `
      <div class="priority-card" draggable="true" data-type="filter" data-id="${filter.id}" data-index="${index}">
        <div class="priority-card-header">
          <span class="drag-handle">⋯</span>
          <span class="priority-card-title">${title} <sup class="text-muted">${type}</sup></span>
          <span class="priority-card-stats">${filter.source_type || 'Filter'}</span>
          <button class="detach-btn" onclick="removeFilter(${index})" title="Remove filter">×</button>
        </div>
      </div>
    `;
  });

  container.innerHTML = html;
  setupDragAndDrop('filtersList', 'filter');
}

// Setup drag and drop for a container
function setupDragAndDrop(containerId, type) {
  const container = document.getElementById(containerId);
  if (!container) return;

  const cards = container.querySelectorAll('.priority-card');
  
  cards.forEach(card => {
    card.addEventListener('dragstart', (e) => {
      card.classList.add('dragging');
      e.dataTransfer.setData('text/plain', '');
      e.dataTransfer.effectAllowed = 'move';
    });

    card.addEventListener('dragend', () => {
      card.classList.remove('dragging');
      container.classList.remove('drag-over');
    });
  });

  container.addEventListener('dragover', (e) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    
    const dragging = container.querySelector('.dragging');
    if (!dragging) return;

    const siblings = [...container.querySelectorAll('.priority-card:not(.dragging)')];
    const nextSibling = siblings.find(sibling => {
      return e.clientY <= sibling.getBoundingClientRect().top + sibling.offsetHeight / 2;
    });

    container.insertBefore(dragging, nextSibling);
  });

  container.addEventListener('drop', (e) => {
    e.preventDefault();
    container.classList.remove('drag-over');
    
    // Update the array order based on the new DOM order
    const cards = [...container.querySelectorAll('.priority-card')];
    const newOrder = cards.map(card => parseInt(card.dataset.index));
    
    if (type === 'stream') {
      selectedStreamSources = newOrder.map(index => selectedStreamSources[index]);
      renderStreamSources();
    } else if (type === 'epg') {
      selectedEpgSources = newOrder.map(index => selectedEpgSources[index]);
      renderEpgSources();
    } else if (type === 'filter') {
      selectedFilters = newOrder.map(index => selectedFilters[index]);
      renderFilters();
    }
  });
}

// Add/Remove functions
function removeStreamSource(index) {
  selectedStreamSources.splice(index, 1);
  renderStreamSources();
}

function removeEpgSource(index) {
  selectedEpgSources.splice(index, 1);
  renderEpgSources();
}

function removeFilter(index) {
  selectedFilters.splice(index, 1);
  renderFilters();
}

// Modal functions
function openAddStreamSourceModal() {
  console.log('Opening stream source modal...');
  console.log('Available stream sources:', availableStreamSources.length);
  console.log('Selected stream sources:', selectedStreamSources.length);
  
  populateStreamSourceSelection();
  SharedUtils.showStandardModal('addStreamSourceModal');
  console.log('Modal opened using SharedUtils');
}

function openAddEpgSourceModal() {
  populateEpgSourceSelection();
  SharedUtils.showStandardModal('addEpgSourceModal');
}

function openAddFilterModal() {
  populateFilterSelection();
  SharedUtils.showStandardModal('addFilterModal');
}

function closeAddStreamSourceModal() {
  SharedUtils.hideStandardModal('addStreamSourceModal');
}

function closeAddEpgSourceModal() {
  SharedUtils.hideStandardModal('addEpgSourceModal');
}

function closeAddFilterModal() {
  SharedUtils.hideStandardModal('addFilterModal');
}

// Populate selection modals
function populateStreamSourceSelection() {
  const container = document.getElementById('streamSourcesSelectionList');
  console.log('Container found:', container);
  if (!container) {
    console.error('Stream sources selection container not found!');
    return;
  }

  const alreadySelected = selectedStreamSources.map(s => s.id);
  const available = availableStreamSources.filter(s => !alreadySelected.includes(s.id));
  
  console.log('Already selected IDs:', alreadySelected);
  console.log('Available sources after filtering:', available.length);

  if (available.length === 0) {
    console.log('No available sources - showing message');
    container.innerHTML = '<p class="text-muted">All available stream sources are already selected</p>';
    return;
  }

  let html = '';
  available.forEach(source => {
    const title = source.description ? `${SharedUtils.escapeHtml(source.name)}: ${SharedUtils.escapeHtml(source.description)}` : SharedUtils.escapeHtml(source.name);
    const stats = `${source.channel_count || 0} channels`;
    const isSelected = selectedStreamSources.some(s => s.id === source.id);
    html += `
      <div class="source-selection-item ${isSelected ? 'selected' : ''}" onclick="toggleStreamSourceSelection('${source.id}')">
        <input type="checkbox" id="stream_${source.id}" ${isSelected ? 'checked' : ''} onchange="toggleStreamSourceSelection('${source.id}')" />
        <div class="source-selection-content">
          <div class="source-selection-title">${title}</div>
          <div class="source-selection-stats">${stats}</div>
        </div>
      </div>
    `;
  });

  container.innerHTML = html;
}

function populateEpgSourceSelection() {
  const container = document.getElementById('epgSourcesSelectionList');
  if (!container) return;

  const alreadySelected = selectedEpgSources.map(s => s.id);
  const available = availableEpgSources.filter(s => !alreadySelected.includes(s.id));

  if (available.length === 0) {
    container.innerHTML = '<p class="text-muted">All available EPG sources are already selected</p>';
    return;
  }

  let html = '';
  available.forEach(source => {
    const title = source.description ? `${SharedUtils.escapeHtml(source.name)}: ${SharedUtils.escapeHtml(source.description)}` : SharedUtils.escapeHtml(source.name);
    const stats = `${source.program_count || 0} programs`;
    const isSelected = selectedEpgSources.some(s => s.id === source.id);
    html += `
      <div class="source-selection-item ${isSelected ? 'selected' : ''}" onclick="toggleEpgSourceSelection('${source.id}')">
        <input type="checkbox" id="epg_${source.id}" ${isSelected ? 'checked' : ''} onchange="toggleEpgSourceSelection('${source.id}')" />
        <div class="source-selection-content">
          <div class="source-selection-title">${title}</div>
          <div class="source-selection-stats">${stats}</div>
        </div>
      </div>
    `;
  });

  container.innerHTML = html;
}

function populateFilterSelection() {
  const container = document.getElementById('filtersSelectionList');
  if (!container) return;

  const alreadySelected = selectedFilters.map(f => f.id);
  const available = availableFilters.filter(f => !alreadySelected.includes(f.id));

  if (available.length === 0) {
    container.innerHTML = '<p class="text-muted">All available filters are already selected</p>';
    return;
  }

  let html = '';
  available.forEach(item => {
    const filter = item.filter || item; // Handle nested filter object
    const title = filter.description ? `${SharedUtils.escapeHtml(filter.name)}: ${SharedUtils.escapeHtml(filter.description)}` : SharedUtils.escapeHtml(filter.name);
    const type = filter.is_inverse ? 'EXCLUDE' : 'INCLUDE';
    const isSelected = selectedFilters.some(f => f.id === filter.id);
    html += `
      <div class="source-selection-item ${isSelected ? 'selected' : ''}" onclick="toggleFilterSelection('${filter.id}')">
        <input type="checkbox" id="filter_${filter.id}" ${isSelected ? 'checked' : ''} onchange="toggleFilterSelection('${filter.id}')" />
        <div class="source-selection-title">${title} <sup class="text-muted">${type}</sup></div>
        <div class="source-selection-stats">${filter.source_type || 'Filter'}</div>
      </div>
    `;
  });

  container.innerHTML = html;
}

// Toggle selection functions
function toggleStreamSourceSelection(sourceId) {
  const source = availableStreamSources.find(s => s.id === sourceId);
  if (!source) return;
  
  const existingIndex = selectedStreamSources.findIndex(s => s.id === sourceId);
  if (existingIndex >= 0) {
    selectedStreamSources.splice(existingIndex, 1);
  } else {
    selectedStreamSources.push(source);
  }
  
  // Update UI
  const checkbox = document.getElementById(`stream_${sourceId}`);
  const item = checkbox.closest('.source-selection-item');
  if (existingIndex >= 0) {
    checkbox.checked = false;
    item.classList.remove('selected');
  } else {
    checkbox.checked = true;
    item.classList.add('selected');
  }
}

function toggleEpgSourceSelection(sourceId) {
  const source = availableEpgSources.find(s => s.id === sourceId);
  if (!source) return;
  
  const existingIndex = selectedEpgSources.findIndex(s => s.id === sourceId);
  if (existingIndex >= 0) {
    selectedEpgSources.splice(existingIndex, 1);
  } else {
    selectedEpgSources.push(source);
  }
  
  // Update UI
  const checkbox = document.getElementById(`epg_${sourceId}`);
  const item = checkbox.closest('.source-selection-item');
  if (existingIndex >= 0) {
    checkbox.checked = false;
    item.classList.remove('selected');
  } else {
    checkbox.checked = true;
    item.classList.add('selected');
  }
}

function toggleFilterSelection(filterId) {
  const filterItem = availableFilters.find(item => {
    const filter = item.filter || item;
    return filter.id === filterId;
  });
  if (!filterItem) return;
  
  const filter = filterItem.filter || filterItem;
  const existingIndex = selectedFilters.findIndex(f => {
    const selectedFilter = f.filter || f;
    return selectedFilter.id === filterId;
  });
  
  if (existingIndex >= 0) {
    selectedFilters.splice(existingIndex, 1);
  } else {
    selectedFilters.push(filterItem);
  }
  
  // Update UI
  const checkbox = document.getElementById(`filter_${filterId}`);
  const item = checkbox.closest('.source-selection-item');
  if (existingIndex >= 0) {
    checkbox.checked = false;
    item.classList.remove('selected');
  } else {
    checkbox.checked = true;
    item.classList.add('selected');
  }
}

// Save selection functions
function saveSelectedStreamSources() {
  renderStreamSources();
  closeAddStreamSourceModal();
}

function saveSelectedEpgSources() {
  renderEpgSources();
  closeAddEpgSourceModal();
}

function saveSelectedFilters() {
  renderFilters();
  closeAddFilterModal();
}

// Clear proxy form and reset selections
function clearProxyForm() {
  document.getElementById("proxyForm").reset();
  editingProxy = null;
  
  // Reset priority selections
  selectedStreamSources = [];
  selectedEpgSources = [];
  selectedFilters = [];
  
  // Re-render all priority lists
  renderStreamSources();
  renderEpgSources();
  renderFilters();
}
