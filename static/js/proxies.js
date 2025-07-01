// Stream Proxies Management JavaScript

let currentProxies = [];
let editingProxy = null;
let previewData = null;
let availableSources = [];
let availableFilters = [];

// Initialize page
function initializeProxiesPage() {
  console.log("Initializing stream proxies page...");
  loadProxies();
  loadSources();
  loadFilters();

  // Setup standard modal close handlers
  SharedUtils.setupStandardModalCloseHandlers("proxyModal");
  SharedUtils.setupStandardModalCloseHandlers("proxyPreviewModal");
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
    showError("Failed to load stream proxies");
  }
}

// Load sources for dropdown
async function loadSources() {
  try {
    const response = await fetch("/api/v1/sources");
    if (!response.ok) throw new Error("Failed to load sources");

    availableSources = await response.json();
    populateSourcesDropdown();
  } catch (error) {
    console.error("Error loading sources:", error);
  }
}

// Load filters for checkboxes
async function loadFilters() {
  try {
    const response = await fetch("/api/v1/filters");
    if (!response.ok) throw new Error("Failed to load filters");

    availableFilters = await response.json();
  } catch (error) {
    console.error("Error loading filters:", error);
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
                    <h4 class="proxy-name">${escapeHtml(proxy.name)}</h4>
                    <span class="badge badge-${statusClass}">${status}</span>
                </div>
                <div class="proxy-card-body">
                    ${proxy.description ? `<p class="proxy-description">${escapeHtml(proxy.description)}</p>` : ""}
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

// Clear proxy form
function clearProxyForm() {
  document.getElementById("proxyForm").reset();
  document.getElementById("proxyActive").checked = true;
  document.getElementById("proxyAutoRegenerate").checked = true;
  populateSourcesDropdown();
  populateFiltersCheckboxes();
}

// Populate proxy form with data
function populateProxyForm(proxy) {
  document.getElementById("proxyName").value = proxy.name;
  document.getElementById("proxyDescription").value = proxy.description || "";
  document.getElementById("proxySource").value = proxy.source_id || "";
  document.getElementById("proxyActive").checked = proxy.is_active;
  document.getElementById("proxyAutoRegenerate").checked =
    proxy.auto_regenerate;

  populateSourcesDropdown();
  populateFiltersCheckboxes(proxy.filter_ids || []);
}

// Populate sources dropdown
function populateSourcesDropdown() {
  const select = document.getElementById("proxySource");

  // Clear existing options except the first one
  select.innerHTML = '<option value="">Select a source...</option>';

  availableSources.forEach((source) => {
    const option = document.createElement("option");
    option.value = source.id;
    option.textContent = source.name;
    select.appendChild(option);
  });
}

// Populate filters checkboxes
function populateFiltersCheckboxes(selectedFilterIds = []) {
  const container = document.getElementById("proxyFilters");

  if (availableFilters.length === 0) {
    container.innerHTML = '<p class="text-muted">No filters available</p>';
    return;
  }

  let html = '<div class="filters-checkboxes">';
  availableFilters.forEach((filter) => {
    const isChecked = selectedFilterIds.includes(filter.id) ? "checked" : "";
    html += `
            <label class="filter-checkbox">
                <input type="checkbox" name="filter_ids" value="${filter.id}" ${isChecked}>
                <span class="filter-name">${escapeHtml(filter.name)}</span>
                ${filter.description ? `<small class="filter-description">${escapeHtml(filter.description)}</small>` : ""}
            </label>
        `;
  });
  html += "</div>";

  container.innerHTML = html;
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
      showSuccess("Proxy updated successfully");
    } else {
      // Add new proxy to the list
      currentProxies.push(savedProxy);
      showSuccess("Proxy created successfully");
    }

    renderProxies();
    closeProxyModal();
  } catch (error) {
    console.error("Error saving proxy:", error);
    showError("Failed to save proxy: " + error.message);
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
    showSuccess("Proxy deleted successfully");
  } catch (error) {
    console.error("Error deleting proxy:", error);
    showError("Failed to delete proxy");
  }
}

// Regenerate proxy
async function regenerateProxy(proxyId) {
  try {
    showInfo("Regenerating proxy...");

    const response = await fetch(`/api/v1/proxies/${proxyId}/regenerate`, {
      method: "POST",
    });

    if (!response.ok) throw new Error("Failed to regenerate proxy");

    const result = await response.json();
    showSuccess(
      `Proxy regenerated successfully. Generated ${result.channel_count} channels.`,
    );

    // Reload proxies to get updated info
    loadProxies();
  } catch (error) {
    console.error("Error regenerating proxy:", error);
    showError("Failed to regenerate proxy");
  }
}

// Regenerate all proxies
async function regenerateAllProxies() {
  if (!confirm("Are you sure you want to regenerate all active proxies?")) {
    return;
  }

  try {
    showInfo("Regenerating all proxies...");

    const response = await fetch("/api/v1/proxies/regenerate-all", {
      method: "POST",
    });

    if (!response.ok) throw new Error("Failed to regenerate proxies");

    const result = await response.json();
    showSuccess(`Regenerated ${result.count} proxies successfully.`);

    // Reload proxies to get updated info
    loadProxies();
  } catch (error) {
    console.error("Error regenerating all proxies:", error);
    showError("Failed to regenerate proxies");
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
    showInfo("Generating preview...");

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
    showError("Failed to generate preview");
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
                <td>${escapeHtml(channel.channel_name)}</td>
                <td>${escapeHtml(channel.group_title || "Uncategorized")}</td>
                <td>${channel.tvg_logo ? `<img src="${channel.tvg_logo}" alt="Logo" class="channel-logo-small">` : "No logo"}</td>
                <td><code>${escapeHtml(channel.stream_url.substring(0, 50))}...</code></td>
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
      showSuccess("M3U content copied to clipboard");
    })
    .catch(() => {
      showError("Failed to copy to clipboard");
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
