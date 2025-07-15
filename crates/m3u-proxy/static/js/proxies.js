// Stream Proxies Management JavaScript

// Utility function to convert UUID to base64 (URL safe, no padding)
function uuidToBase64(uuid) {
  // Remove hyphens and convert to bytes
  const hex = uuid.replace(/-/g, "");
  const bytes = [];
  for (let i = 0; i < hex.length; i += 2) {
    bytes.push(parseInt(hex.substr(i, 2), 16));
  }
  // Convert to base64 and make URL safe
  let base64 = btoa(String.fromCharCode.apply(null, bytes));
  return base64.replace(/\+/g, "-").replace(/\//g, "_").replace(/=/g, "");
}

let currentProxies = [];
let editingProxy = null;
let previewData = null;
let previewEpgData = null;

let currentStatsTab = "overview";
let epgTooltip = null;
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
    // Handle paginated API response format
    currentProxies = data.data ? data.data.items : data;
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
    availableStreamSources = Array.isArray(data)
      ? data
      : Array.isArray(data.data)
        ? data.data
        : [];
    console.log(
      "Available stream sources loaded:",
      availableStreamSources.length,
    );
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
    availableEpgSources = Array.isArray(data)
      ? data
      : Array.isArray(data.data)
        ? data.data
        : [];
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
    availableFilters = Array.isArray(data)
      ? data
      : Array.isArray(data.data)
        ? data.data
        : [];
  } catch (error) {
    console.error("Error loading filters:", error);
    availableFilters = [];
  }
}

// Render proxies list
function renderProxies() {
  const tbody = document.getElementById("proxiesTableBody");

  if (currentProxies.length === 0) {
    tbody.innerHTML = `
            <tr>
                <td colspan="6" class="text-center text-muted">
                    No stream proxies configured. Click "Add Proxy" to get started.
                </td>
            </tr>
        `;
    return;
  }

  tbody.innerHTML = "";

  currentProxies.forEach((proxy) => {
    const row = document.createElement("tr");
    // More robust active status check to handle different data types
    const isActive =
      proxy.is_active === true ||
      proxy.is_active === "true" ||
      proxy.is_active === 1;
    row.style.opacity = isActive ? "1" : "0.6";
    console.log(
      `Proxy ${proxy.name}: is_active = ${proxy.is_active} (${typeof proxy.is_active}), calculated isActive = ${isActive}`,
    );

    // Name and Configuration column
    const nameCell = document.createElement("td");
    const modeIndicator = proxy.proxy_mode === "redirect" ? "REDIR" : "PROXY";

    nameCell.innerHTML = `
            <div class="proxy-info">
                <div class="proxy-name">
                    <strong>${escapeHtml(proxy.name)}<sup class="text-muted" style="font-size: 0.7em; margin-left: 3px;">${modeIndicator}</sup></strong>
                </div>
                ${
                  proxy.description
                    ? `<div class="proxy-description text-muted">
                    <small>${escapeHtml(proxy.description)}</small>
                </div>`
                    : ""
                }
                <div class="proxy-config text-muted">
                    <small>📺 Channel #${proxy.starting_channel_number}${proxy.upstream_timeout ? ` • ⏱️ ${proxy.upstream_timeout}s` : ""}${proxy.max_concurrent_streams ? ` • 🔗 ${proxy.max_concurrent_streams}` : ""}</small>
                </div>
            </div>
        `;

    // Stream Sources column
    const streamSourcesCell = document.createElement("td");
    streamSourcesCell.innerHTML = renderSourcesList(
      proxy.stream_sources,
      "stream",
    );

    // EPG Sources column
    const epgSourcesCell = document.createElement("td");
    epgSourcesCell.innerHTML = renderSourcesList(proxy.epg_sources, "epg");

    // Filters column
    const filtersCell = document.createElement("td");
    filtersCell.innerHTML = renderFiltersList(proxy.filters);

    // Status column
    const statusCell = document.createElement("td");
    statusCell.innerHTML = renderProxyStatusCell(proxy);

    // Actions column
    const actionsCell = document.createElement("td");
    actionsCell.innerHTML = renderProxyActionsCell(proxy);

    row.appendChild(nameCell);
    row.appendChild(streamSourcesCell);
    row.appendChild(epgSourcesCell);
    row.appendChild(filtersCell);
    row.appendChild(statusCell);
    row.appendChild(actionsCell);

    tbody.appendChild(row);
  });
}

// Helper function to render sources list with limit
function renderSourcesList(sources, type) {
  if (!sources || sources.length === 0) {
    return '<span class="text-muted">None</span>';
  }

  const limit = 3;
  const visibleSources = sources.slice(0, limit);
  const remaining = sources.length - limit;

  let html = '<div class="sources-list">';

  visibleSources.forEach((source, index) => {
    let name;
    if (type === "epg") {
      name = source.epg_source_name || source.name || "Unknown";
    } else {
      name = source.source_name || source.name || "Unknown";
    }
    html += `<div class="source-item text-muted">
                <small>${escapeHtml(name)}</small>
             </div>`;
  });

  if (remaining > 0) {
    html += `<div class="source-item text-muted">
                <small><em>+${remaining} more</em></small>
             </div>`;
  }

  html += "</div>";
  return html;
}

// Helper function to render filters list with limit
function renderFiltersList(filters) {
  if (!filters || filters.length === 0) {
    return '<span class="text-muted">None</span>';
  }

  const limit = 3;
  const visibleFilters = filters.slice(0, limit);
  const remaining = filters.length - limit;

  let html = '<div class="filters-list">';

  visibleFilters.forEach((filter, index) => {
    const name = filter.filter_name || filter.name || "Unknown";
    const type = filter.is_active === false ? "🔴" : "🟢";
    html += `<div class="filter-item text-muted">
                <small>${type} ${escapeHtml(name)}</small>
             </div>`;
  });

  if (remaining > 0) {
    html += `<div class="filter-item text-muted">
                <small><em>+${remaining} more</em></small>
             </div>`;
  }

  html += "</div>";
  return html;
}

// Helper function to render proxy status
function renderProxyStatusCell(proxy) {
  // More robust active status check to handle different data types
  const isActive =
    proxy.is_active === true ||
    proxy.is_active === "true" ||
    proxy.is_active === 1;
  console.log(
    `Rendering status for proxy ${proxy.name}: is_active = ${proxy.is_active} (type: ${typeof proxy.is_active}), calculated isActive = ${isActive}`,
  );
  let statusBadge = `<span class="badge badge-${isActive ? "success" : "secondary"}">${isActive ? "Active" : "Inactive"}</span>`;

  if (proxy.last_generated_at) {
    const lastGenerated = new Date(proxy.last_generated_at);
    const now = new Date();
    const timeDiff = now - lastGenerated;
    const daysDiff = Math.floor(timeDiff / (1000 * 60 * 60 * 24));

    let timeText = "";
    if (daysDiff === 0) {
      timeText = "Today";
    } else if (daysDiff === 1) {
      timeText = "Yesterday";
    } else if (daysDiff < 7) {
      timeText = `${daysDiff} days ago`;
    } else {
      timeText = lastGenerated.toLocaleDateString();
    }

    statusBadge += `<br><small class="text-muted">Generated ${timeText}</small>`;
  } else {
    statusBadge += `<br><small class="text-muted">Never generated</small>`;
  }

  return statusBadge;
}

// Helper function to render proxy actions
function renderProxyActionsCell(proxy) {
  // Convert UUID to base64 for shorter URLs
  const base64Id = uuidToBase64(proxy.id);

  return `
        <div class="btn-group-vertical" role="group">
            <div class="btn-group mb-1" role="group">
                <button class="btn btn-sm btn-outline-primary" onclick="previewProxy('${proxy.id}')" title="Preview">
                    👁️
                </button>
                <button class="btn btn-sm btn-outline-secondary" onclick="editProxy('${proxy.id}')" title="Edit">
                    ✏️
                </button>
                <button class="btn btn-sm btn-outline-success" onclick="regenerateProxy('${proxy.id}')" title="Regenerate">
                    🔄
                </button>
                <button class="btn btn-sm btn-outline-danger" onclick="deleteProxy('${proxy.id}')" title="Delete">
                    🗑️
                </button>
            </div>
            <div class="btn-group" role="group">
                <a href="/proxy/${base64Id}/m3u8" target="_blank" class="btn btn-sm btn-outline-info" title="M3U Playlist">
                    📺 M3U
                </a>
                <a href="/proxy/${base64Id}/xmltv" target="_blank" class="btn btn-sm btn-outline-info" title="XMLTV EPG">
                    📅 EPG
                </a>
            </div>
        </div>
    `;
}

// Helper function to escape HTML (if SharedUtils.escapeHtml not available)
function escapeHtml(text) {
  if (typeof SharedUtils !== "undefined" && SharedUtils.escapeHtml) {
    return SharedUtils.escapeHtml(text);
  }
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
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
  document.getElementById("proxyCacheLogos").checked = true;
  document.getElementById("proxyCacheProgramLogos").checked = false;

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
  document.getElementById("proxyAutoRegenerate").checked =
    proxy.auto_regenerate || false;
  document.getElementById("proxyCacheLogos").checked =
    proxy.cache_channel_logos !== undefined ? proxy.cache_channel_logos : true;
  document.getElementById("proxyCacheProgramLogos").checked =
    proxy.cache_program_logos !== undefined ? proxy.cache_program_logos : false;

  // Set streaming configuration
  document.getElementById("proxyMode").value = proxy.proxy_mode || "redirect";
  document.getElementById("upstreamTimeout").value =
    proxy.upstream_timeout || 30;
  document.getElementById("bufferSize").value = proxy.buffer_size || 8192;
  document.getElementById("maxConcurrentStreams").value =
    proxy.max_concurrent_streams || 1;
  document.getElementById("startingChannelNumber").value =
    proxy.starting_channel_number || 1;

  // Reset and populate priority selections
  // Transform proxy response format to selection format
  selectedStreamSources = (proxy.stream_sources || []).map((source) => ({
    id: source.source_id,
    name: source.source_name,
    priority_order: source.priority_order,
  }));

  selectedEpgSources = (proxy.epg_sources || []).map((source) => ({
    id: source.epg_source_id,
    name: source.epg_source_name,
    priority_order: source.priority_order,
  }));

  selectedFilters = (proxy.filters || []).map((filter) => ({
    id: filter.filter_id,
    name: filter.filter_name,
    priority_order: filter.priority_order,
    is_active: filter.is_active,
    is_inverse: filter.is_inverse,
    source_type: filter.source_type,
    starting_channel_number: filter.starting_channel_number,
  }));

  // Re-render all priority lists
  renderStreamSources();
  renderEpgSources();
  renderFilters();
}

// Save proxy
async function saveProxy() {
  const form = document.getElementById("proxyForm");
  const formData = new FormData(form);

  // Build stream sources array with priority order
  const streamSources = selectedStreamSources.map((source, index) => ({
    source_id: source.id,
    priority_order: index + 1,
  }));

  // Build EPG sources array with priority order
  const epgSources = selectedEpgSources.map((source, index) => ({
    epg_source_id: source.id,
    priority_order: index + 1,
  }));

  // Build filters array with priority order - skip if no filters selected
  // Note: Filter functionality is not yet implemented in backend
  const filters =
    selectedFilters.length > 0
      ? selectedFilters.map((filterItem, index) => {
          const filter = filterItem.filter || filterItem;
          return {
            filter_id: filter.id,
            priority_order: index + 1,
            is_active: true,
          };
        })
      : [];

  const proxyData = {
    name: formData.get("name"),
    description: formData.get("description") || null,
    proxy_mode: formData.get("proxy_mode") || "redirect",
    upstream_timeout: formData.get("upstream_timeout")
      ? parseInt(formData.get("upstream_timeout"))
      : null,
    buffer_size: formData.get("buffer_size")
      ? parseInt(formData.get("buffer_size"))
      : null,
    max_concurrent_streams: formData.get("max_concurrent_streams")
      ? parseInt(formData.get("max_concurrent_streams"))
      : null,
    starting_channel_number: formData.get("starting_channel_number")
      ? parseInt(formData.get("starting_channel_number"))
      : 1,
    stream_sources: streamSources,
    epg_sources: epgSources,
    filters: filters,
    is_active: formData.has("is_active"),
    auto_regenerate: formData.has("auto_regenerate"),
    cache_channel_logos: formData.has("cache_channel_logos"),
    cache_program_logos: formData.has("cache_program_logos"),
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

    const responseData = await response.json();

    // Debug logging to check response structure
    console.log("Saved proxy response:", responseData);

    // Extract the actual proxy data from the wrapped response
    const savedProxy = responseData.data || responseData;

    console.log("Extracted proxy data:", savedProxy);
    console.log("Is active field:", savedProxy.is_active);
    console.log("Auto regenerate field:", savedProxy.auto_regenerate);

    if (editingProxy) {
      // Update existing proxy in the list
      const index = currentProxies.findIndex((p) => p.id === editingProxy.id);
      if (index !== -1) {
        console.log("Updating proxy at index:", index);
        console.log("Old proxy:", currentProxies[index]);
        currentProxies[index] = savedProxy;
        console.log("New proxy:", currentProxies[index]);
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
    SharedUtils.showSuccess(
      `Regenerated ${result.count} proxies successfully.`,
    );

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
      name: formData.get("name") || "Preview",
      description: formData.get("description") || null,
      proxy_mode: formData.get("proxy_mode") || "direct",
      upstream_timeout: formData.get("upstream_timeout")
        ? parseInt(formData.get("upstream_timeout"))
        : null,
      buffer_size: formData.get("buffer_size")
        ? parseInt(formData.get("buffer_size"))
        : null,
      max_concurrent_streams: formData.get("max_concurrent_streams")
        ? parseInt(formData.get("max_concurrent_streams"))
        : null,
      starting_channel_number: formData.get("starting_channel_number")
        ? parseInt(formData.get("starting_channel_number"))
        : 1,
      stream_sources: [
        {
          source_id: formData.get("source_id"),
          priority_order: 1,
        },
      ],
      epg_sources: [],
      filters: filterIds.map((id, index) => ({
        filter_id: id,
        priority_order: index + 1,
        is_active: true,
      })),
    };

    await generatePreview(previewData);
  } else {
    // Preview existing proxy
    const proxy = currentProxies.find((p) => p.id === proxyId);
    if (!proxy) return;

    // Transform proxy object to PreviewProxyRequest format
    const previewData = {
      name: proxy.name,
      description: proxy.description,
      proxy_mode: proxy.proxy_mode || "direct",
      upstream_timeout: proxy.upstream_timeout,
      buffer_size: proxy.buffer_size,
      max_concurrent_streams: proxy.max_concurrent_streams,
      starting_channel_number: proxy.starting_channel_number || 1,
      stream_sources: (proxy.stream_sources || []).map((source, index) => ({
        source_id: source.source_id,
        priority_order: source.priority_order || index + 1,
      })),
      epg_sources: (proxy.epg_sources || []).map((epg, index) => ({
        epg_source_id: epg.epg_source_id,
        priority_order: epg.priority_order || index + 1,
      })),
      filters: (proxy.filters || []).map((filter, index) => ({
        filter_id: filter.filter_id,
        priority_order: filter.priority_order || index + 1,
        is_active: filter.is_active !== undefined ? filter.is_active : true,
      })),
    };

    await generatePreview(previewData);
  }
}

// Generate preview data
async function generatePreview(proxyData) {
  try {
    console.log("Sending preview request with data:", proxyData);

    // Show loading modal immediately
    showPreviewModal(true); // Pass true to indicate loading state

    const response = await fetch("/api/v1/proxies/preview", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(proxyData),
    });

    if (!response.ok) {
      const errorText = await response.text();
      console.error("Preview API error:", response.status, errorText);
      throw new Error(
        `Failed to generate preview: ${response.status} ${response.statusText}`,
      );
    }

    previewData = await response.json();

    // Update modal with actual data
    showPreviewModal(false); // Pass false to indicate data loaded
  } catch (error) {
    console.error("Error generating preview:", error);
    SharedUtils.hideStandardModal("proxyPreviewModal");

    // Show more detailed error message
    const errorMessage = error.message.includes("Failed to fetch")
      ? "Server connection failed. Please check if the server is running and try again."
      : `Preview generation failed: ${error.message}`;

    SharedUtils.showError(errorMessage);
  }
}

// Show preview modal
function showPreviewModal(isLoading = false) {
  if (isLoading) {
    // Show modal with loading state
    document.getElementById("channelsPreview").innerHTML = `
      <div class="loading-state">
        <div class="spinner"></div>
        <p>Generating preview...</p>
      </div>
    `;
    document.getElementById("m3uContent").innerHTML = `
      <div class="loading-state">
        <div class="spinner"></div>
        <p>Loading M3U content...</p>
      </div>
    `;
    document.getElementById("proxyStats").innerHTML = `
      <div class="loading-state">
        <div class="spinner"></div>
        <p>Loading statistics...</p>
      </div>
    `;

    // Show channels tab by default
    showPreviewTab("channels");
    SharedUtils.showStandardModal("proxyPreviewModal");
    return;
  }

  if (!previewData) return;

  // Update with actual data
  updatePreviewContent();

  // Show channels tab by default if not already shown
  if (!document.querySelector(".tab-button.active")) {
    showPreviewTab("channels");
  }

  // Modal should already be visible from loading state
  SharedUtils.showStandardModal("proxyPreviewModal");
}

// Update preview content with actual data
function updatePreviewContent() {
  if (!previewData) return;

  // Update channels content
  updateChannelsContent();

  // Update M3U content
  updateM3uContent();

  // Update statistics content
  updateStatsContent();

  // Initialize EPG date to today
  initializeEpgControls();
}

// Update channels content
function updateChannelsContent() {
  const channelsContainer = document.getElementById("channelsPreview");
  if (!channelsContainer) return;

  const data = previewData.data || previewData;
  const channels = data.channels || data.preview_channels || [];
  const stats = data.stats || {};

  // Update group filter options
  populateGroupFilter();

  // Update inline stats badges
  const totalChannelsBadge = document.getElementById("totalChannelsBadge");
  const uniqueGroupsBadge = document.getElementById("uniqueGroupsBadge");
  const appliedFiltersBadge = document.getElementById("appliedFiltersBadge");

  if (totalChannelsBadge) {
    totalChannelsBadge.textContent = `${stats.total_channels || stats.filtered_channels || channels.length} channels`;
  }
  if (uniqueGroupsBadge) {
    uniqueGroupsBadge.textContent = `${Object.keys(stats.channels_by_group || {}).length} groups`;
  }
  if (appliedFiltersBadge) {
    appliedFiltersBadge.textContent = `${stats.applied_filters?.length || 0} filters`;
  }

  let html = "";

  if (channels.length === 0) {
    html += '<div class="no-results">No channels found</div>';
  } else {
    channels.forEach((channel, index) => {
      html += renderChannelItem(channel, index);
    });
  }

  channelsContainer.innerHTML = html;
}

function renderChannelItem(channel, index) {
  const channelData = channel.channel || channel;

  let html = `
    <div class="channel-item"
         data-channel-name="${escapeHtml(channelData.channel_name || "")}"
         data-group="${escapeHtml(channelData.group_title || channelData.channel_group || "")}">
      <div class="channel-header">
        <span class="channel-title">${escapeHtml(channelData.channel_name || "Unknown Channel")}</span>
        <span class="channel-number">#${channelData.channel_number || index + 1}</span>
      </div>
      <div class="channel-details">
  `;

  // Add channel fields if they exist
  const fields = [
    {
      label: "Group",
      value: channelData.group_title || channelData.channel_group,
    },
    { label: "TVG ID", value: channelData.tvg_id || channelData.channel_id },
    {
      label: "TVG Logo",
      value: channelData.tvg_logo || channelData.channel_logo,
    },
    { label: "Stream URL", value: channelData.stream_url || channelData.url },
    { label: "Channel Number", value: channelData.tvg_chno },
    {
      label: "Language",
      value: channelData.tvg_language || channelData.language,
    },
    { label: "Country", value: channelData.tvg_country || channelData.country },
    { label: "Time Shift", value: channelData.tvg_shift },
    { label: "Group Logo", value: channelData.group_logo },
    { label: "Radio", value: channelData.radio },
    { label: "Source", value: channelData.source_name },
  ];

  fields.forEach((field) => {
    if (field.value) {
      const displayValue =
        field.label === "Stream URL" ||
        field.label === "TVG Logo" ||
        field.label === "Group Logo"
          ? `<code>${escapeHtml(field.value)}</code>`
          : escapeHtml(field.value);

      html += `
        <div class="channel-field">
          <span class="field-label">${field.label}:</span>
          <span class="field-value">${displayValue}</span>
        </div>
      `;
    }
  });

  html += `
      </div>
    </div>
  `;

  return html;
}

// Update M3U content
function updateM3uContent() {
  const m3uContainer = document.getElementById("m3uContent");
  if (!m3uContainer) return;

  const data = previewData.data || previewData;
  const m3uContent = data.m3u_content || "";

  m3uContainer.textContent = m3uContent;
}

// Update statistics content
function updateStatsContent() {
  // Update all stats tabs
  updateOverviewStats();
  updatePipelineStats();
  updateMemoryStats();
  updateProcessingStats();
}

function updateOverviewStats() {
  const statsContainer = document.getElementById("proxyStats");
  if (!statsContainer) return;

  const data = previewData.data || previewData;
  const stats = data.stats || {};

  let html = `
    <div class="stat-item">
      <span class="stat-label">Total Sources:</span>
      <span class="stat-value">${stats.total_sources || 0}</span>
    </div>
    <div class="stat-item">
      <span class="stat-label">Channels Before Filters:</span>
      <span class="stat-value">${stats.total_channels_before_filters || 0}</span>
    </div>
    <div class="stat-item">
      <span class="stat-label">Channels After Filters:</span>
      <span class="stat-value">${stats.total_channels_after_filters || stats.filtered_channels || 0}</span>
    </div>
    <div class="stat-item">
      <span class="stat-label">Applied Filters:</span>
      <span class="stat-value">${stats.applied_filters?.length || 0}</span>
    </div>
    <div class="stat-item">
      <span class="stat-label">Channels by Source:</span>
      <span class="stat-value">${Object.keys(stats.channels_by_source || {}).length}</span>
    </div>
    <div class="stat-item">
      <span class="stat-label">Channel Groups:</span>
      <span class="stat-value">${Object.keys(stats.channels_by_group || {}).length}</span>
    </div>
  `;

  if (stats.applied_filters && stats.applied_filters.length > 0) {
    html += `
      <div class="stat-item full-width">
        <span class="stat-label">Applied Filters:</span>
        <div class="stat-list">
          ${stats.applied_filters.map((filter) => `<span class="badge badge-primary">${escapeHtml(filter)}</span>`).join(" ")}
        </div>
      </div>
    `;
  }

  if (
    stats.channels_by_group &&
    Object.keys(stats.channels_by_group).length > 0
  ) {
    html += `
      <div class="stat-item full-width">
        <span class="stat-label">Channels by Group:</span>
        <div class="stat-list">
          ${Object.entries(stats.channels_by_group)
            .map(
              ([group, count]) =>
                `<span class="badge badge-secondary">${escapeHtml(group)}: ${count}</span>`,
            )
            .join(" ")}
        </div>
      </div>
    `;
  }

  if (
    stats.channels_by_source &&
    Object.keys(stats.channels_by_source).length > 0
  ) {
    html += `
      <div class="stat-item full-width">
        <span class="stat-label">Channels by Source:</span>
        <div class="stat-list">
          ${Object.entries(stats.channels_by_source)
            .map(
              ([source, count]) =>
                `<span class="badge badge-info">${escapeHtml(source)}: ${count}</span>`,
            )
            .join(" ")}
        </div>
      </div>
    `;
  }

  statsContainer.innerHTML = html;
}

function updatePipelineStats() {
  const container = document.getElementById("pipelineStatsContent");
  if (!container) return;

  const data = previewData.data || previewData;
  const stats = data.stats || {};

  let html = `
    <div class="stat-item">
      <span class="stat-label">Pipeline Stages:</span>
      <span class="stat-value">${stats.pipeline_stages || "N/A"}</span>
    </div>
    <div class="stat-item">
      <span class="stat-label">Filter Execution Time:</span>
      <span class="stat-value">${stats.filter_execution_time || "N/A"}</span>
    </div>
    <div class="stat-item">
      <span class="stat-label">Channel Processing Rate:</span>
      <span class="stat-value">${stats.processing_rate || "N/A"}</span>
    </div>
    <div class="stat-item">
      <span class="stat-label">Memory Peak:</span>
      <span class="stat-value">${stats.memory_peak || "N/A"}</span>
    </div>
  `;

  if (stats.pipeline_stages_detail) {
    html += `
      <div class="stat-item full-width">
        <span class="stat-label">Pipeline Stages Detail:</span>
        <div class="pipeline-stages">
          ${stats.pipeline_stages_detail
            .map(
              (stage) => `
            <div class="pipeline-stage">
              <span class="stage-name">${escapeHtml(stage.name)}</span>
              <span class="stage-duration">${stage.duration}ms</span>
            </div>
          `,
            )
            .join("")}
        </div>
      </div>
    `;
  }

  container.innerHTML = html;
}

function updateMemoryStats() {
  const container = document.getElementById("memoryStatsContent");
  if (!container) return;

  const data = previewData.data || previewData;
  const stats = data.stats || {};

  let html = `
    <div class="stat-item">
      <span class="stat-label">Current Memory Usage:</span>
      <span class="stat-value">${formatFileSize(stats.current_memory || 0)}</span>
    </div>
    <div class="stat-item">
      <span class="stat-label">Peak Memory Usage:</span>
      <span class="stat-value">${formatFileSize(stats.peak_memory || 0)}</span>
    </div>
    <div class="stat-item">
      <span class="stat-label">Memory Efficiency:</span>
      <span class="stat-value">${stats.memory_efficiency || "N/A"}</span>
    </div>
    <div class="stat-item">
      <span class="stat-label">GC Collections:</span>
      <span class="stat-value">${stats.gc_collections || "N/A"}</span>
    </div>
  `;

  if (stats.memory_by_stage) {
    html += `
      <div class="stat-item full-width">
        <span class="stat-label">Memory by Stage:</span>
        <div class="memory-stages">
          ${Object.entries(stats.memory_by_stage)
            .map(
              ([stage, memory]) => `
            <div class="memory-stage">
              <span class="stage-name">${escapeHtml(stage)}</span>
              <span class="stage-memory">${formatFileSize(memory)}</span>
            </div>
          `,
            )
            .join("")}
        </div>
      </div>
    `;
  }

  container.innerHTML = html;
}

function updateProcessingStats() {
  const container = document.getElementById("processingStatsContent");
  if (!container) return;

  const data = previewData.data || previewData;
  const stats = data.stats || {};

  let html = `
    <div class="stat-item">
      <span class="stat-label">Total Processing Time:</span>
      <span class="stat-value">${stats.total_processing_time || "N/A"}</span>
    </div>
    <div class="stat-item">
      <span class="stat-label">Average Channel Time:</span>
      <span class="stat-value">${stats.avg_channel_time || "N/A"}</span>
    </div>
    <div class="stat-item">
      <span class="stat-label">Throughput:</span>
      <span class="stat-value">${stats.throughput || "N/A"}</span>
    </div>
    <div class="stat-item">
      <span class="stat-label">Errors:</span>
      <span class="stat-value">${stats.errors || 0}</span>
    </div>
  `;

  if (stats.processing_timeline) {
    html += `
      <div class="stat-item full-width">
        <span class="stat-label">Processing Timeline:</span>
        <div class="processing-timeline">
          ${stats.processing_timeline
            .map(
              (event) => `
            <div class="timeline-event">
              <span class="event-time">${new Date(event.timestamp).toLocaleTimeString()}</span>
              <span class="event-description">${escapeHtml(event.description)}</span>
            </div>
          `,
            )
            .join("")}
        </div>
      </div>
    `;
  }

  container.innerHTML = html;
}

// Filter preview channels
function filterPreviewChannels() {
  const searchTerm =
    document.getElementById("channelSearch")?.value.toLowerCase() || "";
  const groupFilter = document.getElementById("groupFilter")?.value || "";
  const channelItems = document.querySelectorAll(".channel-item");

  channelItems.forEach((item) => {
    const channelName = item.dataset.channelName?.toLowerCase() || "";
    const group = item.dataset.group?.toLowerCase() || "";

    const searchMatches =
      !searchTerm ||
      channelName.includes(searchTerm) ||
      group.includes(searchTerm);
    const groupMatches = !groupFilter || group === groupFilter.toLowerCase();

    item.style.display = searchMatches && groupMatches ? "block" : "none";
  });
}

// Copy M3U content to clipboard
function copyM3uContent() {
  const content = document.getElementById("m3uContent")?.textContent || "";
  navigator.clipboard
    .writeText(content)
    .then(() => {
      SharedUtils.showSuccess("M3U content copied to clipboard");
    })
    .catch(() => {
      SharedUtils.showError("Failed to copy content");
    });
}

// Download M3U content
function downloadM3u() {
  const content = document.getElementById("m3uContent")?.textContent || "";
  const blob = new Blob([content], { type: "application/vnd.apple.mpegurl" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = "preview.m3u";
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

// Close proxy preview modal
function closeProxyPreview() {
  SharedUtils.hideStandardModal("proxyPreviewModal");
  previewData = null;
}

// Show preview tab
function showPreviewTab(tabName) {
  // Update tab buttons
  document.querySelectorAll(".tab-button").forEach((btn) => {
    btn.classList.remove("active");
  });
  document
    .querySelector(`[onclick="showPreviewTab('${tabName}')"]`)
    ?.classList.add("active");

  // Update tab content
  document.querySelectorAll(".tab-content").forEach((content) => {
    content.classList.remove("active");
  });
  document.getElementById(tabName + "Tab")?.classList.add("active");

  // Populate group filter if on channels tab
  if (tabName === "channels" && previewData) {
    populateGroupFilter();
  }
}

// Populate group filter dropdown
function populateGroupFilter() {
  const groupFilter = document.getElementById("groupFilter");
  if (!groupFilter) return;

  const data = previewData.data || previewData;
  const channels = data.preview_channels || [];

  // Get unique groups
  const groups = [
    ...new Set(channels.map((ch) => ch.group_title).filter((g) => g)),
  ];

  // Clear existing options except "All Groups"
  groupFilter.innerHTML = '<option value="">All Groups</option>';

  // Add group options
  groups.forEach((group) => {
    const option = document.createElement("option");
    option.value = group;
    option.textContent = group;
    groupFilter.appendChild(option);
  });
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
    container.innerHTML =
      '<p class="text-muted small">No stream sources selected</p>';
    return;
  }

  let html = "";
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
  setupDragAndDrop("streamSourcesList", "stream");
}

// Render priority cards for EPG sources
function renderEpgSources() {
  const container = document.getElementById("epgSourcesList");
  if (!container) return;

  if (selectedEpgSources.length === 0) {
    container.innerHTML =
      '<p class="text-muted small">No EPG sources selected</p>';
    return;
  }

  let html = "";
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
  setupDragAndDrop("epgSourcesList", "epg");
}

// Render priority cards for filters
function renderFilters() {
  const container = document.getElementById("filtersList");
  if (!container) return;

  if (selectedFilters.length === 0) {
    container.innerHTML = '<p class="text-muted small">No filters selected</p>';
    return;
  }

  let html = "";
  selectedFilters.forEach((item, index) => {
    const filter = item.filter || item; // Handle nested filter object
    const title = filter.description
      ? `${SharedUtils.escapeHtml(filter.name)}: ${SharedUtils.escapeHtml(filter.description)}`
      : SharedUtils.escapeHtml(filter.name);
    const type = filter.is_inverse ? "EXCLUDE" : "INCLUDE";
    html += `
      <div class="priority-card" draggable="true" data-type="filter" data-id="${filter.id}" data-index="${index}">
        <div class="priority-card-header">
          <span class="drag-handle">⋯</span>
          <span class="priority-card-title">${title} <sup class="text-muted">${type}</sup></span>
          <span class="priority-card-stats">${filter.source_type || "Filter"}</span>
          <button class="detach-btn" onclick="removeFilter(${index})" title="Remove filter">×</button>
        </div>
      </div>
    `;
  });

  container.innerHTML = html;
  setupDragAndDrop("filtersList", "filter");
}

// Setup drag and drop for a container
function setupDragAndDrop(containerId, type) {
  const container = document.getElementById(containerId);
  if (!container) return;

  const cards = container.querySelectorAll(".priority-card");

  cards.forEach((card) => {
    card.addEventListener("dragstart", (e) => {
      card.classList.add("dragging");
      e.dataTransfer.setData("text/plain", "");
      e.dataTransfer.effectAllowed = "move";
    });

    card.addEventListener("dragend", () => {
      card.classList.remove("dragging");
      container.classList.remove("drag-over");
    });
  });

  container.addEventListener("dragover", (e) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";

    const dragging = container.querySelector(".dragging");
    if (!dragging) return;

    const siblings = [
      ...container.querySelectorAll(".priority-card:not(.dragging)"),
    ];
    const nextSibling = siblings.find((sibling) => {
      return (
        e.clientY <=
        sibling.getBoundingClientRect().top + sibling.offsetHeight / 2
      );
    });

    container.insertBefore(dragging, nextSibling);
  });

  container.addEventListener("drop", (e) => {
    e.preventDefault();
    container.classList.remove("drag-over");

    // Update the array order based on the new DOM order
    const cards = [...container.querySelectorAll(".priority-card")];
    const newOrder = cards.map((card) => parseInt(card.dataset.index));

    if (type === "stream") {
      selectedStreamSources = newOrder.map(
        (index) => selectedStreamSources[index],
      );
      renderStreamSources();
    } else if (type === "epg") {
      selectedEpgSources = newOrder.map((index) => selectedEpgSources[index]);
      renderEpgSources();
    } else if (type === "filter") {
      selectedFilters = newOrder.map((index) => selectedFilters[index]);
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
  console.log("Opening stream source modal...");
  console.log("Available stream sources:", availableStreamSources.length);
  console.log("Selected stream sources:", selectedStreamSources.length);

  populateStreamSourceSelection();
  SharedUtils.showStandardModal("addStreamSourceModal");
  console.log("Modal opened using SharedUtils");
}

function openAddEpgSourceModal() {
  populateEpgSourceSelection();
  SharedUtils.showStandardModal("addEpgSourceModal");
}

function openAddFilterModal() {
  populateFilterSelection();
  SharedUtils.showStandardModal("addFilterModal");
}

function closeAddStreamSourceModal() {
  SharedUtils.hideStandardModal("addStreamSourceModal");
}

function closeAddEpgSourceModal() {
  SharedUtils.hideStandardModal("addEpgSourceModal");
}

function closeAddFilterModal() {
  SharedUtils.hideStandardModal("addFilterModal");
}

// Populate selection modals
function populateStreamSourceSelection() {
  const container = document.getElementById("streamSourcesSelectionList");
  console.log("Container found:", container);
  if (!container) {
    console.error("Stream sources selection container not found!");
    return;
  }

  const alreadySelected = selectedStreamSources.map((s) => s.id);
  const available = availableStreamSources.filter(
    (s) => !alreadySelected.includes(s.id),
  );

  console.log("Already selected IDs:", alreadySelected);
  console.log("Available sources after filtering:", available.length);

  if (available.length === 0) {
    console.log("No available sources - showing message");
    container.innerHTML =
      '<p class="text-muted">All available stream sources are already selected</p>';
    return;
  }

  let html = "";
  available.forEach((source) => {
    const title = source.description
      ? `${SharedUtils.escapeHtml(source.name)}: ${SharedUtils.escapeHtml(source.description)}`
      : SharedUtils.escapeHtml(source.name);
    const stats = `${source.channel_count || 0} channels`;
    const isSelected = selectedStreamSources.some((s) => s.id === source.id);
    html += `
      <div class="source-selection-item ${isSelected ? "selected" : ""}" onclick="toggleStreamSourceSelection('${source.id}')">
        <input type="checkbox" id="stream_${source.id}" ${isSelected ? "checked" : ""} onchange="toggleStreamSourceSelection('${source.id}')" />
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
  const container = document.getElementById("epgSourcesSelectionList");
  if (!container) return;

  const alreadySelected = selectedEpgSources.map((s) => s.id);
  const available = availableEpgSources.filter(
    (s) => !alreadySelected.includes(s.id),
  );

  if (available.length === 0) {
    container.innerHTML =
      '<p class="text-muted">All available EPG sources are already selected</p>';
    return;
  }

  let html = "";
  available.forEach((source) => {
    const title = source.description
      ? `${SharedUtils.escapeHtml(source.name)}: ${SharedUtils.escapeHtml(source.description)}`
      : SharedUtils.escapeHtml(source.name);
    const stats = `${source.program_count || 0} programs`;
    const isSelected = selectedEpgSources.some((s) => s.id === source.id);
    html += `
      <div class="source-selection-item ${isSelected ? "selected" : ""}" onclick="toggleEpgSourceSelection('${source.id}')">
        <input type="checkbox" id="epg_${source.id}" ${isSelected ? "checked" : ""} onchange="toggleEpgSourceSelection('${source.id}')" />
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
  const container = document.getElementById("filtersSelectionList");
  if (!container) return;

  const alreadySelected = selectedFilters.map((f) => f.id);
  const available = availableFilters.filter(
    (f) => !alreadySelected.includes(f.id),
  );

  if (available.length === 0) {
    container.innerHTML =
      '<p class="text-muted">All available filters are already selected</p>';
    return;
  }

  let html = "";
  available.forEach((item) => {
    const filter = item.filter || item; // Handle nested filter object
    const title = filter.description
      ? `${SharedUtils.escapeHtml(filter.name)}: ${SharedUtils.escapeHtml(filter.description)}`
      : SharedUtils.escapeHtml(filter.name);
    const type = filter.is_inverse ? "EXCLUDE" : "INCLUDE";
    const isSelected = selectedFilters.some((f) => f.id === filter.id);
    html += `
      <div class="source-selection-item ${isSelected ? "selected" : ""}" onclick="toggleFilterSelection('${filter.id}')">
        <input type="checkbox" id="filter_${filter.id}" ${isSelected ? "checked" : ""} onchange="toggleFilterSelection('${filter.id}')" />
        <div class="source-selection-title">${title} <sup class="text-muted">${type}</sup></div>
        <div class="source-selection-stats">${filter.source_type || "stream"}</div>
      </div>
    `;
  });

  container.innerHTML = html;
}

// Toggle selection functions
function toggleStreamSourceSelection(sourceId) {
  const source = availableStreamSources.find((s) => s.id === sourceId);
  if (!source) return;

  const existingIndex = selectedStreamSources.findIndex(
    (s) => s.id === sourceId,
  );
  if (existingIndex >= 0) {
    selectedStreamSources.splice(existingIndex, 1);
  } else {
    selectedStreamSources.push(source);
  }

  // Update UI
  const checkbox = document.getElementById(`stream_${sourceId}`);
  const item = checkbox.closest(".source-selection-item");
  if (existingIndex >= 0) {
    checkbox.checked = false;
    item.classList.remove("selected");
  } else {
    checkbox.checked = true;
    item.classList.add("selected");
  }
}

function toggleEpgSourceSelection(sourceId) {
  const source = availableEpgSources.find((s) => s.id === sourceId);
  if (!source) return;

  const existingIndex = selectedEpgSources.findIndex((s) => s.id === sourceId);
  if (existingIndex >= 0) {
    selectedEpgSources.splice(existingIndex, 1);
  } else {
    selectedEpgSources.push(source);
  }

  // Update UI
  const checkbox = document.getElementById(`epg_${sourceId}`);
  const item = checkbox.closest(".source-selection-item");
  if (existingIndex >= 0) {
    checkbox.checked = false;
    item.classList.remove("selected");
  } else {
    checkbox.checked = true;
    item.classList.add("selected");
  }
}

function toggleFilterSelection(filterId) {
  const filterItem = availableFilters.find((item) => {
    const filter = item.filter || item;
    return filter.id === filterId;
  });
  if (!filterItem) return;

  const filter = filterItem.filter || filterItem;
  const existingIndex = selectedFilters.findIndex((f) => {
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
  const item = checkbox.closest(".source-selection-item");
  if (existingIndex >= 0) {
    checkbox.checked = false;
    item.classList.remove("selected");
  } else {
    checkbox.checked = true;
    item.classList.add("selected");
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

  // Set default values
  document.getElementById("proxyActive").checked = true;
  document.getElementById("proxyAutoRegenerate").checked = true;
  document.getElementById("proxyCacheLogos").checked = true;
  document.getElementById("proxyCacheProgramLogos").checked = false;

  // Reset priority selections
  selectedStreamSources = [];
  selectedEpgSources = [];
  selectedFilters = [];

  // Re-render all priority lists
  renderStreamSources();
  renderEpgSources();
  renderFilters();
}

// Enhanced Preview Functions

function showStatsTab(tabName) {
  currentStatsTab = tabName;

  // Update tab buttons
  document.querySelectorAll(".stats-tab-button").forEach((btn) => {
    btn.classList.remove("active");
  });
  document
    .querySelector(`[onclick="showStatsTab('${tabName}')"]`)
    .classList.add("active");

  // Update tab content
  document.querySelectorAll(".stats-tab-content").forEach((content) => {
    content.classList.remove("active");
  });
  document.getElementById(`${tabName}Stats`).classList.add("active");
}

// EPG Functions

function initializeEpgControls() {
  // Set current date
  const today = new Date().toISOString().split("T")[0];
  document.getElementById("epgDateSelect").value = today;

  // Initialize tooltip
  epgTooltip = document.getElementById("epgTooltip");
}

async function loadEpgForChannels() {
  try {
    showEpgLoading();

    const data = previewData.data || previewData;
    const channels = data.channels || data.preview_channels || [];

    if (channels.length === 0) {
      showEpgNoData();
      return;
    }

    // Get EPG parameters
    const date = document.getElementById("epgDateSelect").value;
    const timeRange = parseInt(document.getElementById("epgTimeRange").value);

    const startTime = new Date(date + "T00:00:00");
    const endTime = new Date(startTime);
    endTime.setHours(endTime.getHours() + timeRange);

    // Get channel IDs for EPG lookup
    const channelIds = channels
      .map((ch) => (ch.channel || ch).tvg_id || (ch.channel || ch).channel_id)
      .filter((id) => id);

    if (channelIds.length === 0) {
      showEpgNoData();
      return;
    }

    const params = new URLSearchParams({
      start_time: startTime.toISOString(),
      end_time: endTime.toISOString(),
      channel_filter: channelIds.join(","),
    });

    const response = await fetch(`/api/v1/epg/viewer?${params}`);

    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }

    previewEpgData = await response.json();
    renderEpgGrid();
  } catch (error) {
    console.error("Error loading EPG data:", error);
    showEpgError("Failed to load EPG data: " + error.message);
  }
}

function renderEpgGrid() {
  if (!previewEpgData || previewEpgData.channels.length === 0) {
    showEpgNoData();
    return;
  }

  showEpgContent();
  renderEpgTimeline();
  renderEpgChannels();
}

function renderEpgTimeline() {
  const timeline = document.getElementById("epgTimeline");
  timeline.innerHTML = "";

  // Add channel info spacer
  const spacer = document.createElement("div");
  spacer.className = "epg-channel-spacer";
  spacer.style.minWidth = "200px";
  timeline.appendChild(spacer);

  // Generate time slots
  const timeRange = parseInt(document.getElementById("epgTimeRange").value);
  const startTime = new Date(previewEpgData.start_time);
  const endTime = new Date(startTime);
  endTime.setHours(endTime.getHours() + timeRange);

  const currentTime = new Date(startTime);
  while (currentTime < endTime) {
    const timeSlot = document.createElement("div");
    timeSlot.className = "epg-time-slot";
    timeSlot.textContent = formatTime(currentTime);
    timeline.appendChild(timeSlot);

    currentTime.setMinutes(currentTime.getMinutes() + 30);
  }
}

function renderEpgChannels() {
  const channelsContainer = document.getElementById("epgChannels");
  channelsContainer.innerHTML = "";

  const filteredChannels = filterEpgChannels();

  if (filteredChannels.length === 0) {
    const noChannels = document.createElement("div");
    noChannels.className = "text-center text-muted p-4";
    noChannels.textContent = "No channels match your filter";
    channelsContainer.appendChild(noChannels);
    return;
  }

  // Update channel count
  document.getElementById("epgChannelCount").textContent =
    `${filteredChannels.length} channels`;

  filteredChannels.forEach((channelData) => {
    const channelRow = createEpgChannelRow(channelData);
    channelsContainer.appendChild(channelRow);
  });
}

function createEpgChannelRow(channelData) {
  const row = document.createElement("div");
  row.className = "epg-channel-row";

  // Channel info
  const channelInfo = document.createElement("div");
  channelInfo.className = "epg-channel-info";
  channelInfo.innerHTML = `
    <div class="epg-channel-name">${escapeHtml(channelData.channel.channel_name)}</div>
    <div class="epg-channel-id">${escapeHtml(channelData.channel.channel_id)}</div>
  `;

  // Programs
  const programsContainer = document.createElement("div");
  programsContainer.className = "epg-programs";

  renderEpgPrograms(programsContainer, channelData.programs);

  row.appendChild(channelInfo);
  row.appendChild(programsContainer);

  return row;
}

function renderEpgPrograms(container, programs) {
  const timeRange = parseInt(document.getElementById("epgTimeRange").value);
  const startTime = new Date(previewEpgData.start_time);
  const endTime = new Date(startTime);
  endTime.setHours(endTime.getHours() + timeRange);

  // Sort programs by start time
  const sortedPrograms = programs.sort(
    (a, b) => new Date(a.start_time) - new Date(b.start_time),
  );

  const timeSlotDuration = 30; // 30 minutes per slot
  const currentTime = new Date(startTime);

  while (currentTime < endTime) {
    const slotEndTime = new Date(currentTime);
    slotEndTime.setMinutes(slotEndTime.getMinutes() + timeSlotDuration);

    // Find program that overlaps with this time slot
    const program = findProgramForTimeSlot(
      sortedPrograms,
      currentTime,
      slotEndTime,
    );

    if (program) {
      const programElement = createEpgProgramElement(
        program,
        currentTime,
        slotEndTime,
      );
      container.appendChild(programElement);
    } else {
      const emptySlot = createEpgEmptySlot();
      container.appendChild(emptySlot);
    }

    currentTime.setMinutes(currentTime.getMinutes() + timeSlotDuration);
  }
}

function findProgramForTimeSlot(programs, slotStart, slotEnd) {
  return programs.find((program) => {
    const programStart = new Date(program.start_time);
    const programEnd = new Date(program.end_time);

    // Program overlaps with time slot
    return programStart < slotEnd && programEnd > slotStart;
  });
}

function createEpgProgramElement(program, slotStart, slotEnd) {
  const programDiv = document.createElement("div");
  programDiv.className = "epg-program";

  // Check if program is currently airing
  const now = new Date();
  const programStart = new Date(program.start_time);
  const programEnd = new Date(program.end_time);

  if (now >= programStart && now <= programEnd) {
    programDiv.className += " current";
  }

  const timeText = `${formatTime(programStart)} - ${formatTime(programEnd)}`;

  programDiv.innerHTML = `
    <div class="epg-program-time">${timeText}</div>
    <div class="epg-program-title">${escapeHtml(program.program_title)}</div>
    ${
      program.program_category
        ? `<div class="epg-program-category">${escapeHtml(program.program_category)}</div>`
        : ""
    }
  `;

  // Add tooltip events
  programDiv.addEventListener("mouseenter", (e) => {
    showEpgTooltip(e, program);
  });

  programDiv.addEventListener("mouseleave", () => {
    hideEpgTooltip();
  });

  return programDiv;
}

function createEpgEmptySlot() {
  const emptyDiv = document.createElement("div");
  emptyDiv.className = "epg-empty-slot";
  emptyDiv.textContent = "No Program";
  return emptyDiv;
}

function filterEpgChannels() {
  if (!previewEpgData) return [];

  const searchTerm = document
    .getElementById("epgChannelSearch")
    .value.toLowerCase();

  return previewEpgData.channels.filter((channelData) => {
    if (!searchTerm) return true;

    const channel = channelData.channel;
    return (
      channel.channel_name.toLowerCase().includes(searchTerm) ||
      channel.channel_id.toLowerCase().includes(searchTerm)
    );
  });
}

function showEpgTooltip(event, program) {
  if (!epgTooltip) return;

  const programStart = new Date(program.start_time);
  const programEnd = new Date(program.end_time);
  const duration = Math.round((programEnd - programStart) / (1000 * 60)); // minutes

  let content = `
    <div><strong>${escapeHtml(program.program_title)}</strong></div>
    <div style="margin: 0.5rem 0;">
      ${formatTime(programStart)} - ${formatTime(programEnd)} (${duration} min)
    </div>
  `;

  if (program.program_description) {
    content += `<div style="margin: 0.5rem 0;">${escapeHtml(program.program_description)}</div>`;
  }

  if (program.program_category) {
    content += `<div style="margin: 0.5rem 0;"><strong>Category:</strong> ${escapeHtml(program.program_category)}</div>`;
  }

  if (program.episode_num || program.season_num) {
    let episodeInfo = "";
    if (program.season_num) episodeInfo += `Season ${program.season_num}`;
    if (program.episode_num) {
      if (episodeInfo) episodeInfo += ", ";
      episodeInfo += `Episode ${program.episode_num}`;
    }
    content += `<div style="margin: 0.5rem 0;"><strong>Episode:</strong> ${episodeInfo}</div>`;
  }

  if (program.rating) {
    content += `<div style="margin: 0.5rem 0;"><strong>Rating:</strong> ${escapeHtml(program.rating)}</div>`;
  }

  epgTooltip.innerHTML = content;

  // Position tooltip
  const rect = event.target.getBoundingClientRect();
  epgTooltip.style.left = `${rect.left + window.scrollX}px`;
  epgTooltip.style.top = `${rect.top + window.scrollY - epgTooltip.offsetHeight - 10}px`;

  // Show tooltip
  epgTooltip.style.display = "block";
  epgTooltip.classList.add("show");
}

function hideEpgTooltip() {
  if (epgTooltip) {
    epgTooltip.style.display = "none";
    epgTooltip.classList.remove("show");
  }
}

function formatTime(date) {
  return date.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

function showEpgLoading() {
  document.getElementById("epgLoading").style.display = "flex";
  document.getElementById("epgNoData").style.display = "none";
  document.getElementById("epgContent").style.display = "none";
}

function showEpgNoData() {
  document.getElementById("epgLoading").style.display = "none";
  document.getElementById("epgNoData").style.display = "flex";
  document.getElementById("epgContent").style.display = "none";
}

function showEpgContent() {
  document.getElementById("epgLoading").style.display = "none";
  document.getElementById("epgNoData").style.display = "none";
  document.getElementById("epgContent").style.display = "block";
}

function showEpgError(message) {
  console.error("EPG Error:", message);

  const epgPreview = document.getElementById("epgPreview");
  epgPreview.innerHTML = `
    <div class="alert alert-danger">
      <strong>Error loading EPG data:</strong> ${escapeHtml(message)}
    </div>
  `;
}

// Enhanced preview tab switching
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
  document.getElementById(`${tabName}Tab`).classList.add("active");

  // Load EPG data when EPG tab is shown
  if (tabName === "epg" && previewData && !previewEpgData) {
    loadEpgForChannels();
  }
}

// Enhanced group filter population
function populateGroupFilter() {
  const groupSelect = document.getElementById("groupFilter");
  if (!groupSelect) return;

  const data = previewData.data || previewData;
  const channels = data.channels || data.preview_channels || [];

  // Get unique groups
  const groups = [
    ...new Set(
      channels.map((ch) => {
        const channel = ch.channel || ch;
        return channel.group_title || channel.channel_group || "Uncategorized";
      }),
    ),
  ].sort();

  // Clear existing options (except "All Groups")
  groupSelect.innerHTML = '<option value="">All Groups</option>';

  // Add group options
  groups.forEach((group) => {
    const option = document.createElement("option");
    option.value = group;
    option.textContent = group;
    groupSelect.appendChild(option);
  });
}

// Enhanced channel filtering
function filterPreviewChannels() {
  const searchTerm = document
    .getElementById("channelSearch")
    .value.toLowerCase();
  const selectedGroup = document.getElementById("groupFilter").value;

  const channelItems = document.querySelectorAll(".channel-item");

  channelItems.forEach((item) => {
    const channelName = item.dataset.channelName.toLowerCase();
    const channelGroup = item.dataset.group;

    const matchesSearch = !searchTerm || channelName.includes(searchTerm);
    const matchesGroup = !selectedGroup || channelGroup === selectedGroup;

    if (matchesSearch && matchesGroup) {
      item.style.display = "block";
    } else {
      item.style.display = "none";
    }
  });
}

// Enhanced file size formatting
function formatFileSize(bytes) {
  if (bytes === 0) return "0 B";

  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));

  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

// Initialize enhanced features when page loads
document.addEventListener("DOMContentLoaded", function () {
  // Initialize EPG tooltip
  epgTooltip = document.getElementById("epgTooltip");

  // Add event listeners for EPG controls
  const epgChannelSearch = document.getElementById("epgChannelSearch");
  if (epgChannelSearch) {
    epgChannelSearch.addEventListener("input", () => {
      renderEpgChannels();
    });
  }

  // Hide tooltip when clicking outside
  document.addEventListener("click", function (e) {
    if (!e.target.closest(".epg-program")) {
      hideEpgTooltip();
    }
  });
});
