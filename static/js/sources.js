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

    // Modal close
    document.getElementById("closeModal").addEventListener("click", () => {
      this.hideSourceModal();
    });

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

    // Click outside modal to close
    document.getElementById("sourceModal").addEventListener("click", (e) => {
      if (e.target.id === "sourceModal") {
        this.hideSourceModal();
      }
    });

    // Channels modal functionality is now handled by shared channels-viewer.js
  }

  async loadSources() {
    try {
      this.showLoading();
      const response = await fetch("/api/sources");

      if (!response.ok) {
        throw new Error("Failed to load sources");
      }

      this.sources = await response.json();
      await this.loadProcessingInfo();
      this.renderSources();
    } catch (error) {
      this.showAlert("Failed to load sources: " + error.message, "danger");
      console.error("Error loading sources:", error);
    } finally {
      this.hideLoading();
    }
  }

  renderSources() {
    const tbody = document.getElementById("sourcesTableBody");

    if (this.sources.length === 0) {
      tbody.innerHTML = `
                <tr>
                    <td colspan="7" class="text-center text-muted">
                        No stream sources configured. Click "Add Source" to get started.
                    </td>
                </tr>
            `;
      return;
    }

    tbody.innerHTML = this.sources
      .map((sourceWithStats) => {
        const progress = this.getSourceProgress(sourceWithStats.id);
        const statusCell = this.renderStatusCell(sourceWithStats, progress);
        const actionsCell = this.renderActionsCell(sourceWithStats, progress);

        const typeIndicator =
          sourceWithStats.source_type === "m3u" ? "M3U" : "XC";

        return `
                <tr>
                    <td>
                        <strong>${this.escapeHtml(sourceWithStats.name)}<sup class="text-muted" style="font-size: 0.7em; margin-left: 3px;">${typeIndicator}</sup></strong>
                        <br>
                        <small class="text-muted">${this.escapeHtml(sourceWithStats.url)}</small>
                    </td>
                    <td>
                        <button class="btn btn-link p-0 text-primary" onclick="channelsViewer.showChannels('${sourceWithStats.id}', '${this.escapeHtml(sourceWithStats.name)}')" title="View channels">
                            ${sourceWithStats.channel_count.toLocaleString()} ${sourceWithStats.channel_count === 1 ? "channel" : "channels"}
                        </button>
                    </td>
                    <td>${statusCell}</td>
                    <td>
                        <div class="update-badges">
                            <div class="badge badge-secondary badge-sm">
                                Last: ${
                                  sourceWithStats.last_ingested_at
                                    ? this.formatTimeCompact(
                                        this.parseDateTime(
                                          sourceWithStats.last_ingested_at,
                                        ),
                                      )
                                    : "Never"
                                }
                            </div>
                            <div class="badge badge-info badge-sm">
                                Next: ${
                                  sourceWithStats.next_scheduled_update
                                    ? this.formatTimeCompact(
                                        this.parseDateTime(
                                          sourceWithStats.next_scheduled_update,
                                        ),
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
      refreshButtonContent = `<button class="btn btn-sm btn-warning" onclick="sourcesManager.cancelSourceIngestion('${source.id}')" title="Cancel current operation">
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
    const modal = document.getElementById("sourceModal");
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
      // Force the cron field to update by triggering an input event
      const cronField = document.getElementById("updateCron");
      cronField.dispatchEvent(new Event("input", { bubbles: true }));
    }

    this.toggleSourceTypeFields(document.getElementById("sourceType").value);
    modal.classList.add("show");
  }

  hideSourceModal() {
    document.getElementById("sourceModal").classList.remove("show");
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
        response = await fetch(`/api/sources/${this.editingSource.id}`, {
          method: "PUT",
          headers: {
            "Content-Type": "application/json",
          },
          body: JSON.stringify(formData),
        });
      } else {
        response = await fetch("/api/sources", {
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
    const source = this.sources.find((s) => s.id === id);
    if (!source) return;

    if (
      !confirm(
        `Force refresh "${source.name}"? This will immediately fetch and update channels from the source.`,
      )
    ) {
      return;
    }

    try {
      const response = await fetch(`/api/sources/${id}/refresh`, {
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
    const source = this.sources.find((s) => s.id === id);
    if (!source) return;

    if (
      !confirm(
        `Are you sure you want to delete "${source.name}"? This action cannot be undone.`,
      )
    ) {
      return;
    }

    try {
      const response = await fetch(`/api/sources/${id}`, {
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

  // Channel viewing functionality moved to shared channels-viewer.js

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
      const response = await fetch("/api/progress");
      if (!response.ok) return;

      const newProgressData = await response.json();

      // Check if any sources just completed
      const justCompleted = Object.keys(newProgressData).filter((sourceId) => {
        const oldProgress = this.progressData[sourceId];
        const newProgress = newProgressData[sourceId];
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
      });

      this.progressData = newProgressData;

      // Also load processing info for backoff states
      await this.loadProcessingInfo();

      // Reload sources if any just completed
      if (justCompleted.length > 0) {
        await this.loadSources();
      }

      // Only re-render if there's actual progress to show
      const hasActiveProgress = Object.values(newProgressData).some((p) =>
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
    try {
      // Load processing info for all sources to get backoff states
      const promises = this.sources.map(async (source) => {
        try {
          const response = await fetch(`/api/sources/${source.id}/processing`);
          if (response.ok) {
            const processingInfo = await response.json();
            if (processingInfo) {
              this.processingInfo[source.id] = processingInfo;
            }
          }
        } catch (error) {
          // Silently ignore individual source errors
          console.debug(
            `Failed to load processing info for ${source.id}:`,
            error,
          );
        }
      });

      await Promise.all(promises);
    } catch (error) {
      console.debug("Processing info loading error:", error);
    }
  }

  escapeHtml(text) {
    const div = document.createElement("div");
    div.textContent = text;
    return div.innerHTML;
  }

  parseDateTime(dateStr) {
    // Handle RFC3339 format with potential timezone issues
    if (dateStr) {
      // Replace +00:00 with Z for better JavaScript compatibility
      const normalizedDateStr = dateStr.replace(/\+00:00$/, "Z");
      const date = new Date(normalizedDateStr);

      // If that fails, try removing nanoseconds
      if (isNaN(date.getTime()) && dateStr.includes(".")) {
        const withoutNanos = dateStr
          .replace(/\.\d+/, "")
          .replace(/\+00:00$/, "Z");
        return new Date(withoutNanos);
      }

      return date;
    }
    return null;
  }

  formatTimeCompact(date) {
    // Ensure we have a valid date object
    if (!date || isNaN(date.getTime())) {
      return "Invalid date";
    }

    const now = new Date();
    const diffMs = now.getTime() - date.getTime(); // now - date for proper past/future calculation
    const diffDays = Math.floor(Math.abs(diffMs) / (1000 * 60 * 60 * 24));
    const diffHours = Math.floor(Math.abs(diffMs) / (1000 * 60 * 60));
    const diffMins = Math.floor(Math.abs(diffMs) / (1000 * 60));

    // For past dates (positive diffMs means date is in the past)
    if (diffMs > 0) {
      if (diffDays > 0) {
        return `${diffDays}d ago`;
      } else if (diffHours > 0) {
        return `${diffHours}h ago`;
      } else if (diffMins > 5) {
        return `${diffMins}m ago`;
      } else {
        return "Just now";
      }
    }

    // For future dates (negative diffMs means date is in the future)
    if (diffDays > 0) {
      return `in ${diffDays}d`;
    } else if (diffHours > 0) {
      return `in ${diffHours}h`;
    } else if (diffMins > 5) {
      return `in ${diffMins}m`;
    } else {
      return "Soon";
    }
  }

  formatBytes(bytes) {
    if (bytes === 0) return "0 B";
    if (!bytes) return "";

    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));

    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
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

  async cancelSourceIngestion(id) {
    const source = this.sources.find((s) => s.id === id);
    if (!source) return false;

    try {
      const response = await fetch(`/api/sources/${id}/cancel`, {
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
});
