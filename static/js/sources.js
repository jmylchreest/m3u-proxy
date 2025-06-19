// Stream Sources Management JavaScript

class SourcesManager {
  constructor() {
    this.sources = [];
    this.editingSource = null;
    this.progressPollingInterval = null;
    this.progressData = {};
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

    // Channels modal click outside to close
    document.getElementById("channelsModal").addEventListener("click", (e) => {
      if (e.target.id === "channelsModal") {
        this.hideChannelsModal();
      }
    });

    // Prevent modal content clicks from bubbling up
    document
      .querySelector("#channelsModal .modal-content")
      .addEventListener("click", (e) => {
        e.stopPropagation();
      });
  }

  async loadSources() {
    try {
      this.showLoading();
      const response = await fetch("/api/sources");

      if (!response.ok) {
        throw new Error("Failed to load sources");
      }

      this.sources = await response.json();
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
        const source = sourceWithStats.source || sourceWithStats; // Handle both new and old format
        const channelCount =
          sourceWithStats.channel_count !== undefined
            ? sourceWithStats.channel_count
            : 0;
        const progress = this.getSourceProgress(source.id);
        const statusCell = this.renderStatusCell(source, progress);
        const actionsCell = this.renderActionsCell(source, progress);

        return `
                <tr>
                    <td>
                        <strong>${this.escapeHtml(source.name)}</strong>
                        <br>
                        <small class="text-muted">${this.escapeHtml(source.url)}</small>
                    </td>
                    <td>${source.source_type.toUpperCase()}</td>
                    <td>${source.max_concurrent_streams}</td>
                    <td>
                        <button class="btn btn-link p-0 text-primary" onclick="sourcesManager.showChannels('${source.id}', '${this.escapeHtml(source.name)}')" title="View channels">
                            ${channelCount.toLocaleString()} ${channelCount === 1 ? "channel" : "channels"}
                        </button>
                    </td>
                    <td>${statusCell}</td>
                    <td>
                        ${
                          source.last_ingested_at
                            ? new Date(source.last_ingested_at).toLocaleString()
                            : '<span class="text-muted">Never</span>'
                        }
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
                            <div class="progress" style="height: 4px;">
                                <div class="progress-bar" role="progressbar" style="width: ${percentage}%"></div>
                            </div>
                            <small class="text-muted">${progress.progress.current_step}</small>
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
      ["connecting", "downloading", "parsing", "saving"].includes(
        progress.state,
      );

    return `
            <button class="btn btn-sm btn-outline-primary" onclick="sourcesManager.editSource('${source.id}')" ${isIngesting ? "disabled" : ""}>
                Edit
            </button>
            <button class="btn btn-sm btn-outline-secondary" onclick="sourcesManager.refreshSource('${source.id}')" title="Force refresh this source" ${isIngesting ? "disabled" : ""}>
                ${isIngesting ? "⏳" : "🔄"} Refresh
            </button>
            <button class="btn btn-sm btn-danger" onclick="sourcesManager.deleteSource('${source.id}')" ${isIngesting ? "disabled" : ""}>
                Delete
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
      document.getElementById("maxStreams").value = 1;
      document.getElementById("updateCron").value = "0 */6 * * *";
      document.getElementById("isActive").checked = true;
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

  async showChannels(sourceId, sourceName) {
    try {
      const modal = document.getElementById("channelsModal");
      const title = document.getElementById("channelsModalTitle");
      const loading = document.getElementById("channelsLoading");
      const content = document.getElementById("channelsContent");
      const filterInput = document.getElementById("channelsFilter");

      title.textContent = `Channels - ${sourceName}`;
      loading.style.display = "block";
      content.style.display = "none";
      filterInput.value = "";
      modal.classList.add("show");

      // Reset pagination state
      this.currentPage = 1;
      this.currentFilter = "";
      this.currentSourceId = sourceId;
      this.currentChannels = [];
      this.renderedChannels = 0;

      // Ensure the modal is properly focused and interactive
      setTimeout(() => {
        const filterInput = document.getElementById("channelsFilter");
        if (filterInput) {
          filterInput.focus();
        }
      }, 100);

      // Load first page of channels
      await this.loadChannelsPage(1, "");

      // Set up scroll container for progressive loading
      this.scrollContainer = document.querySelector(
        ".channels-table-container",
      );

      console.log("Scroll container found:", this.scrollContainer);

      this.renderChannelsProgressively();
      this.updateChannelCount();

      loading.style.display = "none";
      content.style.display = "block";

      // Force scroll container to be scrollable - do this after content is visible
      setTimeout(() => {
        if (this.scrollContainer) {
          console.log("Setting up scroll container...");
          this.scrollContainer.style.overflowY = "scroll";
          this.scrollContainer.style.height = "60vh";
          this.scrollContainer.style.maxHeight = "60vh";
          this.scrollContainer.style.display = "block";

          // Force a reflow
          this.scrollContainer.offsetHeight;

          console.log(
            "Scroll container height:",
            this.scrollContainer.scrollHeight,
          );
          console.log(
            "Container client height:",
            this.scrollContainer.clientHeight,
          );

          this.setupProgressiveScrolling();
        }
      }, 100);
    } catch (error) {
      this.showAlert("Failed to load channels: " + error.message, "danger");
      this.hideChannelsModal();
      console.error("Error loading channels:", error);
    }
  }

  setupProgressiveScrolling() {
    if (this.scrollContainer) {
      // Remove existing scroll listener
      this.scrollContainer.removeEventListener(
        "scroll",
        this.handleScroll.bind(this),
      );

      // Add scroll listener for progressive loading
      this.scrollContainer.addEventListener(
        "scroll",
        this.handleScroll.bind(this),
        { passive: true },
      );

      // Add wheel event listener for better mouse scroll support
      this.scrollContainer.addEventListener(
        "wheel",
        (e) => {
          // Allow normal scrolling behavior
          e.stopPropagation();
        },
        { passive: true },
      );

      // Add keyboard event support for arrow keys
      this.scrollContainer.addEventListener("keydown", (e) => {
        if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          e.preventDefault();
          const scrollAmount = 50;
          this.scrollContainer.scrollTop +=
            e.key === "ArrowDown" ? scrollAmount : -scrollAmount;
        }
      });

      // Make container focusable for keyboard events
      this.scrollContainer.setAttribute("tabindex", "0");
    }
  }

  handleScroll() {
    if (!this.scrollContainer) return;

    const { scrollTop, scrollHeight, clientHeight } = this.scrollContainer;

    // Load more when user scrolls to within 200px of bottom
    if (scrollTop + clientHeight >= scrollHeight - 200) {
      this.loadMoreChannels();
    }
  }

  renderChannelsProgressively() {
    const tbody = document.getElementById("channelsTableBody");

    if (this.filteredChannels.length === 0) {
      tbody.innerHTML = `
                <tr>
                    <td colspan="4" class="text-center text-muted">
                        No channels found.
                    </td>
                </tr>
            `;
      this.updateChannelCount();
      return;
    }

    // Clear existing content and reset rendered count
    tbody.innerHTML = "";
    this.renderedChannels = 0;

    // Load first batch
    this.loadMoreChannels();
    this.updateChannelCount();

    // Ensure scroll container is properly configured
    setTimeout(() => {
      if (this.scrollContainer) {
        this.scrollContainer.style.overflowY = "scroll";
        this.scrollContainer.scrollTop = 0; // Reset scroll position
      }
    }, 50);
  }

  async loadMoreChannels() {
    const tbody = document.getElementById("channelsTableBody");
    const startIndex = this.renderedChannels;
    const endIndex = Math.min(
      startIndex + this.channelsPerBatch,
      this.filteredChannels.length,
    );

    // If we've rendered all current channels and there are more pages, load next page
    if (
      startIndex >= this.filteredChannels.length &&
      this.currentPage < this.totalPages
    ) {
      try {
        await this.loadChannelsPage(this.currentPage + 1, this.currentFilter);
        // After loading new page, try again
        this.loadMoreChannels();
        return;
      } catch (error) {
        console.error("Error loading more channels:", error);
        return;
      }
    }

    if (startIndex >= this.filteredChannels.length) {
      return; // No more channels to load
    }

    // Remove existing loading row if present
    const existingLoadingRow = document.getElementById("loadingMoreRow");
    if (existingLoadingRow) {
      existingLoadingRow.remove();
    }

    const channelsToRender = this.filteredChannels.slice(startIndex, endIndex);

    // Render channels in batch
    const fragment = document.createDocumentFragment();

    channelsToRender.forEach((channel) => {
      const row = document.createElement("tr");
      row.innerHTML = `
                <td>
                    <strong>${this.escapeHtml(channel.channel_name)}</strong>
                    ${
                      channel.tvg_name &&
                      channel.tvg_name !== channel.channel_name
                        ? `<br><small class="text-muted">TVG: ${this.escapeHtml(channel.tvg_name)}</small>`
                        : ""
                    }
                </td>
                <td>${channel.group_title ? this.escapeHtml(channel.group_title) : '<span class="text-muted">-</span>'}</td>
                <td>${channel.tvg_id ? this.escapeHtml(channel.tvg_id) : '<span class="text-muted">-</span>'}</td>
                <td>
                    ${
                      channel.tvg_logo
                        ? `<img src="${this.escapeHtml(channel.tvg_logo)}" alt="Logo" style="max-width: 40px; max-height: 40px;" onerror="this.style.display='none'">`
                        : '<span class="text-muted">-</span>'
                    }
                </td>
            `;
      fragment.appendChild(row);
    });

    tbody.appendChild(fragment);
    this.renderedChannels = endIndex;

    // Add loading indicator if there are more channels (either in current data or more pages)
    if (
      this.renderedChannels < this.filteredChannels.length ||
      this.currentPage < this.totalPages
    ) {
      const loadingRow = document.createElement("tr");
      loadingRow.id = "loadingMoreRow";
      loadingRow.innerHTML = `
                <td colspan="4" class="text-center text-muted" style="padding: 1rem;">
                    <span class="loading"></span> Loading more channels...
                </td>
            `;
      tbody.appendChild(loadingRow);
    }
  }

  async loadChannelsPage(page, filter) {
    const params = new URLSearchParams({
      page: page.toString(),
      limit: "10000",
    });

    if (filter) {
      params.append("filter", filter);
    }

    const response = await fetch(
      `/api/sources/${this.currentSourceId}/channels?${params}`,
    );

    if (!response.ok) {
      throw new Error("Failed to load channels");
    }

    const data = await response.json();

    this.currentPage = data.page;
    this.totalPages = data.total_pages;
    this.totalCount = data.total_count;
    this.currentFilter = filter;

    if (page === 1) {
      // First page - replace all channels
      this.filteredChannels = data.channels;
      this.renderedChannels = 0;
    } else {
      // Subsequent pages - append channels
      this.filteredChannels = [...this.filteredChannels, ...data.channels];
    }

    return data;
  }

  updateChannelCount() {
    document.getElementById("channelsCount").textContent =
      `${this.filteredChannels.length} of ${this.totalCount} channels`;
  }

  async filterChannels() {
    const filter = document
      .getElementById("channelsFilter")
      .value.toLowerCase()
      .trim();

    // If filter hasn't changed, don't reload
    if (filter === this.currentFilter) {
      return;
    }

    try {
      // Show loading while filtering
      const tbody = document.getElementById("channelsTableBody");
      tbody.innerHTML = `
        <tr>
          <td colspan="4" class="text-center text-muted" style="padding: 2rem;">
            <span class="loading"></span> Filtering channels...
          </td>
        </tr>
      `;

      // Load filtered results from server
      await this.loadChannelsPage(1, filter);

      // Re-render progressively with filtered results
      this.renderChannelsProgressively();
      this.updateChannelCount();
    } catch (error) {
      console.error("Error filtering channels:", error);
      this.showAlert("Failed to filter channels: " + error.message, "danger");
    }
  }

  hideChannelsModal() {
    document.getElementById("channelsModal").classList.remove("show");

    // Clean up progressive loading
    if (this.scrollContainer) {
      this.scrollContainer.removeEventListener(
        "scroll",
        this.handleScroll.bind(this),
      );
      this.scrollContainer.removeEventListener(
        "wheel",
        this.handleScroll.bind(this),
      );
      this.scrollContainer.removeEventListener(
        "keydown",
        this.handleScroll.bind(this),
      );
      this.scrollContainer.removeAttribute("tabindex");
      this.scrollContainer = null;
    }

    this.currentChannels = [];
    this.filteredChannels = [];
    this.renderedChannels = 0;
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
      const response = await fetch("/api/progress");
      if (!response.ok) return;

      const newProgressData = await response.json();

      // Check if any sources just completed
      const justCompleted = Object.keys(newProgressData).filter((sourceId) => {
        const oldProgress = this.progressData[sourceId];
        const newProgress = newProgressData[sourceId];
        return (
          oldProgress &&
          ["connecting", "downloading", "parsing", "saving"].includes(
            oldProgress.state,
          ) &&
          newProgress.state === "completed"
        );
      });

      this.progressData = newProgressData;

      // Reload sources if any just completed
      if (justCompleted.length > 0) {
        await this.loadSources();
      }

      // Only re-render if there's actual progress to show
      const hasActiveProgress = Object.values(newProgressData).some((p) =>
        ["connecting", "downloading", "parsing", "saving"].includes(p.state),
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

  escapeHtml(text) {
    const div = document.createElement("div");
    div.textContent = text;
    return div.innerHTML;
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
