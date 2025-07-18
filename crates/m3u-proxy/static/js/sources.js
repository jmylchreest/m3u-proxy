// Stream Sources Management JavaScript

class SourcesManager {
  constructor() {
    this.sources = [];
    this.editingSource = null;
    this.progressPollingInterval = null;
    this.progressData = {};
    this.processingInfo = {};
    this.lastHadProgress = false;
    this.currentChannels = [];
    this.filteredChannels = [];
    this.renderedChannels = 0;
    this.channelsPerBatch = 100;
    this.scrollContainer = null;
    this.currentPage = 1;
    this.totalPages = 1;
    this.totalCount = 0;
    this.currentFilter = "";
    this.currentSourceId = null;
    this.filterTimeout = null;
    this.init();
  }

  async init() {
    this.setupEventListeners();
    await this.loadSources();
    this.startProgressPolling();
  }

  setupEventListeners() {
    // Add source button
    document.getElementById("addSourceBtn").addEventListener("click", () => {
      this.showSourceModal();
    });

    // Modal close functionality handled by standard modal utilities

    // Modal cancel
    document.getElementById("cancelSource").addEventListener("click", () => {
      this.hideSourceModal();
    });

    // Modal save
    document.getElementById("saveSource").addEventListener("click", () => {
      this.saveSource();
    });

    // Source type change
    document.getElementById("sourceType").addEventListener("change", (e) => {
      this.toggleSourceTypeFields(e.target.value);
    });

    // Removed click-outside-to-close to prevent accidental closure

    // Channels modal functionality is now handled by shared channels-viewer.js
  }

  async loadSources() {
    try {
      this.showLoading();
      const response = await fetch("/api/v1/sources/stream");

      if (!response.ok) {
        throw new Error(
          `Failed to load sources: ${response.status} ${response.statusText}`,
        );
      }

      const data = await response.json();
      // Handle paginated API response format
      if (data.success && data.data && Array.isArray(data.data.items)) {
        this.sources = data.data.items;
      } else if (Array.isArray(data)) {
        this.sources = data;
      } else {
        this.sources = [];
      }
      await this.loadProcessingInfo();
      this.renderSources();
    } catch (error) {
      this.showAlert("Failed to load sources: " + error.message, "danger");
      console.error("Error loading sources:", error);
      this.sources = [];
      this.renderSources();
    } finally {
      this.hideLoading();
    }
  }

  renderSources() {
    const tbody = document.getElementById("sourcesTableBody");

    if (!tbody) {
      console.error("sourcesTableBody element not found!");
      return;
    }

    if (!this.sources || this.sources.length === 0) {
      tbody.innerHTML = `
                <tr>
                    <td colspan="5" class="text-center text-muted">
                        No stream sources configured. Click "Add Stream Source" to get started.
                    </td>
                </tr>
            `;
      return;
    }

    tbody.innerHTML = this.sources
      .map((sourceWithStats) => {
        // Handle unified source structure
        const source = sourceWithStats.source || sourceWithStats;
        const channelCount = sourceWithStats.channel_count || 0;
        const nextScheduledUpdate = sourceWithStats.next_scheduled_update;

        const progress = this.getSourceProgress(source.id);
        const statusCell = this.renderStatusCell(source, progress);
        const actionsCell = this.renderActionsCell(source, progress);

        const typeIndicator = source.source_type === "m3u" ? "M3U" : "XC";
        const typeColor = "#007bff"; // Blue color to match STREAM color in data mapping
        const rowOpacity = source.is_active ? "1" : "0.6";

        return `
                <tr style="opacity: ${rowOpacity}">
                    <td>
                        <strong>${this.escapeHtml(source.name)}<sup style="color: ${typeColor}; font-size: 0.7em; margin-left: 3px;">${typeIndicator}</sup></strong>
                        <br>
                        <small class="text-muted">${this.escapeHtml(source.url)}</small>
                    </td>
                    <td>
                        <button class="btn btn-link p-0 text-primary" onclick="sourcesManager.showChannels('${source.id}', '${this.escapeHtml(source.name)}')" title="View channels">
                            ${channelCount.toLocaleString()} ${channelCount === 1 ? "channel" : "channels"}
                        </button>
                    </td>
                    <td>${statusCell}</td>
                    <td>
                        <div class="update-badges">
                            <div class="badge badge-secondary badge-sm">
                                Last: ${
                                  source.last_ingested_at
                                    ? this.formatTimeCompact(
                                        this.parseDateTime(
                                          source.last_ingested_at,
                                        ),
                                      )
                                    : "Never"
                                }
                            </div>
                            <div class="badge badge-info badge-sm">
                                Next: ${
                                  nextScheduledUpdate
                                    ? this.formatTimeCompact(
                                        this.parseDateTime(nextScheduledUpdate),
                                      )
                                    : "Not scheduled"
                                }
                            </div>
                        </div>
                    </td>
                    <td>${actionsCell}</td>
                </tr>
            `;
      })
      .join("");
  }

  renderStatusCell(source, progress) {
    if (progress) {
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

      const color = stateColors[progress.state] || "secondary";
      const percentage = progress.progress.percentage || 0;

      return `
                <div>
                    <span class="badge badge-${color}">${progress.state.toUpperCase()}</span>
                    ${
                      progress.state !== "idle" &&
                      progress.state !== "completed" &&
                      progress.state !== "error"
                        ? `
                        <div style="margin-top: 2px;">
                            <div class="progress" style="height: 6px;">
                                <div class="progress-bar" role="progressbar" style="width: ${percentage}%"></div>
                            </div>
                            <small class="text-muted" style="font-size: 0.75em; line-height: 1.2;">${this.formatProgressText(progress.progress)}</small>
                        </div>
                    `
                        : ""
                    }
                    ${progress.error ? `<br><small class="text-danger">${this.escapeHtml(progress.error)}</small>` : ""}
                </div>
            `;
    }

    return `
            <span class="badge ${source.is_active ? "badge-success" : "badge-secondary"}">
                ${source.is_active ? "Active" : "Inactive"}
            </span>
        `;
  }

  renderActionsCell(source, progress) {
    const isIngesting =
      progress &&
      ["connecting", "downloading", "parsing", "saving", "processing"].includes(
        progress.state,
      );

    const processingInfo = this.processingInfo[source.id];
    const isInBackoff =
      processingInfo &&
      processingInfo.next_retry_after &&
      new Date(processingInfo.next_retry_after) > new Date();

    let refreshButtonContent;
    if (isIngesting) {
      refreshButtonContent = `<button class="btn btn-sm btn-warning" onclick="sourcesManager.cancelIngestion('${source.id}')" title="Cancel current operation">
                🔄 Cancel
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
      refreshButtonContent = `<button class="btn btn-sm btn-success" onclick="sourcesManager.refreshSource('${source.id}')" title="Force refresh this source">
                🔄 Refresh
            </button>`;
    }

    return `
            <button class="btn btn-sm btn-primary" onclick="sourcesManager.editSource('${source.id}')" ${isIngesting ? "disabled" : ""}>
                ✏️ Edit
            </button>
            ${refreshButtonContent}
            <button class="btn btn-sm btn-danger" onclick="sourcesManager.deleteSource('${source.id}')" ${isIngesting ? "disabled" : ""}>
                🗑️ Delete
            </button>
        `;
  }

  showSourceModal(source = null) {
    this.editingSource = source;
    const title = document.getElementById("modalTitle");
    const form = document.getElementById("sourceForm");

    title.textContent = source ? "Edit Stream Source" : "Add Stream Source";

    if (source) {
      document.getElementById("sourceName").value = source.name;
      document.getElementById("sourceType").value = source.source_type;
      document.getElementById("sourceUrl").value = source.url;
      document.getElementById("maxStreams").value =
        source.max_concurrent_streams;
      document.getElementById("updateCron").value = source.update_cron;
      document.getElementById("username").value = source.username || "";
      document.getElementById("password").value = source.password || "";
      document.getElementById("fieldMap").value = source.field_map || "";
      document.getElementById("isActive").checked = source.is_active;
    } else {
      form.reset();
      // Set default values after reset to override HTML defaults
      document.getElementById("maxStreams").value = 1;
      document.getElementById("updateCron").value = "0 0 */6 * * * *";
      document.getElementById("isActive").checked = true;
      document.getElementById("sourceType").value = "xtream";
      // Force the cron field to update by triggering an input event
      const cronField = document.getElementById("updateCron");
      cronField.dispatchEvent(new Event("input", { bubbles: true }));
    }

    this.toggleSourceTypeFields(document.getElementById("sourceType").value);
    SharedUtils.showStandardModal("sourceModal");
  }

  hideSourceModal() {
    SharedUtils.hideStandardModal("sourceModal");
    this.editingSource = null;
  }

  toggleSourceTypeFields(sourceType) {
    const xtreamFields = document.getElementById("xtreamFields");
    const m3uFields = document.getElementById("m3uFields");

    if (sourceType === "xtream") {
      xtreamFields.style.display = "block";
      m3uFields.style.display = "none";
    } else {
      xtreamFields.style.display = "none";
      m3uFields.style.display = "block";
    }
  }

  async saveSource() {
    try {
      const formData = this.getFormData();

      if (!this.validateForm(formData)) {
        return;
      }

      this.setModalLoading(true);

      let response;
      if (this.editingSource) {
        response = await fetch(
          `/api/v1/sources/stream/${this.editingSource.id}`,
          {
            method: "PUT",
            headers: {
              "Content-Type": "application/json",
            },
            body: JSON.stringify(formData),
          },
        );
      } else {
        response = await fetch("/api/v1/sources/stream", {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
          },
          body: JSON.stringify(formData),
        });
      }

      if (!response.ok) {
        const error = await response.text();
        throw new Error(error || "Failed to save source");
      }

      this.hideSourceModal();
      await this.loadSources();
      this.showAlert(
        `Source ${this.editingSource ? "updated" : "created"} successfully!`,
        "success",
      );
    } catch (error) {
      this.showAlert("Failed to save source: " + error.message, "danger");
      console.error("Error saving source:", error);
    } finally {
      this.setModalLoading(false);
    }
  }

  getFormData() {
    return {
      name: document.getElementById("sourceName").value.trim(),
      source_type: document.getElementById("sourceType").value,
      url: document.getElementById("sourceUrl").value.trim(),
      max_concurrent_streams: parseInt(
        document.getElementById("maxStreams").value,
      ),
      update_cron: document.getElementById("updateCron").value.trim(),
      username: document.getElementById("username").value.trim() || null,
      password: document.getElementById("password").value.trim() || null,
      field_map: document.getElementById("fieldMap").value.trim() || null,
      is_active: document.getElementById("isActive").checked,
    };
  }

  validateForm(data) {
    if (!data.name) {
      this.showAlert("Source name is required", "danger");
      return false;
    }

    if (!data.url) {
      this.showAlert("Source URL is required", "danger");
      return false;
    }

    if (data.source_type === "xtream" && (!data.username || !data.password)) {
      this.showAlert(
        "Username and password are required for Xtream sources",
        "danger",
      );
      return false;
    }

    if (data.max_concurrent_streams < 1) {
      this.showAlert("Max concurrent streams must be at least 1", "danger");
      return false;
    }

    return true;
  }

  async editSource(id) {
    const source = this.sources.find((s) => s.id === id);
    if (source) {
      this.showSourceModal(source);
    }
  }

  async refreshSource(id) {
    const sourceWithStats = this.sources.find((s) => (s.source || s).id === id);
    if (!sourceWithStats) return;

    const source = sourceWithStats.source || sourceWithStats;

    try {
      const response = await fetch(`/api/v1/sources/stream/${id}/refresh`, {
        method: "POST",
      });

      if (!response.ok) {
        throw new Error("Failed to refresh source");
      }

      const result = await response.json();

      if (result.success) {
        this.showAlert(`${result.message}`, "success");
        // Sources will be reloaded automatically when ingestion completes
      } else {
        this.showAlert(`Refresh failed: ${result.message}`, "danger");
      }
    } catch (error) {
      this.showAlert("Failed to refresh source: " + error.message, "danger");
      console.error("Error refreshing source:", error);
    }
  }

  async deleteSource(id) {
    const sourceWithStats = this.sources.find((s) => (s.source || s).id === id);
    if (!sourceWithStats) return;

    const source = sourceWithStats.source || sourceWithStats;

    if (
      !confirm(
        `Are you sure you want to delete "${source.name}"? This action cannot be undone.`,
      )
    ) {
      return;
    }

    try {
      const response = await fetch(`/api/v1/sources/stream/${id}`, {
        method: "DELETE",
      });

      if (!response.ok) {
        throw new Error("Failed to delete source");
      }

      await this.loadSources();
      this.showAlert("Source deleted successfully!", "success");
    } catch (error) {
      this.showAlert("Failed to delete source: " + error.message, "danger");
      console.error("Error deleting source:", error);
    }
  }

  // Channel viewing functionality
  async showChannels(sourceId, sourceName) {
    try {
      await window.channelsViewer.showChannels(sourceId, sourceName);
    } catch (error) {
      console.error("Error showing channels:", error);
      this.showAlert("Failed to show channels: " + error.message, "danger");
    }
  }

  showAlert(message, type = "info") {
    const alertsContainer = document.getElementById("alertsContainer");
    const alert = document.createElement("div");
    alert.className = `alert alert-${type}`;
    alert.textContent = message;

    alertsContainer.appendChild(alert);

    setTimeout(() => {
      alert.remove();
    }, 5000);
  }

  showLoading() {
    document.getElementById("loadingIndicator").style.display = "block";
    document.getElementById("sourcesTable").style.opacity = "0.5";
  }

  hideLoading() {
    document.getElementById("loadingIndicator").style.display = "none";
    document.getElementById("sourcesTable").style.opacity = "1";
  }

  setModalLoading(loading) {
    const saveBtn = document.getElementById("saveSource");
    const cancelBtn = document.getElementById("cancelSource");

    if (loading) {
      saveBtn.disabled = true;
      saveBtn.innerHTML = '<span class="loading"></span> Saving...';
      cancelBtn.disabled = true;
    } else {
      saveBtn.disabled = false;
      saveBtn.textContent = "Save";
      cancelBtn.disabled = false;
    }
  }

  async startProgressPolling() {
    // Poll every 2 seconds for progress updates
    this.progressPollingInterval = setInterval(async () => {
      await this.loadProgress();
    }, 2000);
  }

  stopProgressPolling() {
    if (this.progressPollingInterval) {
      clearInterval(this.progressPollingInterval);
      this.progressPollingInterval = null;
    }
  }

  async loadProgress() {
    try {
      const response = await fetch("/api/v1/progress/sources");
      if (!response.ok) return;

      const data = await response.json();
      const newProgressData = data.progress || {};

      // Extract progress and processing info from consolidated response
      const extractedProgress = {};
      const extractedProcessingInfo = {};

      Object.entries(newProgressData).forEach(([sourceId, data]) => {
        if (data.progress) {
          extractedProgress[sourceId] = data.progress;
        }
        if (data.processing_info) {
          extractedProcessingInfo[sourceId] = data.processing_info;
        }
      });

      // Check if any sources just completed
      const justCompleted = Object.keys(extractedProgress).filter(
        (sourceId) => {
          const oldProgress = this.progressData[sourceId];
          const newProgress = extractedProgress[sourceId];
          return (
            oldProgress &&
            [
              "connecting",
              "downloading",
              "parsing",
              "saving",
              "processing",
            ].includes(oldProgress.state) &&
            newProgress.state === "completed"
          );
        },
      );

      this.progressData = extractedProgress;
      this.processingInfo = extractedProcessingInfo;

      // Reload sources if any just completed
      if (justCompleted.length > 0) {
        await this.loadSources();
      }

      // Only re-render if there's actual progress to show
      const hasActiveProgress = Object.values(extractedProgress).some((p) =>
        [
          "connecting",
          "downloading",
          "parsing",
          "saving",
          "processing",
        ].includes(p.state),
      );

      if (hasActiveProgress || this.lastHadProgress) {
        this.renderSources();
        this.lastHadProgress = hasActiveProgress;
      }
    } catch (error) {
      // Silently ignore progress polling errors
      console.debug("Progress polling error:", error);
    }
  }

  getSourceProgress(sourceId) {
    return this.progressData ? this.progressData[sourceId] : null;
  }

  async loadProcessingInfo() {
    // This method is now handled by loadProgress() which gets consolidated data
    // Keeping this method for backward compatibility but it's no longer needed
    console.debug(
      "loadProcessingInfo() called but processing info is now loaded via consolidated progress endpoint",
    );
  }

  escapeHtml(text) {
    return SharedUtils.escapeHtml(text);
  }

  parseDateTime(dateStr) {
    return SharedUtils.parseDateTime(dateStr);
  }

  formatTimeCompact(date) {
    return SharedUtils.formatTimeCompact(date);
  }

  formatBytes(bytes) {
    return SharedUtils.formatFileSize(bytes);
  }

  formatProgressText(progress) {
    let text = progress.current_step;

    // Simplify "Processing channels into database" message
    if (text.includes("Processing channels into database")) {
      // Extract current/total from text like "Processing channels into database (1060000/1205218)"
      const match = text.match(/\((\d+)\/(\d+)\)/);
      if (match) {
        const current = parseInt(match[1]).toLocaleString();
        const total = parseInt(match[2]).toLocaleString();
        text = `Processing... ${current}/${total}`;
      } else {
        text = "Processing...";
      }
    } else {
      // Handle other progress messages
      if (progress.downloaded_bytes && progress.total_bytes) {
        const downloaded = this.formatBytes(progress.downloaded_bytes);
        const total = this.formatBytes(progress.total_bytes);
        const percentage = (
          (progress.downloaded_bytes / progress.total_bytes) *
          100
        ).toFixed(1);
        text += ` - ${downloaded} / ${total} (${percentage}%)`;
      } else if (progress.downloaded_bytes) {
        text += ` - ${this.formatBytes(progress.downloaded_bytes)}`;
      }

      if (progress.channels_parsed && !text.includes("Processing")) {
        text += ` - ${progress.channels_parsed} channels`;
      }
    }

    return text;
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

  async cancelIngestion(id) {
    const sourceWithStats = this.sources.find((s) => (s.source || s).id === id);
    if (!sourceWithStats) return false;

    const source = sourceWithStats.source || sourceWithStats;

    try {
      const response = await fetch(`/api/v1/sources/stream/${id}/cancel`, {
        method: "POST",
      });

      if (!response.ok) {
        throw new Error("Failed to cancel ingestion");
      }

      const result = await response.json();
      if (result.success) {
        this.showAlert(`Cancelled ingestion for "${source.name}"`, "info");
        // Refresh the UI to update button states
        await this.loadProgress();
        this.renderSources();
        return true;
      } else {
        this.showAlert(`Failed to cancel: ${result.message}`, "warning");
        return false;
      }
    } catch (error) {
      this.showAlert("Failed to cancel ingestion: " + error.message, "danger");
      console.error("Error cancelling ingestion:", error);
      return false;
    }
  }

  async previewDataMapping(sourceId, sourceType) {
    // Find the preview button for this source
    const button = document.querySelector(
      `button[onclick="sourcesManager.previewDataMapping('${sourceId}', '${sourceType}')"]`,
    );
    const originalText = button ? button.innerHTML : "";

    try {
      // Show loading state
      if (button) {
        button.disabled = true;
        button.innerHTML = "⏳ Loading...";
      }

      const response = await fetch(
        `/api/v1/sources/${sourceType}/${sourceId}/data-mapping/preview`,
      );

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const result = await response.json();
      this.displayPreviewResults(
        `Data Mapping Preview - ${sourceType} Source`,
        result,
      );
    } catch (error) {
      console.error("Preview failed:", error);
      this.showAlert(
        "Failed to preview data mapping rules: " + error.message,
        "danger",
      );
    } finally {
      // Restore button state
      if (button) {
        button.disabled = false;
        button.innerHTML = originalText;
      }
    }
  }

  displayPreviewResults(title, result) {
    const totalChannels = result.original_count || 0;
    const affectedChannels = result.mapped_count || 0;
    const channels = result.final_channels || [];

    // Create modal HTML using application's standard modal structure
    const modalHtml = `
      <div class="modal" id="previewModal" style="display: none">
        <div class="modal-content standard-modal preview-modal-large">
          <div class="modal-header">
            <h3 class="modal-title">${title}</h3>
          </div>
            <div class="modal-body">
              <div class="mb-3">
                <div class="alert alert-info">
                  <strong>Summary:</strong> ${affectedChannels} channels modified out of ${totalChannels} total channels
                </div>
              </div>

              ${
                channels.length > 0
                  ? `
              <div class="preview-channels">
                <h6>Modified Channels:</h6>
                <div class="table-responsive">
                  <table class="table table-sm">
                    <thead>
                      <tr>
                        <th>Channel Name</th>
                        <th>Changes</th>
                        <th>Applied Rules</th>
                      </tr>
                    </thead>
                    <tbody>
                      ${channels
                        .map((channel) => {
                          const changes = [];

                          // Check for changes between original and mapped values
                          if (
                            channel.original_channel_name !==
                            channel.mapped_channel_name
                          ) {
                            changes.push(
                              `Name: "${channel.original_channel_name}" → "${channel.mapped_channel_name}"`,
                            );
                          }
                          if (
                            channel.original_tvg_id !== channel.mapped_tvg_id
                          ) {
                            changes.push(
                              `TVG ID: "${channel.original_tvg_id || "null"}" → "${channel.mapped_tvg_id || "null"}"`,
                            );
                          }
                          if (
                            channel.original_tvg_shift !==
                            channel.mapped_tvg_shift
                          ) {
                            changes.push(
                              `TVG Shift: "${channel.original_tvg_shift || "null"}" → "${channel.mapped_tvg_shift || "null"}"`,
                            );
                          }
                          if (
                            channel.original_group_title !==
                            channel.mapped_group_title
                          ) {
                            changes.push(
                              `Group: "${channel.original_group_title || "null"}" → "${channel.mapped_group_title || "null"}"`,
                            );
                          }
                          if (
                            channel.original_tvg_logo !==
                            channel.mapped_tvg_logo
                          ) {
                            changes.push(
                              `Logo: "${channel.original_tvg_logo || "null"}" → "${channel.mapped_tvg_logo || "null"}"`,
                            );
                          }

                          return `
                          <tr>
                            <td><strong>${this.escapeHtml(channel.channel_name)}</strong></td>
                            <td>
                              ${
                                changes.length > 0
                                  ? changes
                                      .map(
                                        (change) =>
                                          `<div class="small text-muted">${this.escapeHtml(change)}</div>`,
                                      )
                                      .join("")
                                  : '<span class="text-muted">No visible changes</span>'
                              }
                            </td>
                            <td>
                              ${
                                channel.applied_rules &&
                                channel.applied_rules.length > 0
                                  ? `<span class="badge badge-primary">${channel.applied_rules.length} rule(s)</span>`
                                  : '<span class="text-muted">No rules</span>'
                              }
                            </td>
                          </tr>
                        `;
                        })
                        .join("")}
                    </tbody>
                  </table>
                </div>
              </div>
              `
                  : `
              <div class="alert alert-warning">
                No channels were modified by the current rules.
              </div>
              `
              }
            </div>
            <div class="modal-footer">
              <button type="button" class="btn btn-secondary" onclick="sourcesManager.closePreviewModal()">Close</button>
            </div>
          </div>
        </div>
    `;

    // Remove any existing modal
    const existingModal = document.getElementById("previewModal");
    if (existingModal) {
      existingModal.remove();
    }

    // Add modal to DOM
    document.body.insertAdjacentHTML("beforeend", modalHtml);

    // Show modal using application's standard approach
    const modal = document.getElementById("previewModal");
    if (modal) {
      modal.style.display = "flex";
      modal.style.alignItems = "center";
      modal.style.justifyContent = "center";
      modal.classList.add("show");
      document.body.classList.add("modal-open");
    }
  }

  closePreviewModal() {
    const modal = document.getElementById("previewModal");
    if (modal) {
      modal.classList.remove("show");
      modal.style.display = "none";
      modal.style.alignItems = "";
      modal.style.justifyContent = "";
      document.body.classList.remove("modal-open");
      modal.remove();
    }
  }
}

// Password visibility toggle function
function togglePassword(fieldId) {
  const passwordField = document.getElementById(fieldId);
  const toggleButton = passwordField.nextElementSibling;

  if (passwordField.type === "password") {
    passwordField.type = "text";
    toggleButton.classList.add("hidden");
  } else {
    passwordField.type = "password";
    toggleButton.classList.remove("hidden");
  }
}

// Initialize when page loads
let sourcesManager;
document.addEventListener("DOMContentLoaded", () => {
  sourcesManager = new SourcesManager();
  window.sourcesManager = sourcesManager;

  // Setup standard modal close handlers
  SharedUtils.setupStandardModalCloseHandlers("sourceModal");
});
