// Shared Channels Viewer Module
class ChannelsViewer {
  constructor() {
    this.currentChannels = [];
    this.filteredChannels = [];
    this.renderedChannels = 0;
    this.currentFilter = "";
    this.currentSourceId = null;
    this.scrollContainer = null;
    this.isLoading = false;
    this.batchSize = 50;
    this.closeCallback = null;
  }

  async init() {
    this.setupEventListeners();
  }

  setupEventListeners() {
    // Close button
    document
      .getElementById("channelsModalCloseFooterBtn")
      .addEventListener("click", () => {
        this.hideModal();
      });

    // Search filter with debouncing
    const filterInput = document.getElementById("channelsFilter");
    let filterTimeout;
    filterInput.addEventListener("input", () => {
      clearTimeout(filterTimeout);
      filterTimeout = setTimeout(() => {
        this.filterChannels();
      }, 300); // 300ms debounce
    });
  }

  async showChannels(sourceId, sourceName, preFilteredChannels = null) {
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
      SharedUtils.showStandardModal("channelsModal");

      // Reset state
      this.currentFilter = "";
      this.currentSourceId = sourceId;
      this.renderedChannels = 0;

      // If pre-filtered channels are provided (e.g., from filter test), use them
      if (preFilteredChannels) {
        this.currentChannels = preFilteredChannels;
      } else {
        // Load channels from API
        await this.loadChannelsFromAPI(sourceId);
      }

      this.filteredChannels = [...this.currentChannels];

      // Ensure the modal is properly focused and interactive
      setTimeout(() => {
        const filterInput = document.getElementById("channelsFilter");
        if (filterInput) {
          filterInput.focus();
        }
      }, 100);

      this.renderAllChannels();
      this.updateChannelCount();

      loading.style.display = "none";
      content.style.display = "block";
    } catch (error) {
      console.error("Error loading channels:", error);
      this.hideModal();
      throw error;
    }
  }

  async loadChannelsFromAPI(sourceId) {
    let allChannels = [];
    let page = 1;
    let totalPages = 1;

    // Load all pages of channels
    do {
      const response = await fetch(
        `/api/sources/stream/${sourceId}/channels?page=${page}&limit=10000`,
      );
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
      const data = await response.json();

      // Handle both paginated response and direct array response
      if (data.channels) {
        allChannels = allChannels.concat(data.channels);
        totalPages = data.total_pages || 1;
      } else if (Array.isArray(data)) {
        allChannels = data;
        totalPages = 1; // No pagination
      }

      page++;
    } while (page <= totalPages);

    this.currentChannels = allChannels;
    console.log(
      `Loaded ${allChannels.length} channels from ${totalPages} pages`,
    );
  }

  setupScrollContainer() {
    this.scrollContainer = document.querySelector(".channels-table-container");

    if (this.scrollContainer) {
      this.scrollContainer.style.overflowY = "scroll";
      this.scrollContainer.style.height = "60vh";
      this.scrollContainer.style.maxHeight = "60vh";
      this.scrollContainer.style.display = "block";

      // Force a reflow
      this.scrollContainer.offsetHeight;

      this.setupProgressiveScrolling();
    }
  }

  setupProgressiveScrolling() {
    if (this.scrollContainer) {
      // Remove existing listener if any
      this.scrollContainer.removeEventListener(
        "scroll",
        this.handleScroll.bind(this),
      );

      // Add scroll listener
      this.scrollContainer.addEventListener(
        "scroll",
        this.handleScroll.bind(this),
      );
    }
  }

  handleScroll() {
    if (this.isLoading) return;

    const container = this.scrollContainer;
    const scrollTop = container.scrollTop;
    const scrollHeight = container.scrollHeight;
    const clientHeight = container.clientHeight;

    // Load more when scrolled to 80% of the way down
    if (scrollTop + clientHeight >= scrollHeight * 0.8) {
      this.loadMoreChannels();
    }
  }

  renderAllChannels() {
    console.log(
      "Rendering all channels, filtered count:",
      this.filteredChannels.length,
    );
    const tbody = document.getElementById("channelsTableBody");

    if (this.filteredChannels.length === 0) {
      tbody.innerHTML =
        '<tr><td colspan="3" class="text-center text-muted">No channels found</td></tr>';
      return;
    }

    // Render all channels at once
    const fragment = document.createDocumentFragment();

    this.filteredChannels.forEach((channel) => {
      const row = document.createElement("tr");
      row.innerHTML = `
        <td>
          <strong>${this.escapeHtml(channel.channel_name)}</strong>
          ${
            channel.tvg_name && channel.tvg_name !== channel.channel_name
              ? `<br><small class="text-muted">TVG: ${this.escapeHtml(channel.tvg_name)}</small>`
              : ""
          }
        </td>
        <td>${channel.group_title ? this.escapeHtml(channel.group_title) : '<span class="text-muted">-</span>'}</td>
        <td>${channel.tvg_id ? this.escapeHtml(channel.tvg_id) : '<span class="text-muted">-</span>'}</td>
      `;
      fragment.appendChild(row);
    });

    tbody.innerHTML = "";
    tbody.appendChild(fragment);
    console.log("Rendered", this.filteredChannels.length, "channels");
  }

  async loadMoreChannels() {
    if (this.isLoading) return;

    const startIndex = this.renderedChannels;
    const endIndex = Math.min(
      startIndex + this.batchSize,
      this.filteredChannels.length,
    );

    if (startIndex >= this.filteredChannels.length) {
      return; // No more channels to load
    }

    this.isLoading = true;

    // Remove existing loading row if present
    const existingLoadingRow = document.getElementById("loadingMoreRow");
    if (existingLoadingRow) {
      existingLoadingRow.remove();
    }

    const channelsToRender = this.filteredChannels.slice(startIndex, endIndex);

    // Render channels in batch
    const fragment = document.createDocumentFragment();
    const tbody = document.getElementById("channelsTableBody");

    for (const channel of channelsToRender) {
      const row = document.createElement("tr");
      row.innerHTML = `
        <td>
          <strong>${this.escapeHtml(channel.channel_name)}</strong>
          ${
            channel.tvg_name && channel.tvg_name !== channel.channel_name
              ? `<br><small class="text-muted">TVG: ${this.escapeHtml(channel.tvg_name)}</small>`
              : ""
          }
        </td>
        <td>${channel.group_title ? this.escapeHtml(channel.group_title) : '<span class="text-muted">-</span>'}</td>
        <td>${channel.tvg_id ? this.escapeHtml(channel.tvg_id) : '<span class="text-muted">-</span>'}</td>
      `;
      fragment.appendChild(row);
    }

    tbody.appendChild(fragment);
    this.renderedChannels = endIndex;

    // Add loading row if there are more channels
    if (endIndex < this.filteredChannels.length) {
      const loadingRow = document.createElement("tr");
      loadingRow.id = "loadingMoreRow";
      loadingRow.innerHTML = `
        <td colspan="4" class="text-center text-muted">
          <span class="loading"></span> Loading more channels...
        </td>
      `;
      tbody.appendChild(loadingRow);
    }

    this.isLoading = false;
  }

  filterChannels() {
    const filterInput = document.getElementById("channelsFilter");
    const searchTerm = filterInput.value.toLowerCase().trim();

    console.log("Filter called with:", searchTerm); // Debug

    if (searchTerm === this.currentFilter) {
      return; // No change in filter
    }

    this.currentFilter = searchTerm;

    if (!searchTerm) {
      this.filteredChannels = [...this.currentChannels];
      console.log(
        "Filter cleared, showing all channels:",
        this.filteredChannels.length,
      );
    } else {
      // Split search terms for multi-word search
      const searchTerms = searchTerm
        .split(/\s+/)
        .filter((term) => term.length > 0);
      console.log("Filtering with terms:", searchTerms);

      this.filteredChannels = this.currentChannels.filter((channel) => {
        const searchableText = [
          channel.channel_name || "",
          channel.tvg_name || "",
          channel.group_title || "",
          channel.tvg_id || "",
        ]
          .join(" ")
          .toLowerCase();

        // All search terms must match (AND logic)
        const matches = searchTerms.every((term) => {
          const result = searchableText.includes(term); // Simple contains match for now
          return result;
        });

        // Debug first few matches
        if (this.filteredChannels.length < 5) {
          console.log(
            "Channel:",
            channel.channel_name,
            "Searchable:",
            searchableText.substring(0, 50),
            "Matches:",
            matches,
          );
        }

        return matches;
      });
      console.log(
        "Filtered channels:",
        this.filteredChannels.length,
        "out of",
        this.currentChannels.length,
      );
    }

    this.renderAllChannels();
    this.updateChannelCount();
  }

  fuzzyMatch(text, term) {
    // Simple fuzzy matching: exact match or contains
    if (text.includes(term)) {
      return true;
    }

    // Allow for some typos by checking if term is within edit distance
    const words = text.split(/\s+/);
    return words.some((word) => {
      if (word.includes(term) || term.includes(word)) {
        return true;
      }
      // Simple edit distance check for short terms
      if (term.length >= 3 && word.length >= 3) {
        return (
          this.levenshteinDistance(word, term) <= Math.floor(term.length / 3)
        );
      }
      return false;
    });
  }

  levenshteinDistance(str1, str2) {
    const matrix = [];
    for (let i = 0; i <= str2.length; i++) {
      matrix[i] = [i];
    }
    for (let j = 0; j <= str1.length; j++) {
      matrix[0][j] = j;
    }
    for (let i = 1; i <= str2.length; i++) {
      for (let j = 1; j <= str1.length; j++) {
        if (str2.charAt(i - 1) === str1.charAt(j - 1)) {
          matrix[i][j] = matrix[i - 1][j - 1];
        } else {
          matrix[i][j] = Math.min(
            matrix[i - 1][j - 1] + 1,
            matrix[i][j - 1] + 1,
            matrix[i - 1][j] + 1,
          );
        }
      }
    }
    return matrix[str2.length][str1.length];
  }

  updateChannelCount() {
    const countElement = document.getElementById("channelsCount");
    const total = this.filteredChannels.length;
    const originalTotal = this.currentChannels.length;

    if (this.currentFilter) {
      countElement.textContent = `${total.toLocaleString()} of ${originalTotal.toLocaleString()} channels`;
    } else {
      countElement.textContent = `${total.toLocaleString()} ${total === 1 ? "channel" : "channels"}`;
    }
  }

  hideModal() {
    SharedUtils.hideStandardModal("channelsModal");

    // Clean up
    if (this.scrollContainer) {
      this.scrollContainer.removeEventListener(
        "scroll",
        this.handleScroll.bind(this),
      );
      this.scrollContainer = null;
    }

    // Reset state
    this.currentChannels = [];
    this.filteredChannels = [];
    this.renderedChannels = 0;
    this.currentFilter = "";
    this.currentSourceId = null;
    this.isLoading = false;

    // Call close callback if provided
    if (this.closeCallback) {
      this.closeCallback();
      this.closeCallback = null;
    }
  }

  setCloseCallback(callback) {
    this.closeCallback = callback;
  }

  escapeHtml(text) {
    return SharedUtils.escapeHtml(text);
  }
}

// Create global instance
const channelsViewer = new ChannelsViewer();
window.channelsViewer = channelsViewer;

// Initialize when DOM is loaded
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", () => {
    channelsViewer.init();
    SharedUtils.setupStandardModalCloseHandlers("channelsModal");
  });
} else {
  channelsViewer.init();
  SharedUtils.setupStandardModalCloseHandlers("channelsModal");
}

// Global function for onclick handlers
function hideChannelsModal() {
  channelsViewer.hideModal();
}
