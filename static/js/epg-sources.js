// EPG Sources Management JavaScript

class EpgSourcesManager {
  constructor() {
    this.sources = [];
    this.editingSource = null;
    this.progressPollingInterval = null;
    this.progressData = {};
    this.processingInfo = {};
    this.lastHadProgress = false;
    this.currentChannels = [];
    this.filteredChannels = [];
    this.init();
  }

  async init() {
    this.setupEventListeners();
    await this.loadSources();
    this.startProgressPolling();
  }

  setupEventListeners() {
    // Add EPG source button
    document.getElementById("addEpgSourceBtn").addEventListener("click", () => {
      this.showSourceModal();
    });

    // Modal cancel
    document.getElementById("cancelEpgSource").addEventListener("click", () => {
      this.hideSourceModal();
    });

    // Modal save
    document.getElementById("saveEpgSource").addEventListener("click", () => {
      this.saveSource();
    });

    // Source type change
    document.getElementById("epgSourceType").addEventListener("change", (e) => {
      this.toggleSourceTypeFields(e.target.value);
    });

    // View EPG button
    const viewEpgBtn = document.getElementById("viewEpgBtn");
    if (viewEpgBtn) {
      viewEpgBtn.addEventListener("click", () => {
        this.showEpgViewerModal();
      });
    }

    // Channels filter
    const channelsFilter = document.getElementById("channelsFilter");
    if (channelsFilter) {
      channelsFilter.addEventListener("input", (e) => {
        this.filterChannels(e.target.value);
      });
    }
  }

  async loadSources() {
    try {
      this.showLoading();
      const response = await fetch("/api/v1/sources/epg");

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      this.sources = await response.json();
      this.renderSources();
    } catch (error) {
      console.error("Error loading EPG sources:", error);
      this.showAlert("Failed to load EPG sources", "error");
    } finally {
      this.hideLoading();
    }
  }

  renderSources() {
    const tbody = document.getElementById("epgSourcesTableBody");
    tbody.innerHTML = "";

    if (this.sources.length === 0) {
      tbody.innerHTML = `
                <tr>
                    <td colspan="6" class="text-center text-muted">
                        No EPG sources configured. Click "Add EPG Source" to get started.
                    </td>
                </tr>
            `;
      return;
    }

    this.sources.forEach((sourceWithStats) => {
      // Handle unified source structure
      const source = sourceWithStats.source || sourceWithStats;
      const channelCount = sourceWithStats.channel_count || 0;
      const programCount = sourceWithStats.program_count || 0;

      const row = document.createElement("tr");
      row.style.opacity = source.is_active ? "1" : "0.6";

      // Name and URL column
      const nameCell = document.createElement("td");
      const typeIndicator = source.source_type === "xmltv" ? "XML" : "XC";

      nameCell.innerHTML = `
                <div class="source-info">
                    <div class="source-name">
                        <strong>${this.escapeHtml(source.name)}<sup class="text-muted" style="font-size: 0.7em; margin-left: 3px;">${typeIndicator}</sup></strong>
                    </div>
                    <div class="source-url text-muted">
                        <small>${this.escapeHtml(source.url)}</small>
                    </div>
                    ${
                      source.timezone !== "UTC"
                        ? `<div class="source-timezone text-muted">
                            <small>🌍 ${source.timezone}${source.timezone_detected ? " (auto-detected)" : ""}</small>
                        </div>`
                        : ""
                    }
                    ${
                      source.time_offset !== "0"
                        ? `<div class="source-offset text-muted">
                            <small>⏰ ${source.time_offset}</small>
                        </div>`
                        : ""
                    }
                </div>
            `;

      // Channels column
      const channelsCell = document.createElement("td");
      channelsCell.innerHTML = `
                <div class="text-center">
                    <span class="badge badge-secondary">${channelCount}</span>
                    ${
                      channelCount > 0
                        ? `<br><small><a href="#" onclick="epgSourcesManager.showChannels('${source.id}', '${this.escapeHtml(source.name)}')">View</a></small>`
                        : ""
                    }
                </div>
            `;

      // Programs column
      const programsCell = document.createElement("td");
      programsCell.innerHTML = `
                <div class="text-center">
                    <span class="badge badge-info">${programCount}</span>
                </div>
            `;

      // Status column
      const statusCell = document.createElement("td");
      statusCell.innerHTML = this.renderStatusCell(source);

      // Refresh column
      const refreshCell = document.createElement("td");
      refreshCell.innerHTML = this.renderRefreshCell(source);

      // Actions column
      const actionsCell = document.createElement("td");
      actionsCell.innerHTML = this.renderActionsCell(source);

      row.appendChild(nameCell);
      row.appendChild(channelsCell);
      row.appendChild(programsCell);
      row.appendChild(statusCell);
      row.appendChild(refreshCell);
      row.appendChild(actionsCell);

      tbody.appendChild(row);
    });
  }

  renderStatusCell(source) {
    const isActive = source.is_active;
    const lastIngested = source.last_ingested_at;
    const progress = this.progressData[source.id];

    // Show progress state if actively processing
    if (progress && this.isActivelyProcessing(progress.state)) {
      const stateColors = {
        idle: "secondary",
        connecting: "primary",
        downloading: "primary",
        parsing: "primary",
        saving: "primary",
        processing: "primary",
        completed: "success",
        error: "danger",
      };

      const badgeColor = stateColors[progress.state] || "secondary";
      let statusBadge = `<span class="badge badge-${badgeColor}">${progress.state.charAt(0).toUpperCase() + progress.state.slice(1)}</span>`;

      if (progress.progress) {
        statusBadge += `<br><small class="text-muted">${this.formatProgressText(progress.progress)}</small>`;
      }

      return statusBadge;
    }

    // Default status display
    let statusBadge = `<span class="badge badge-${isActive ? "success" : "secondary"}">${isActive ? "Active" : "Inactive"}</span>`;

    if (lastIngested) {
      const lastIngestedDate = new Date(lastIngested);
      const now = new Date();
      const timeDiff = now - lastIngestedDate;
      const daysDiff = Math.floor(timeDiff / (1000 * 60 * 60 * 24));

      let timeText = "";
      if (daysDiff === 0) {
        timeText = "Today";
      } else if (daysDiff === 1) {
        timeText = "Yesterday";
      } else {
        timeText = `${daysDiff} days ago`;
      }

      statusBadge += `<br><small class="text-muted">Last: ${timeText}</small>`;
    } else {
      statusBadge += `<br><small class="text-muted">Never refreshed</small>`;
    }

    return statusBadge;
  }

  formatProgressText(progress) {
    let text = progress.current_step;

    // Simplify common messages
    if (text.includes("Parsing channels")) {
      const match = text.match(/\((\d+)\/(\d+)\)/);
      if (match) {
        const current = parseInt(match[1]).toLocaleString();
        const total = parseInt(match[2]).toLocaleString();
        text = `Channels: ${current}/${total}`;
      }
    } else if (text.includes("Parsing programmes")) {
      const match = text.match(/\((\d+)\/(\d+)\)/);
      if (match) {
        const current = parseInt(match[1]).toLocaleString();
        const total = parseInt(match[2]).toLocaleString();
        text = `Programs: ${current}/${total}`;
      }
    } else if (
      text.includes("Saved") &&
      text.includes("programs to database")
    ) {
      const match = text.match(/Saved (\d+)\/(\d+) programs/);
      if (match) {
        const current = parseInt(match[1]).toLocaleString();
        const total = parseInt(match[2]).toLocaleString();
        text = `Saving: ${current}/${total}`;
      }
    } else if (
      text.includes("Saving") &&
      text.includes("programs to database")
    ) {
      // Handle "Saving X channels and Y programs to database" format
      const channelMatch = text.match(/Saving (\d+) channels/);
      const programMatch = text.match(/(\d+) programs to database/);
      if (channelMatch && programMatch) {
        const channels = parseInt(channelMatch[1]).toLocaleString();
        const programs = parseInt(programMatch[1]).toLocaleString();
        text = `Saving: ${channels} channels, ${programs} programs`;
      }
    } else if (text.includes("Downloaded") && text.includes("bytes")) {
      // Handle download progress
      const match = text.match(/Downloaded (\d+) \/ (\d+) bytes/);
      if (match) {
        const current = parseInt(match[1]);
        const total = parseInt(match[2]);
        const currentMB = (current / 1024 / 1024).toFixed(1);
        const totalMB = (total / 1024 / 1024).toFixed(1);
        text = `Downloading: ${currentMB}/${totalMB} MB`;
      }
    } else if (text.includes("Starting EPG ingestion")) {
      text = "Starting ingestion...";
    } else if (text.includes("Parsing completed")) {
      const match = text.match(/(\d+) channels found/);
      if (match) {
        const count = parseInt(match[1]).toLocaleString();
        text = `Found ${count} channels`;
      } else {
        text = "Parsing completed";
      }
    } else if (text.includes("Completed -")) {
      // Handle completion messages
      const channelMatch = text.match(/(\d+) channels/);
      const programMatch = text.match(/(\d+) programs/);
      if (channelMatch && programMatch) {
        const channels = parseInt(channelMatch[1]).toLocaleString();
        const programs = parseInt(programMatch[1]).toLocaleString();
        text = `Complete: ${channels} channels, ${programs} programs`;
      } else if (channelMatch) {
        const channels = parseInt(channelMatch[1]).toLocaleString();
        text = `Complete: ${channels} channels`;
      }
    }

    // Add percentage if available
    if (progress.percentage != null) {
      text += ` (${Math.round(progress.percentage)}%)`;
    }

    return text;
  }

  renderRefreshCell(source) {
    if (!source.is_active) {
      return '<span class="text-muted">-</span>';
    }

    const progress = this.progressData[source.id];
    const processingInfo = this.processingInfo[source.id];

    const isProcessing = progress && this.isActivelyProcessing(progress.state);
    const isInBackoff =
      processingInfo &&
      processingInfo.next_retry_after &&
      new Date(processingInfo.next_retry_after) > new Date();

    let refreshButtonContent;

    if (isProcessing) {
      refreshButtonContent = `<button class="btn btn-sm btn-primary" disabled>
                <span class="spinner-border spinner-border-sm me-1" role="status"></span>
                Processing...
            </button>`;
    } else if (isInBackoff) {
      const retryTime = new Date(processingInfo.next_retry_after);
      const timeLeft = Math.ceil((retryTime - new Date()) / 1000);
      const minutes = Math.floor(timeLeft / 60);
      const seconds = timeLeft % 60;
      const timeString =
        minutes > 0 ? `${minutes}m ${seconds}s` : `${seconds}s`;

      refreshButtonContent = `<button class="btn btn-sm btn-danger" disabled title="In backoff period after ${processingInfo.failure_count} failures. Retry in ${timeString}">
                ⏳ Retry in ${timeString}
            </button>`;
    } else {
      refreshButtonContent = `<button class="btn btn-sm btn-outline-primary" onclick="epgSourcesManager.refreshSource('${source.id}')">
                🔄 Refresh
            </button>`;
    }

    let refreshInfo = "";
    if (source.next_scheduled_update && !isProcessing && !isInBackoff) {
      const nextUpdate = new Date(source.next_scheduled_update);
      const now = new Date();
      const timeDiff = nextUpdate - now;

      if (timeDiff > 0) {
        const hours = Math.floor(timeDiff / (1000 * 60 * 60));
        const minutes = Math.floor((timeDiff % (1000 * 60 * 60)) / (1000 * 60));
        refreshInfo = `<small class="text-muted">Next: ${hours}h ${minutes}m</small>`;
      } else {
        refreshInfo = `<small class="text-warning">Overdue</small>`;
      }
    }

    return `
            <div class="text-center">
                ${refreshButtonContent}
                ${refreshInfo ? `<br>${refreshInfo}` : ""}
            </div>
        `;
  }

  renderActionsCell(source) {
    return `
            <div class="btn-group" role="group">
                <button class="btn btn-sm btn-outline-secondary" onclick="epgSourcesManager.editSource('${source.id}')">
                    ✏️ Edit
                </button>
                <button class="btn btn-sm btn-outline-danger" onclick="epgSourcesManager.deleteSource('${source.id}', '${this.escapeHtml(source.name)}')">
                    🗑️ Delete
                </button>
            </div>
        `;
  }

  showSourceModal(source = null) {
    this.editingSource = source;
    const modal = document.getElementById("epgSourceModal");
    const title = document.getElementById("modalTitle");
    const form = document.getElementById("epgSourceForm");

    // Reset form
    form.reset();

    if (source) {
      title.textContent = "Edit EPG Source";
      document.getElementById("epgSourceName").value = source.name;
      document.getElementById("epgSourceType").value = source.source_type;
      document.getElementById("epgSourceUrl").value = source.url;
      document.getElementById("updateCron").value = source.update_cron;
      document.getElementById("timezone").value = source.timezone || "UTC";
      document.getElementById("timeOffset").value = source.time_offset || "0";
      document.getElementById("isActive").checked = source.is_active;

      if (source.username) {
        document.getElementById("epgUsername").value = source.username;
      }
      if (source.password) {
        document.getElementById("epgPassword").value = source.password;
      }
    } else {
      title.textContent = "Add EPG Source";
      document.getElementById("updateCron").value = "0 0 */6 * * * *";
      document.getElementById("timezone").value = "UTC";
      document.getElementById("timeOffset").value = "0";
      document.getElementById("isActive").checked = true;
      document.getElementById("epgSourceType").value = "xtream";
    }

    this.toggleSourceTypeFields(document.getElementById("epgSourceType").value);
    modal.classList.add("show");
  }

  hideSourceModal() {
    const modal = document.getElementById("epgSourceModal");
    modal.classList.remove("show");
    this.editingSource = null;
  }

  toggleSourceTypeFields(sourceType) {
    const xtreamFields = document.getElementById("xtreamFields");

    if (sourceType === "xtream") {
      xtreamFields.style.display = "block";
      document.getElementById("epgUsername").required = true;
      document.getElementById("epgPassword").required = true;
    } else {
      xtreamFields.style.display = "none";
      document.getElementById("epgUsername").required = false;
      document.getElementById("epgPassword").required = false;
    }
  }

  async saveSource() {
    if (!this.validateForm()) {
      return;
    }

    const formData = this.getFormData();
    const isEditing = this.editingSource !== null;

    try {
      const url = isEditing
        ? `/api/v1/sources/epg/${this.editingSource.id}`
        : "/api/v1/sources/epg";

      const method = isEditing ? "PUT" : "POST";

      const response = await fetch(url, {
        method: method,
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify(formData),
      });

      if (!response.ok) {
        const errorText = await response.text();
        throw new Error(
          `HTTP error! status: ${response.status}, message: ${errorText}`,
        );
      }

      const result = await response.json();

      this.hideSourceModal();
      await this.loadSources();

      this.showAlert(
        `EPG source "${result.name}" ${isEditing ? "updated" : "created"} successfully`,
        "success",
      );
    } catch (error) {
      console.error("Error saving EPG source:", error);
      this.showAlert(
        `Failed to ${isEditing ? "update" : "create"} EPG source: ${error.message}`,
        "error",
      );
    }
  }

  getFormData() {
    const sourceType = document.getElementById("epgSourceType").value;

    const data = {
      name: document.getElementById("epgSourceName").value.trim(),
      source_type: sourceType,
      url: document.getElementById("epgSourceUrl").value.trim(),
      update_cron:
        document.getElementById("updateCron").value.trim() || "0 0 */6 * * * *",
      timezone: document.getElementById("timezone").value || "UTC",
      time_offset: document.getElementById("timeOffset").value.trim() || "0",
    };

    if (this.editingSource) {
      data.is_active = document.getElementById("isActive").checked;
    }

    if (sourceType === "xtream") {
      data.username = document.getElementById("epgUsername").value.trim();
      data.password = document.getElementById("epgPassword").value.trim();
    }

    return data;
  }

  validateForm() {
    const name = document.getElementById("epgSourceName").value.trim();
    const url = document.getElementById("epgSourceUrl").value.trim();
    const sourceType = document.getElementById("epgSourceType").value;

    if (!name) {
      this.showAlert("Please enter a source name", "error");
      return false;
    }

    if (!url) {
      this.showAlert("Please enter a source URL", "error");
      return false;
    }

    if (sourceType === "xtream") {
      const username = document.getElementById("epgUsername").value.trim();
      const password = document.getElementById("epgPassword").value.trim();

      if (!username || !password) {
        this.showAlert(
          "Username and password are required for Xtream Codes sources",
          "error",
        );
        return false;
      }
    }

    return true;
  }

  async editSource(sourceId) {
    try {
      const response = await fetch(`/api/v1/sources/epg/${sourceId}`);

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const source = await response.json();
      this.showSourceModal(source);
    } catch (error) {
      console.error("Error loading EPG source for editing:", error);
      this.showAlert("Failed to load EPG source for editing", "error");
    }
  }

  async refreshSource(sourceId) {
    try {
      const response = await fetch(`/api/v1/sources/epg/${sourceId}/refresh`, {
        method: "POST",
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const result = await response.json();

      if (result.success) {
        this.showAlert(result.message, "success");
        // Reload sources to update status
        setTimeout(() => {
          this.loadSources();
        }, 1000);
      } else {
        this.showAlert(result.message, "error");
      }
    } catch (error) {
      console.error("Error refreshing EPG source:", error);
      this.showAlert("Failed to refresh EPG source", "error");
    }
  }

  async refreshAllSources() {
    const activeSources = this.sources.filter((s) => s.is_active);

    if (activeSources.length === 0) {
      this.showAlert("No active EPG sources to refresh", "warning");
      return;
    }

    let successCount = 0;
    let errorCount = 0;

    for (const source of activeSources) {
      try {
        const response = await fetch(`/api/v1/sources/epg/${source.id}/refresh`, {
          method: "POST",
        });

        if (response.ok) {
          successCount++;
        } else {
          errorCount++;
        }
      } catch (error) {
        console.error(`Error refreshing source ${source.name}:`, error);
        errorCount++;
      }
    }

    if (errorCount === 0) {
      this.showAlert(
        `Successfully initiated refresh for all ${successCount} EPG sources`,
        "success",
      );
    } else {
      this.showAlert(
        `Initiated refresh for ${successCount} sources, ${errorCount} failed`,
        "warning",
      );
    }

    // Start progress polling immediately
    this.startProgressPolling();

    // Reload sources to update status
    setTimeout(() => {
      this.loadSources();
    }, 2000);
  }

  // Cleanup method for page unload
  cleanup() {
    this.stopProgressPolling();
  }

  // Progress tracking methods
  async loadProgress() {
    try {
      const response = await fetch("/api/v1/progress/epg");
      if (!response.ok) return;

      const data = await response.json();
      const sourcesData = data.sources || [];

      // Extract progress and processing info from API response
      const extractedProgress = {};
      const extractedProcessingInfo = {};

      sourcesData.forEach((sourceData) => {
        const sourceId = sourceData.source_id;
        if (sourceData.progress) {
          extractedProgress[sourceId] = sourceData.progress;
        }
        if (sourceData.processing_info) {
          extractedProcessingInfo[sourceId] = sourceData.processing_info;
        }
      });

      // Check if any sources just completed
      const hasActiveProgress = Object.values(extractedProgress).some((p) =>
        this.isActivelyProcessing(p.state),
      );

      // If progress state changed (had active, now doesn't), reload sources
      if (this.lastHadProgress && !hasActiveProgress) {
        await this.loadSources();
      }

      this.lastHadProgress = hasActiveProgress;
      this.progressData = extractedProgress;
      this.processingInfo = extractedProcessingInfo;

      // Re-render sources to update status display
      this.renderSources();
    } catch (error) {
      console.debug("Progress loading error:", error);
    }
  }

  async loadProcessingInfo() {
    // This method is now handled by loadProgress() which gets consolidated data
    // Keeping this method for backward compatibility but it's no longer needed
    console.debug(
      "loadProcessingInfo() called but processing info is now loaded via consolidated progress endpoint",
    );
  }

  isRecentlyProcessed(progress) {
    if (!progress.updated_at) return false;
    const lastUpdate = new Date(progress.updated_at);
    const now = new Date();
    const timeDiff = now - lastUpdate;
    // Consider as recently processed if updated within the last 30 seconds
    return timeDiff < 30000;
  }

  startProgressPolling() {
    this.stopProgressPolling(); // Clear any existing interval

    // Load progress immediately
    this.loadProgress();

    // Poll every 2 seconds
    this.progressPollingInterval = setInterval(() => {
      this.loadProgress();
    }, 2000);
  }

  stopProgressPolling() {
    if (this.progressPollingInterval) {
      clearInterval(this.progressPollingInterval);
      this.progressPollingInterval = null;
    }
  }

  isActivelyProcessing(state) {
    return [
      "connecting",
      "downloading",
      "parsing",
      "saving",
      "processing",
    ].includes(state);
  }

  async deleteSource(sourceId, sourceName) {
    if (
      !confirm(
        `Are you sure you want to delete the EPG source "${sourceName}"? This will also delete all associated channels and programs.`,
      )
    ) {
      return;
    }

    try {
      const response = await fetch(`/api/v1/sources/epg/${sourceId}`, {
        method: "DELETE",
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      await this.loadSources();
      this.showAlert(
        `EPG source "${sourceName}" deleted successfully`,
        "success",
      );
    } catch (error) {
      console.error("Error deleting EPG source:", error);
      this.showAlert("Failed to delete EPG source", "error");
    }
  }

  async showChannels(sourceId, sourceName) {
    try {
      // Show modal first
      const modal = document.getElementById("channelsModal");
      const title = document.getElementById("channelsModalTitle");
      const loading = document.getElementById("channelsLoading");
      const content = document.getElementById("channelsContent");

      title.textContent = `EPG Channels - ${sourceName}`;
      modal.classList.add("show");
      loading.style.display = "block";
      content.style.display = "none";

      // Load channels
      const response = await fetch(`/api/v1/sources/epg/${sourceId}/channels`);

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      this.currentChannels = await response.json();
      this.filteredChannels = [...this.currentChannels];

      this.renderChannels();

      loading.style.display = "none";
      content.style.display = "block";
    } catch (error) {
      console.error("Error loading EPG channels:", error);
      this.showAlert("Failed to load EPG channels", "error");
      this.hideChannelsModal();
    }
  }

  renderChannels() {
    const tbody = document.getElementById("channelsTableBody");
    const countSpan = document.getElementById("channelsCount");

    tbody.innerHTML = "";
    countSpan.textContent = `${this.filteredChannels.length} channels`;

    if (this.filteredChannels.length === 0) {
      tbody.innerHTML = `
                <tr>
                    <td colspan="4" class="text-center text-muted">
                        No channels found
                    </td>
                </tr>
            `;
      return;
    }

    this.filteredChannels.forEach((channel) => {
      const row = document.createElement("tr");

      row.innerHTML = `
                <td>
                    <div class="channel-info">
                        <strong>${this.escapeHtml(channel.channel_name)}</strong>
                        ${
                          channel.channel_logo
                            ? `<br><img src="${this.escapeHtml(channel.channel_logo)}" alt="Logo" style="max-height: 20px; max-width: 40px;" onerror="this.style.display='none'">`
                            : ""
                        }
                    </div>
                </td>
                <td>
                    <code>${this.escapeHtml(channel.channel_id)}</code>
                </td>
                <td>
                    ${channel.channel_group ? this.escapeHtml(channel.channel_group) : "-"}
                </td>
                <td>
                    ${channel.language ? this.escapeHtml(channel.language) : "-"}
                </td>
            `;

      tbody.appendChild(row);
    });
  }

  filterChannels(filterText) {
    if (!filterText) {
      this.filteredChannels = [...this.currentChannels];
    } else {
      const filter = filterText.toLowerCase();
      this.filteredChannels = this.currentChannels.filter(
        (channel) =>
          channel.channel_name.toLowerCase().includes(filter) ||
          channel.channel_id.toLowerCase().includes(filter) ||
          (channel.channel_group &&
            channel.channel_group.toLowerCase().includes(filter)),
      );
    }

    this.renderChannels();
  }

  hideChannelsModal() {
    const modal = document.getElementById("channelsModal");
    if (modal) {
      modal.classList.remove("show");
    }
    this.currentChannels = [];
    this.filteredChannels = [];
  }

  showAlert(message, type = "info") {
    // Create alert element
    const alert = document.createElement("div");
    alert.className = `alert alert-${type} alert-dismissible fade show`;
    alert.innerHTML = `
            ${message}
            <button type="button" class="close" data-dismiss="alert">
                <span>&times;</span>
            </button>
        `;

    // Add to alerts container
    const container =
      document.getElementById("alertsContainer") || document.body;
    container.appendChild(alert);

    // Auto-hide after 5 seconds
    setTimeout(() => {
      if (alert.parentNode) {
        alert.remove();
      }
    }, 5000);

    // Handle close button
    alert.querySelector(".close").addEventListener("click", () => {
      alert.remove();
    });
  }

  showLoading() {
    const loading = document.getElementById("loadingIndicator");
    if (loading) {
      loading.style.display = "block";
    }
  }

  hideLoading() {
    const loading = document.getElementById("loadingIndicator");
    if (loading) {
      loading.style.display = "none";
    }
  }

  escapeHtml(text) {
    if (!text) return "";
    const div = document.createElement("div");
    div.textContent = text;
    return div.innerHTML;
  }

  // EPG Viewer Modal Methods
  async showEpgViewerModal() {
    console.log("showEpgViewerModal called - DEBUG VERSION 2.0");
    const modal = document.getElementById("epgViewerModal");
    if (modal) {
      console.log("EPG viewer modal found, showing...");
      modal.classList.add("show");

      // Load EPG viewer content into modal
      const container = document.getElementById("epgViewerContainer");
      if (container) {
        console.log(
          "EPG viewer container found, current children:",
          container.children.length,
        );
        console.log("Container innerHTML length:", container.innerHTML.length);

        // Always load content for now to debug the issue
        console.log("Loading EPG viewer content...");
        await this.loadEpgViewerContent(container);
      } else {
        console.error("EPG viewer container not found");
      }
    } else {
      console.error("EPG viewer modal not found");
    }
  }

  hideEpgViewerModal() {
    console.log("hideEpgViewerModal called");
    const modal = document.getElementById("epgViewerModal");
    if (modal) {
      modal.classList.remove("show");
      console.log("EPG viewer modal hidden");
    } else {
      console.error("EPG viewer modal not found");
    }
  }

  async loadEpgViewerContent(container) {
    try {
      console.log("Loading EPG viewer content into container");
      console.log("Container element:", container);

      // Create EPG viewer HTML structure
      console.log("Setting container innerHTML...");
      container.innerHTML = `
        <div class="epg-container">
          <div class="epg-header">
            <div class="epg-controls">
              <div class="form-group mb-0">
                <label for="channelFilterModal" class="form-label">Filter Channels:</label>
                <input
                  type="text"
                  id="channelFilterModal"
                  class="form-control form-control-sm"
                  placeholder="Search channels, groups, TVG-ID..."
                  style="width: 200px;"
                />
              </div>

              <div class="form-group mb-0">
                <label for="dateSelectModal" class="form-label">Date:</label>
                <input
                  type="date"
                  id="dateSelectModal"
                  class="form-control form-control-sm"
                />
              </div>

              <div class="form-group mb-0">
                <label for="timeRangeModal" class="form-label">Time Range:</label>
                <select id="timeRangeModal" class="form-select form-control-sm">
                  <option value="6">6 hours</option>
                  <option value="12" selected>12 hours</option>
                  <option value="24">24 hours</option>
                </select>
              </div>

              <div class="form-group mb-0">
                <label for="startTimeModal" class="form-label">Start Time:</label>
                <input
                  type="time"
                  id="startTimeModal"
                  class="form-control form-control-sm"
                />
              </div>
            </div>

            <div class="epg-controls">
              <button id="refreshEpgModalBtn" class="btn btn-primary btn-sm">
                🔄 Refresh
              </button>
              <button id="nowModalBtn" class="btn btn-outline-secondary btn-sm">
                📍 Now
              </button>
              <span id="channelCountModal" class="text-muted">0 channels</span>
            </div>
          </div>

          <div id="epgGridModal" class="epg-grid">
            <div id="epgLoadingModal" class="epg-loading">
              <div class="text-center">
                <span class="loading"></span>
                <div class="mt-2">Loading EPG data...</div>
              </div>
            </div>

            <div id="epgNoDataModal" class="epg-no-data" style="display: none;">
              <div class="mb-3">📺</div>
              <h5>No EPG Data Available</h5>
              <p class="text-center">
                No program data found for the selected time range.<br>
                Make sure you have configured EPG sources and they have been refreshed.
              </p>
            </div>

            <div id="epgContentModal" style="display: none;">
              <!-- Timeline will be generated here -->
              <div id="epgTimelineModal" class="epg-timeline"></div>

              <!-- Channels and programs will be generated here -->
              <div id="epgChannelsModal" class="epg-channels"></div>
            </div>
          </div>
        </div>
      `;

      console.log("EPG viewer HTML structure created");
      console.log(
        "Container innerHTML after setting:",
        container.innerHTML.length,
        "characters",
      );
      console.log("Container children count:", container.children.length);

      // Initialize modal EPG viewer functionality
      console.log("Initializing modal EPG viewer...");
      this.initializeModalEpgViewer();
      console.log("EPG viewer content loaded successfully");
    } catch (error) {
      console.error("Error loading EPG viewer content:", error);
      container.innerHTML = `
        <div class="text-center p-4">
          <div class="alert alert-danger">
            <h5>Failed to load EPG viewer</h5>
            <p>Please try again. Error: ${error.message}</p>
            <button onclick="epgSourcesManager.loadEpgViewerContent(document.getElementById('epgViewerContainer'))" class="btn btn-primary">Retry</button>
          </div>
        </div>
      `;
    }
  }

  initializeModalEpgViewer() {
    console.log("Initializing modal EPG viewer");

    // Debug: Check if we can find all the expected elements
    const elementsToCheck = [
      "dateSelectModal",
      "startTimeModal",
      "timeRangeModal",
      "channelFilterModal",
      "refreshEpgModalBtn",
      "nowModalBtn",
    ];

    elementsToCheck.forEach((id) => {
      const element = document.getElementById(id);
      console.log(`Element ${id}:`, element ? "found" : "NOT FOUND");
    });

    // Set default date to today
    const dateInput = document.getElementById("dateSelectModal");
    if (dateInput) {
      dateInput.value = new Date().toISOString().split("T")[0];
      console.log("Set default date to today:", dateInput.value);
    } else {
      console.warn("Date input not found");
    }

    // Set default time to current hour
    const timeInput = document.getElementById("startTimeModal");
    if (timeInput) {
      const now = new Date();
      timeInput.value = `${now.getHours().toString().padStart(2, "0")}:00`;
      console.log("Set default time to current hour");
    } else {
      console.warn("Time input not found");
    }

    // Add event listeners
    const refreshBtn = document.getElementById("refreshEpgModalBtn");
    if (refreshBtn) {
      refreshBtn.addEventListener("click", () => {
        console.log("Refresh button clicked");
        this.loadModalEpgData();
      });
      console.log("Added refresh button listener");
    } else {
      console.warn("Refresh button not found");
    }

    const nowBtn = document.getElementById("nowModalBtn");
    if (nowBtn) {
      nowBtn.addEventListener("click", () => {
        console.log("Now button clicked");
        this.goToNowInModal();
      });
      console.log("Added now button listener");
    } else {
      console.warn("Now button not found");
    }

    const channelFilter = document.getElementById("channelFilterModal");
    if (channelFilter) {
      channelFilter.addEventListener("input", (e) => {
        console.log("Channel filter changed:", e.target.value);
        // Re-render the EPG data with the filter applied
        this.loadModalEpgData();
      });
      console.log("Added channel filter listener");
    } else {
      console.warn("Channel filter not found");
    }

    // Load initial data
    console.log("Loading initial EPG data");

    // Debug: Check visibility of key containers
    setTimeout(() => {
      const epgContainer = document.querySelector(".epg-container");
      const epgHeader = document.querySelector(".epg-header");
      const epgGrid = document.querySelector(".epg-grid");

      console.log(
        "EPG Container visible:",
        epgContainer ? getComputedStyle(epgContainer).display : "not found",
      );
      console.log(
        "EPG Header visible:",
        epgHeader ? getComputedStyle(epgHeader).display : "not found",
      );
      console.log(
        "EPG Grid visible:",
        epgGrid ? getComputedStyle(epgGrid).display : "not found",
      );
    }, 100);

    this.loadModalEpgData();
  }

  refreshModalEpgData() {
    console.log("Refreshing modal EPG data");
    this.loadModalEpgData();
  }

  async loadModalEpgData() {
    try {
      console.log("Loading modal EPG data");

      const loadingDiv = document.getElementById("epgLoadingModal");
      const contentDiv = document.getElementById("epgContentModal");
      const noDataDiv = document.getElementById("epgNoDataModal");

      if (loadingDiv) {
        loadingDiv.style.display = "flex";
        console.log("Showing loading indicator");
      }
      if (contentDiv) contentDiv.style.display = "none";
      if (noDataDiv) noDataDiv.style.display = "none";

      // Get form values
      const date =
        document.getElementById("dateSelectModal")?.value ||
        new Date().toISOString().split("T")[0];
      const startTime =
        document.getElementById("startTimeModal")?.value || "00:00";
      const timeRange = parseInt(
        document.getElementById("timeRangeModal")?.value || "12",
      );

      console.log("Form values:", { date, startTime, timeRange });

      // Create start datetime
      const startDateTime = new Date(`${date}T${startTime}:00`);
      const endDateTime = new Date(
        startDateTime.getTime() + timeRange * 60 * 60 * 1000,
      );

      console.log("Date range:", { startDateTime, endDateTime });

      // Build API URL
      const params = new URLSearchParams({
        start_time: startDateTime.toISOString(),
        end_time: endDateTime.toISOString(),
      });

      // Note: We do client-side filtering instead of server-side for better flexibility

      const apiUrl = `/api/v1/epg/viewer?${params}`;
      console.log("Making API request to:", apiUrl);

      const response = await fetch(apiUrl, {
        cache: "no-cache",
        headers: {
          "Cache-Control": "no-cache",
        },
      });

      if (!response.ok) {
        const errorText = await response.text();
        console.error("API response error:", response.status, errorText);
        throw new Error(
          `HTTP error! status: ${response.status}, message: ${errorText}`,
        );
      }

      const data = await response.json();
      console.log("Received EPG data:", data);

      // Filter channels first to determine what to show
      const channelFilterElement =
        document.getElementById("channelFilterModal");
      const filterText = channelFilterElement
        ? channelFilterElement.value.toLowerCase()
        : "";

      let channelsToShow = data.channels || [];
      if (filterText) {
        channelsToShow = data.channels.filter((channel) => {
          const searchFields = [
            channel.channel_name,
            channel.channel_id,
            channel.tvg_id,
            channel.tvg_name,
            channel.group_title,
            channel.tvg_logo,
            channel.stream_url,
          ];

          return searchFields.some(
            (field) =>
              field && field.toString().toLowerCase().includes(filterText),
          );
        });
      }

      console.log(
        `Total channels: ${data.channels?.length || 0}, After filtering: ${channelsToShow.length}, Filter: "${filterText}"`,
      );

      if (loadingDiv) {
        loadingDiv.style.display = "none";
        console.log("Hiding loading indicator");
      }

      // Show appropriate content based on filtered results
      if (channelsToShow.length > 0) {
        if (contentDiv) {
          contentDiv.style.display = "block";
          console.log(
            "Showing content with",
            channelsToShow.length,
            "filtered channels",
          );
        }
        if (noDataDiv) {
          noDataDiv.style.display = "none";
        }

        // Render the filtered data
        this.renderModalEpgData({ ...data, channels: channelsToShow });
      } else {
        // No channels after filtering or no data at all
        if (noDataDiv) {
          noDataDiv.style.display = "flex";

          // Update message based on whether we have data but no filter matches
          const noDataMessage = noDataDiv.querySelector("p");
          if (noDataMessage) {
            if (filterText && data.channels && data.channels.length > 0) {
              noDataMessage.innerHTML = `
                No channels match your search for "<strong>${filterText}</strong>".<br>
                Try a different search term or clear the filter to see all channels.
              `;
            } else {
              noDataMessage.innerHTML = `
                No program data found for the selected time range.<br>
                Make sure you have configured EPG sources and they have been refreshed.
              `;
            }
          }
          console.log("Showing no data message");
        }
        if (contentDiv) {
          contentDiv.style.display = "none";
        }
      }
    } catch (error) {
      console.error("Error loading EPG data:", error);
      const loadingDiv = document.getElementById("epgLoadingModal");
      const noDataDiv = document.getElementById("epgNoDataModal");

      if (loadingDiv) loadingDiv.style.display = "none";
      if (noDataDiv) {
        noDataDiv.style.display = "flex";
        // Update no data message to show the error
        const noDataContent = noDataDiv.querySelector("p");
        if (noDataContent) {
          noDataContent.innerHTML = `
            Failed to load EPG data: ${error.message}<br>
            Please check that EPG sources are configured and the server is running.
          `;
        }
      }
    }
  }

  renderModalEpgData(data) {
    console.log(
      "Rendering modal EPG data for",
      data.channels?.length || 0,
      "channels",
    );

    // Update channel count
    const channelCount = document.getElementById("channelCountModal");
    if (channelCount && data.channels) {
      channelCount.textContent = `${data.channels.length} channels`;
    }

    // Get time range from form
    const dateInput = document.getElementById("dateSelectModal");
    const timeInput = document.getElementById("startTimeModal");
    const rangeInput = document.getElementById("timeRangeModal");

    if (!dateInput || !timeInput || !rangeInput) {
      console.error("Form inputs not found for EPG rendering");
      return;
    }

    const selectedDate = dateInput.value;
    const selectedTime = timeInput.value;
    const timeRange = parseInt(rangeInput.value);

    // Generate timeline
    const timelineDiv = document.getElementById("epgTimelineModal");
    const channelsDiv = document.getElementById("epgChannelsModal");

    if (timelineDiv && channelsDiv) {
      // Create time slots for the timeline
      const startDateTime = new Date(`${selectedDate}T${selectedTime}`);
      const timeSlots = [];

      for (let i = 0; i < timeRange; i++) {
        const slotTime = new Date(startDateTime.getTime() + i * 60 * 60 * 1000); // Add hours
        timeSlots.push(slotTime);
      }

      // Render timeline header
      timelineDiv.innerHTML = timeSlots
        .map(
          (time) =>
            `<div class="epg-time-slot">${time.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" })}</div>`,
        )
        .join("");

      // Render channels (filtering is now handled in loadModalEpgData)
      const channelsToShow = data.channels;

      channelsDiv.innerHTML = channelsToShow
        .map(
          (channel, index) => `
        <div class="epg-channel-row">
          <div class="epg-channel-info">
            <div class="epg-channel-name">${channel.channel_name}</div>
            <div class="epg-channel-id">${channel.channel_id}</div>
            ${channel.group_title ? `<div class="epg-channel-group">📂 ${channel.group_title}</div>` : ""}
            ${channel.tvg_id ? `<div class="epg-channel-tvg">🆔 ${channel.tvg_id}</div>` : ""}
          </div>
          <div class="epg-programs">
            ${timeSlots
              .map((time) => {
                // Find program for this time slot
                const program = channel.programs?.find((p) => {
                  const start = new Date(p.start_time);
                  const end = new Date(p.end_time);
                  return time >= start && time < end;
                });

                if (program) {
                  return `
                  <div class="epg-program" title="${program.program_description || ""}">
                    <div class="epg-program-title">${program.program_title}</div>
                    <div class="epg-program-time">${new Date(program.start_time).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" })}</div>
                    ${program.program_category ? `<div class="epg-program-category">${program.program_category}</div>` : ""}
                  </div>
                `;
                } else {
                  return '<div class="epg-empty-slot">No Program</div>';
                }
              })
              .join("")}
          </div>
        </div>
      `,
        )
        .join("");

      console.log(
        "EPG timeline rendered with",
        channelsToShow.length,
        "channels",
      );

      // Add scroll debug info
      setTimeout(() => {
        const epgGrid = document.getElementById("epgGridModal");
        if (epgGrid) {
          console.log("EPG Grid scroll info:", {
            scrollHeight: epgGrid.scrollHeight,
            clientHeight: epgGrid.clientHeight,
            scrollable: epgGrid.scrollHeight > epgGrid.clientHeight,
          });
        }
      }, 100);
    }
  }

  goToNowInModal() {
    const now = new Date();
    const dateInput = document.getElementById("dateSelectModal");
    const timeInput = document.getElementById("startTimeModal");

    if (dateInput) {
      dateInput.value = now.toISOString().split("T")[0];
    }

    if (timeInput) {
      timeInput.value = `${now.getHours().toString().padStart(2, "0")}:00`;
    }

    this.loadModalEpgData();
  }

  filterModalChannels(filter) {
    // Implement channel filtering in modal
    // This would filter the displayed channels based on the search term
    console.log("Filtering modal channels:", filter);
  }
}

function togglePassword(fieldId) {
  const field = document.getElementById(fieldId);
  const button = field.nextElementSibling;

  if (field.type === "password") {
    field.type = "text";
    button.textContent = "👁️";
  } else {
    field.type = "password";
    button.textContent = "👁️‍🗨️";
  }
}

function hideChannelsModal() {
  if (window.epgSourcesManager) {
    window.epgSourcesManager.hideChannelsModal();
  }
}

// Initialize when page loads
let epgSourcesManager;
document.addEventListener("DOMContentLoaded", () => {
  epgSourcesManager = new EpgSourcesManager();
  window.epgSourcesManager = epgSourcesManager;
});

// Cleanup when page unloads
window.addEventListener("beforeunload", () => {
  if (epgSourcesManager) {
    epgSourcesManager.cleanup();
  }
});
