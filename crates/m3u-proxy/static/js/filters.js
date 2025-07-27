// Filters Management JavaScript
class FiltersManager {
  constructor() {
    this.filters = [];
    this.sources = [];
    this.isEditing = false;
    this.currentFilter = null;
    this.availableFields = [];
    this.validationTimeout = null;
    this.lastValidationPattern = "";
  }

  async init() {
    await this.loadFilterFields();
    await this.loadSources();
    await this.loadFilters();
    this.setupEventListeners();
    this.initializeFilterPreview();
  }

  setupEventListeners() {
    // Add filter button
    document.getElementById("addFilterBtn").addEventListener("click", () => {
      this.showFilterModal();
    });

    // Modal close buttons
    document.getElementById("cancelFilter").addEventListener("click", () => {
      this.hideFilterModal();
    });

    // Optional close button (may not exist in current implementation)
    const closeButton = document.getElementById("closeFilterModal");
    if (closeButton) {
      closeButton.addEventListener("click", () => {
        this.hideFilterModal();
      });
    }

    // Save filter button
    document.getElementById("saveFilter").addEventListener("click", () => {
      this.saveFilter();
    });

    // Test pattern button
    document.getElementById("testPatternBtn").addEventListener("click", () => {
      this.testPattern();
    });

    // Test source change
    document.getElementById("testSource").addEventListener("change", (e) => {
      this.updateTestButton();
      this.clearTestResults(); // Clear test results when source changes
      // Re-validate with new source
      if (document.getElementById("filterPattern").value.trim()) {
        this.debouncedValidatePattern();
      }
    });

    // Pattern textarea change
    document.getElementById("filterPattern").addEventListener("input", () => {
      this.updateTestButton();
      this.updateFilterPreview();
      this.debouncedValidatePattern();
      this.clearTestResults(); // Clear test results when pattern changes
    });

    // Also handle paste events in pattern textarea
    document.getElementById("filterPattern").addEventListener("paste", () => {
      setTimeout(() => {
        this.updateTestButton();
        this.updateFilterPreview();
        this.debouncedValidatePattern();
        this.clearTestResults(); // Clear test results when pattern changes
      }, 10);
    });

    // Modal click outside behavior removed to prevent accidental closing

    // Examples modal close
    document.getElementById("examplesModal").addEventListener("click", (e) => {
      if (e.target === document.getElementById("examplesModal")) {
        this.hideExamplesModal();
      }
    });
  }

  async loadFilterFields() {
    try {
      const response = await fetch("/api/v1/filters/fields");
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
      this.availableFields = await response.json();
      console.log("Loaded available fields from API:", this.availableFields);
    } catch (error) {
      console.error("Failed to load filter fields:", error);
      // Fallback to basic fields if API fails
      this.availableFields = [
        {
          name: "channel_name",
          display_name: "Channel Name",
          field_type: "string",
          nullable: false,
        },
        {
          name: "group_title",
          display_name: "Group Title",
          field_type: "string",
          nullable: true,
        },
        {
          name: "tvg_id",
          display_name: "TVG ID",
          field_type: "string",
          nullable: true,
        },
        {
          name: "tvg_name",
          display_name: "TVG Name",
          field_type: "string",
          nullable: true,
        },
        {
          name: "stream_url",
          display_name: "Stream URL",
          field_type: "string",
          nullable: false,
        },
      ];
      console.log("Using fallback fields:", this.availableFields);
    }
  }

  async loadSources() {
    try {
      const response = await fetch("/api/v1/sources/stream");
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
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
      this.populateSourceSelect();
    } catch (error) {
      console.error("Failed to load stream sources:", error);
      this.showAlert("Failed to load stream sources", "danger");
    }
  }

  populateSourceSelect() {
    const select = document.getElementById("testSource");
    const label = document.querySelector('label[for="testSource"]');

    if (!select) {
      console.warn("testSource element not found");
      return;
    }

    // Clear existing options
    select.innerHTML = "";

    if (this.sources.length === 0) {
      select.innerHTML =
        '<option value="">No stream sources available</option>';
      select.disabled = true;
      return;
    }

    if (this.sources.length === 1) {
      // Only one source - hide the label and dropdown, auto-select it
      const sourceData = this.sources[0];
      const option = document.createElement("option");
      option.value = sourceData.id;
      option.textContent = `${sourceData.name} (${sourceData.channel_count} channels)`;
      option.selected = true;
      select.appendChild(option);

      // Hide just the label and dropdown, not the test button
      if (label) {
        label.style.display = "none";
      }
      select.style.display = "none";
    } else {
      // Multiple sources - show dropdown with default option
      select.innerHTML =
        '<option value="">Select a source to test the pattern...</option>';

      this.sources.forEach((sourceData) => {
        const option = document.createElement("option");
        option.value = sourceData.id;
        option.textContent = `${sourceData.name} (${sourceData.channel_count} channels)`;
        select.appendChild(option);
      });

      // Make sure the label and dropdown are visible
      if (label) {
        label.style.display = "block";
      }
      select.style.display = "block";
    }

    select.disabled = false;

    // Update test button state after populating
    this.updateTestButton();
  }

  async loadFilters() {
    try {
      const response = await fetch("/api/v1/filters?" + new Date().getTime());
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
      const data = await response.json();
      // Handle paginated API response format
      if (data.success && data.data && Array.isArray(data.data.items)) {
        this.filters = data.data.items;
      } else if (Array.isArray(data)) {
        this.filters = data;
      } else {
        this.filters = [];
      }
      this.renderFilters();
    } catch (error) {
      console.error("Failed to load filters:", error);
      this.showAlert("Failed to load filters", "danger");
    }
  }

  renderFilters() {
    const tbody = document.getElementById("filtersTableBody");
    if (!tbody) {
      console.error("filtersTableBody element not found");
      return;
    }

    if (this.filters.length === 0) {
      tbody.innerHTML = `
                <div class="no-filters-message">
                    No filters found. <a href="#" onclick="filtersManager.showFilterModal()">Create your first filter</a>
                </div>
            `;
      return;
    }

    // Sort filters alphabetically by name
    const sortedFilters = [...this.filters].sort((a, b) => {
      const nameA = (a.filter || a).name;
      const nameB = (b.filter || b).name;
      return nameA.localeCompare(nameB, undefined, {
        numeric: true,
        sensitivity: "base",
      });
    });

    tbody.innerHTML = sortedFilters
      .map((filterData, index) => {
        // Handle FilterWithUsage structure - check if filter data is nested
        const filter = filterData.filter || filterData;
        const usageCount = filterData.usage_count || 0;

        // Generate filter preview from condition_tree or conditions
        let filterPreview = "No conditions defined";
        try {
          if (filter.condition_tree && filter.condition_tree.trim() !== "") {
            const tree = JSON.parse(filter.condition_tree);
            filterPreview = this.convertTreeToPattern(tree);
          } else if (filter.conditions && filter.conditions.length > 0) {
            filterPreview = this.generateFilterPreview(filter);
          } else {
            // Handle legacy filters with missing condition data
            filterPreview = "⚠️ Legacy filter - click Edit to reconfigure";
          }
        } catch (e) {
          console.warn("Failed to parse filter for preview:", e);
          filterPreview = "⚠️ Invalid filter pattern - click Edit to fix";
        }

        return `
                <div class="filter-card">
                    <div class="filter-header">
                        <div class="filter-info">
                            <div class="filter-name">
                                <strong>${this.escapeHtml(filter.name)}</strong>
                            </div>
                            <div class="filter-meta">
                                <span class="badge ${filter.is_inverse ? "badge-danger" : "badge-success"}">
                                    ${filter.is_inverse ? "Exclude" : "Include"}
                                </span>
                                ${filter.is_system_default ? '<span class="badge badge-info">System Default</span>' : ""}
                                <span class="badge ${usageCount > 0 ? "badge-primary" : "badge-secondary"}">
                                    ${usageCount} ${usageCount === 1 ? "proxy" : "proxies"}
                                </span>
                            </div>
                        </div>
                        <div class="filter-actions">
                            ${this.renderActionsCell(filter, usageCount)}
                        </div>
                    </div>
                    <div class="rule-expression">
                        <strong>Expression:</strong>
                        <pre><code>${this.escapeHtml(filterPreview)}</code></pre>
                        ${this.generateFilterExpressionTree(filterData)}
                    </div>
                </div>
            `;
      })
      .join("");
  }

  renderActionsCell(filter, usageCount) {
    const canDelete = usageCount === 0 && !filter.is_system_default;
    const deleteTooltip = filter.is_system_default
      ? "Cannot delete: system default filter"
      : usageCount > 0
        ? `Cannot delete: filter is in use by ${usageCount} proxy/proxies`
        : "";

    return `
            <div class="filter-action-buttons">
                <button class="btn btn-primary btn-sm btn-edit" onclick="filtersManager.editFilter('${filter.id}')">
                    Edit
                </button>
                <button class="btn btn-success btn-sm btn-duplicate" onclick="filtersManager.duplicateFilter('${filter.id}')">
                    Duplicate
                </button>
                ${
                  !filter.is_system_default
                    ? `
                <button class="btn btn-danger btn-sm btn-delete" onclick="filtersManager.deleteFilter('${filter.id}')" ${deleteTooltip ? `title="${deleteTooltip}"` : ""} ${!canDelete ? "disabled" : ""}>
                    Delete
                </button>
                `
                    : ""
                }
            </div>
        `;
  }

  showFilterModal(filter = null) {
    this.isEditing = !!filter;
    this.currentFilter = filter;

    const modal = document.getElementById("filterModal");
    const title = document.getElementById("modalTitle");
    const form = document.getElementById("filterForm");

    title.textContent = filter ? "Edit Filter" : "Add Filter";

    // Reset form
    form.reset();

    // Clear any previous test results
    this.clearTestResults();

    // Using pattern-based filter editing

    if (filter) {
      document.getElementById("filterName").value = filter.name;
      document.getElementById("isInverse").checked = filter.is_inverse;

      // Populate the pattern field with text representation from filter data
      this.populatePatternFromFilter(filter);

      // Show a warning for legacy filters
      if (
        (!filter.condition_tree || filter.condition_tree.trim() === "") &&
        (!filter.conditions || filter.conditions.length === 0)
      ) {
        setTimeout(() => {
          this.showAlert(
            "⚠️ Legacy Filter Detected: This filter was created in an older version and has no pattern data. Please define a new filter pattern below and save to restore functionality.",
            "warning",
          );
        }, 500);
      }
    } else {
      document.getElementById("startingChannelNumber").value = 1;
      // Clear the pattern field and ensure placeholder is visible
      document.getElementById("filterPattern").value = "";
    }

    // Update UI after everything is set up
    setTimeout(() => {
      this.updateFilterPreview();
      this.updateTestButton();
    }, 0);

    modal.classList.add("show");
    document.getElementById("filterName").focus();
  }

  hideFilterModal() {
    const modal = document.getElementById("filterModal");
    modal.classList.remove("show");
    this.currentFilter = null;
    this.isEditing = false;
  }

  async saveFilter() {
    if (!this.validateForm()) {
      return;
    }

    const formData = this.getFormData();
    const saveBtn = document.getElementById("saveFilter");
    const originalText = saveBtn.textContent;

    this.setModalLoading(saveBtn, "Saving...");

    try {
      const url = this.isEditing
        ? `/api/v1/filters/${this.currentFilter.id}`
        : "/api/v1/filters";
      const method = this.isEditing ? "PUT" : "POST";

      const response = await fetch(url, {
        method,
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify(formData),
      });

      if (!response.ok) {
        const error = await response.text();
        throw new Error(error || `HTTP error! status: ${response.status}`);
      }

      const filter = await response.json();
      this.showAlert(
        `Filter "${filter.name}" ${this.isEditing ? "updated" : "created"} successfully`,
        "success",
      );

      this.hideFilterModal();
      await this.loadFilters();
    } catch (error) {
      console.error("Failed to save filter:", error);
      this.showAlert(
        `Failed to ${this.isEditing ? "update" : "create"} filter: ${error.message}`,
        "danger",
      );
    } finally {
      this.setModalLoading(saveBtn, originalText);
    }
  }

  getFormData() {
    return {
      name: document.getElementById("filterName").value.trim(),
      source_type: "stream", // Filters only apply to stream sources
      is_inverse: document.getElementById("isInverse").checked,
      filter_expression: document.getElementById("filterPattern").value.trim(),
    };
  }

  validateForm() {
    const name = document.getElementById("filterName").value.trim();
    const startingNumber = parseInt(
      document.getElementById("startingChannelNumber").value,
    );

    if (!name) {
      this.showAlert("Filter name is required", "danger");
      document.getElementById("filterName").focus();
      return false;
    }

    if (isNaN(startingNumber) || startingNumber < 1) {
      this.showAlert("Starting channel number must be at least 1", "danger");
      document.getElementById("startingChannelNumber").focus();
      return false;
    }

    // Validate filter pattern
    const pattern = document.getElementById("filterPattern").value.trim();
    if (!pattern) {
      this.showAlert("Filter pattern is required", "danger");
      document.getElementById("filterPattern").focus();
      return false;
    }

    // Pattern validation will be done by the backend
    // Frontend just ensures it's not empty

    return true;
  }

  async editFilter(filterId) {
    const filterData = this.filters.find(
      (f) => (f.filter || f).id === filterId,
    );
    if (filterData) {
      // Handle FilterWithUsage structure - extract filter if nested
      const filter = filterData.filter || filterData;
      this.showFilterModal(filter);
    }
  }

  async duplicateFilter(filterId) {
    const filterData = this.filters.find(
      (f) => (f.filter || f).id === filterId,
    );
    if (filterData) {
      // Handle FilterWithUsage structure - extract filter if nested
      const filter = filterData.filter || filterData;
      const duplicateFilter = {
        ...filter,
        name: `${filter.name} (Copy)`,
        id: null,
      };
      this.showFilterModal(duplicateFilter);
    }
  }

  async deleteFilter(filterId) {
    const filterData = this.filters.find(
      (f) => (f.filter || f).id === filterId,
    );
    if (!filterData) return;

    // Handle FilterWithUsage structure - extract filter and usage count
    const filter = filterData.filter || filterData;
    const usageCount = filterData.usage_count || 0;

    if (usageCount > 0) {
      this.showAlert(
        `Cannot delete filter "${filter.name}" as it is being used by ${usageCount} ${usageCount === 1 ? "proxy" : "proxies"}`,
        "danger",
      );
      return;
    }

    if (
      !confirm(
        `Are you sure you want to delete the filter "${filter.name}"? This action cannot be undone.`,
      )
    ) {
      return;
    }

    try {
      const response = await fetch(`/api/v1/filters/${filter.id}`, {
        method: "DELETE",
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      this.showAlert(`Filter "${filter.name}" deleted successfully`, "success");
      await this.loadFilters();
    } catch (error) {
      console.error("Failed to delete filter:", error);
      this.showAlert(`Failed to delete filter: ${error.message}`, "danger");
    }
  }

  async testPattern() {
    const sourceId = document.getElementById("testSource").value;
    const isInverse = document.getElementById("isInverse").checked;

    const pattern = document.getElementById("filterPattern").value.trim();
    if (!sourceId || !pattern) {
      this.showAlert(
        "Please select a source and enter a pattern to test",
        "warning",
      );
      return;
    }

    // Send raw pattern directly to backend - it will parse everything

    const testData = {
      source_id: sourceId,
      source_type: "stream",
      filter_expression: pattern,
      is_inverse: isInverse,
    };

    console.log("Test request data:", JSON.stringify(testData, null, 2));

    const testBtn = document.getElementById("testPatternBtn");
    const originalText = testBtn.textContent;
    testBtn.textContent = "Testing...";
    testBtn.disabled = true;

    try {
      const response = await fetch("/api/v1/filters/test", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify(testData),
      });

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      const result = await response.json();
      this.displayTestResults(result);
    } catch (error) {
      console.error("Failed to test pattern:", error);
      this.showAlert(`Failed to test pattern: ${error.message}`, "danger");
    } finally {
      testBtn.textContent = originalText;
      testBtn.disabled = false;
    }
  }

  displayTestResults(result) {
    const testResultsContainer = document.getElementById("testResults");
    const testResultsContent = document.getElementById("testResultsContent");

    if (!result.is_valid) {
      // Show error in the test results container
      testResultsContent.innerHTML = `
        <div class="alert alert-danger">
          <strong>Test Failed:</strong> ${this.escapeHtml(result.error || "Filter test failed")}
        </div>
      `;
      testResultsContainer.style.display = "block";
      return;
    }

    // Get source name for display
    const sourceSelect = document.getElementById("testSource");
    const sourceName =
      sourceSelect.options[sourceSelect.selectedIndex]?.text ||
      "Unknown Source";

    const percentage =
      result.total_channels > 0
        ? Math.round((result.matched_count / result.total_channels) * 100)
        : 0;

    // Build the results HTML
    let resultsHtml = `
      <div class="alert alert-success">
        <strong>Test Completed:</strong> ${result.matched_count} of ${result.total_channels} channels matched (${percentage}%) from ${this.escapeHtml(sourceName)}
      </div>
    `;

    // Add expression tree if available
    if (result.expression_tree) {
      resultsHtml += this.generateFilterExpressionTreeHtml(
        result.expression_tree,
      );
    }

    if (result.matching_channels && result.matching_channels.length > 0) {
      resultsHtml += `
        <div class="test-results-channels">
          <h6>Matching Channels (showing first ${Math.min(result.matching_channels.length, 20)}):</h6>
          <div class="channels-list">
      `;

      // Show up to 20 channels to avoid overwhelming the modal
      const channelsToShow = result.matching_channels.slice(0, 20);
      for (const channel of channelsToShow) {
        resultsHtml += `
          <div class="channel-item">
            <div class="channel-name">${this.escapeHtml(channel.channel_name)}</div>
            ${channel.group_title ? `<div class="channel-group text-muted">${this.escapeHtml(channel.group_title)}</div>` : ""}
          </div>
        `;
      }

      resultsHtml += `</div>`;

      if (result.matching_channels.length > 20) {
        resultsHtml += `<div class="text-muted mt-2">... and ${result.matching_channels.length - 20} more channels</div>`;
      }

      resultsHtml += `</div>`;
    } else {
      resultsHtml += `
        <div class="alert alert-info">
          No channels matched the filter criteria.
        </div>
      `;
    }

    testResultsContent.innerHTML = resultsHtml;
    testResultsContainer.style.display = "block";
  }

  clearTestResults() {
    const testResultsContainer = document.getElementById("testResults");
    if (testResultsContainer) {
      testResultsContainer.style.display = "none";
    }
  }

  showExamplesModal() {
    document.getElementById("examplesModal").classList.add("show");
  }

  hideExamplesModal() {
    document.getElementById("examplesModal").classList.remove("show");
  }

  showAlert(message, type = "info") {
    const alertsContainer = document.getElementById("alertsContainer");
    const alertId = "alert-" + Date.now();

    const alertHtml = `
            <div id="${alertId}" class="alert alert-${type}" role="alert">
                ${this.escapeHtml(message)}
                <button type="button" class="modal-close" onclick="document.getElementById('${alertId}').remove()" style="float: right; background: none; border: none; font-size: 1.2em; cursor: pointer;">&times;</button>
            </div>
        `;

    alertsContainer.insertAdjacentHTML("beforeend", alertHtml);

    // Auto-remove success alerts after 5 seconds
    if (type === "success") {
      setTimeout(() => {
        const alert = document.getElementById(alertId);
        if (alert) alert.remove();
      }, 5000);
    }
  }

  showLoading() {
    const loadingIndicator = document.getElementById("loadingIndicator");
    const filtersContainer = document.getElementById("filtersTableBody");
    if (loadingIndicator) {
      loadingIndicator.style.display = "block";
    }
    if (filtersContainer) {
      filtersContainer.style.opacity = "0.5";
    }
  }

  hideLoading() {
    const loadingIndicator = document.getElementById("loadingIndicator");
    const filtersContainer = document.getElementById("filtersTableBody");
    if (loadingIndicator) {
      loadingIndicator.style.display = "none";
    }
    if (filtersContainer) {
      filtersContainer.style.opacity = "1";
    }
  }

  setModalLoading(button, text) {
    if (text) {
      button.textContent = text;
      button.disabled = true;
    } else {
      button.disabled = false;
    }
  }

  generateFilterPreview(filterData) {
    const conditions = filterData.conditions || [];
    const operator = (filterData.logical_operator || "AND").toLowerCase();
    const operatorText = operator === "and" ? "AND" : "OR";

    if (conditions.length === 0) {
      return "No conditions";
    }

    // Detect if we're on a mobile device for responsive truncation
    const isMobile = window.innerWidth <= 480;
    const singleConditionMaxLength = isMobile ? 50 : 80;

    if (conditions.length === 1) {
      const condition = conditions[0];
      const field = this.availableFields.find(
        (f) => f.name === condition.field_name,
      );
      const fieldName = field ? field.name : condition.field_name;
      const operatorDisplay = this.formatOperatorDisplay(condition.operator);

      return `${fieldName} ${operatorDisplay} "${this.truncateTextSmart(condition.value, singleConditionMaxLength)}"`;
    }

    // For multiple conditions, show detailed list
    let preview = `${operatorText} of ${conditions.length} conditions:\n`;
    let currentLength = preview.length;
    // Adjust max length based on typical preview window size and device
    const maxLength = isMobile ? 500 : 800;
    const maxLineLength = isMobile ? 50 : 80; // Maximum characters per line for comfortable reading

    for (let i = 0; i < conditions.length; i++) {
      const condition = conditions[i];
      const field = this.availableFields.find(
        (f) => f.name === condition.field_name,
      );
      const fieldName = field ? field.name : condition.field_name;
      const operatorDisplay = this.formatOperatorDisplay(condition.operator);

      // Calculate dynamic truncation based on remaining space and line length
      const baseConditionLength = `- ${fieldName} ${operatorDisplay} ""`.length;
      const availableValueLength = Math.min(
        maxLineLength - baseConditionLength,
        Math.max(
          15, // Minimum value length
          (maxLength - currentLength) / (conditions.length - i) -
            baseConditionLength -
            10,
        ),
      );

      const value = this.truncateTextSmart(
        condition.value,
        availableValueLength,
      );
      const conditionText = `- ${fieldName} ${operatorDisplay} "${value}"`;

      // Check if adding this condition would exceed the limit
      if (currentLength + conditionText.length + 1 > maxLength && i > 0) {
        const remaining = conditions.length - i;
        preview += `... and ${remaining} more condition${remaining === 1 ? "" : "s"}`;
        break;
      }

      preview += conditionText;
      currentLength += conditionText.length + 1; // +1 for newline

      if (i < conditions.length - 1) {
        preview += "\n";
      }
    }

    return preview;
  }

  truncateText(text, maxLength) {
    if (!text || text.length <= maxLength) {
      return text || "";
    }
    return text.substring(0, maxLength - 3) + "...";
  }

  // Enhanced version that handles word boundaries better for preview display
  truncateTextSmart(text, maxLength) {
    if (!text || text.length <= maxLength) {
      return text || "";
    }

    // If we need to truncate, try to break at word boundaries
    const truncated = text.substring(0, maxLength - 3);
    const lastSpaceIndex = truncated.lastIndexOf(" ");

    // If there's a space reasonably close to the end, break there
    if (lastSpaceIndex > maxLength * 0.7) {
      return truncated.substring(0, lastSpaceIndex) + "...";
    }

    return truncated + "...";
  }

  formatOperatorDisplay(operator) {
    // Convert database format to modifier syntax for consistency
    let displayOperator = operator;

    // Handle not_ prefix
    if (operator.startsWith("not_")) {
      displayOperator = `not ${operator.substring(4)}`;
    }
    // Handle case_sensitive_ prefix
    else if (operator.startsWith("case_sensitive_")) {
      displayOperator = `case_sensitive ${operator.substring(15)}`;
    }
    // Handle not_case_sensitive_ prefix (both modifiers)
    else if (operator.startsWith("not_case_sensitive_")) {
      displayOperator = `not case_sensitive ${operator.substring(19)}`;
    }

    return displayOperator;
  }

  escapeHtml(text) {
    const div = document.createElement("div");
    div.textContent = text;
    return div.innerHTML;
  }

  // Initialize filter preview element with proper styling
  initializeFilterPreview() {
    const preview = document.getElementById("filterPreview");
    if (preview && !preview.classList.contains("filter-preview-text")) {
      preview.classList.add("filter-preview-text");
    }
  }

  // Pattern preview and validation methods

  updateFilterPreview() {
    const preview = document.getElementById("filterPreview");
    if (!preview) {
      console.warn("filterPreview element not found");
      return;
    }

    // Apply the correct CSS class for styling
    if (!preview.classList.contains("filter-preview-text")) {
      preview.classList.add("filter-preview-text");
    }

    const patternInput = document.getElementById("filterPattern");
    const pattern = patternInput ? patternInput.value.trim() : "";
    preview.textContent =
      pattern ||
      'Enter a filter pattern above (e.g., channel_name contains "sport")';
  }

  updateTestButton() {
    const testBtn = document.getElementById("testPatternBtn");
    const sourceSelect = document.getElementById("testSource");

    let hasValidFilter = false;

    const pattern = document.getElementById("filterPattern").value.trim();
    hasValidFilter = pattern.length > 0;

    testBtn.disabled = !hasValidFilter || !sourceSelect.value;
  }

  // ===== CLIENT-SIDE VALIDATION FOR REAL-TIME FEEDBACK =====

  /**
   * Debounced validation to avoid excessive API calls
   */
  debouncedValidatePattern() {
    clearTimeout(this.validationTimeout);
    this.validationTimeout = setTimeout(() => {
      this.validatePattern();
    }, 300); // 300ms delay for more responsive validation
  }

  /**
   * Validate the current filter pattern with server-side validation
   */
  async validatePattern() {
    const textarea = document.getElementById("filterPattern");
    const pattern = textarea.value.trim();

    if (!pattern) {
      this.clearValidationHighlights(textarea);
      this.updateValidationUI({
        valid: true,
        errors: [],
        serverValidation: null,
      });
      this.lastValidationPattern = "";
      return;
    }

    // Skip validation if pattern hasn't changed
    if (pattern === this.lastValidationPattern) {
      return;
    }
    this.lastValidationPattern = pattern;

    // Start with client-side validation for immediate feedback
    const clientValidation = this.validatePatternSyntax(pattern);
    this.updateValidationHighlights(textarea, clientValidation);

    // Skip server validation if client-side validation already found errors
    if (!clientValidation.valid) {
      this.updateValidationUI({
        ...clientValidation,
        serverValidation: null,
      });
      return;
    }

    // Show loading state for server validation
    this.updateValidationUI({
      ...clientValidation,
      serverValidation: { loading: true },
    });

    // Then do server-side validation
    try {
      const serverValidation = await this.validatePatternOnServer(pattern);

      // Combine client and server validation
      const combinedValidation = this.combineValidationResults(
        clientValidation,
        serverValidation,
      );
      this.updateValidationHighlights(textarea, combinedValidation);
      this.updateValidationUI(combinedValidation);
    } catch (error) {
      console.error("Server validation failed:", error);
      // Fall back to client-side validation only
      this.updateValidationUI({
        ...clientValidation,
        serverValidation: { error: error.message },
      });
    }
  }

  /**
   * Validate pattern on server using the test API
   */
  async validatePatternOnServer(pattern) {
    const sourceId = document.getElementById("testSource").value;

    if (!sourceId) {
      // Can't validate without a source, return neutral result
      return {
        valid: true,
        is_valid: true,
        error: null,
        syntax_valid: true,
        fields_valid: true,
        operators_valid: true,
      };
    }

    const testData = {
      source_id: sourceId,
      source_type: "stream",
      filter_expression: pattern,
      is_inverse: false,
    };

    const response = await fetch("/api/v1/filters/test", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(testData),
    });

    if (!response.ok) {
      const errorText = await response.text();
      throw new Error(`Server validation failed: ${errorText}`);
    }

    const result = await response.json();
    return result;
  }

  /**
   * Combine client-side and server-side validation results
   */
  combineValidationResults(clientValidation, serverValidation) {
    const combined = { ...clientValidation };

    // If server validation failed, add server errors
    if (!serverValidation.is_valid && serverValidation.error) {
      combined.valid = false;
      combined.serverValidation = {
        valid: false,
        error: serverValidation.error,
      };

      // Try to extract useful information from server error
      const errorLower = serverValidation.error.toLowerCase();
      if (errorLower.includes("syntax")) {
        combined.syntax.valid = false;
        combined.syntax.errors.push(`Server: ${serverValidation.error}`);
      } else if (
        errorLower.includes("field") ||
        errorLower.includes("unknown")
      ) {
        combined.fields.valid = false;
        combined.fields.errors.push(`Server: ${serverValidation.error}`);
      } else if (errorLower.includes("operator")) {
        combined.operators.valid = false;
        combined.operators.errors.push(`Server: ${serverValidation.error}`);
      } else {
        // General server error
        combined.syntax.valid = false;
        combined.syntax.errors.push(`Server: ${serverValidation.error}`);
      }
    } else {
      combined.serverValidation = {
        valid: true,
        matchedChannels: serverValidation.matched_count || 0,
        totalChannels: serverValidation.total_channels || 0,
      };
    }

    return combined;
  }

  /**
   * Perform comprehensive client-side validation
   */
  validatePatternSyntax(pattern) {
    const result = {
      valid: true,
      errors: [],
      warnings: [],
      syntax: { valid: true, errors: [] },
      fields: { valid: true, errors: [] },
      operators: { valid: true, errors: [] },
      values: { valid: true, errors: [] },
      highlights: [], // Array of {start, end, type, message}
    };

    try {
      // 1. Basic syntax validation
      this.validateBasicSyntax(pattern, result);

      // 2. Field validation
      this.validateFields(pattern, result);

      // 3. Operator validation
      this.validateOperators(pattern, result);

      // 4. Value validation (quotes, escaping)
      this.validateValues(pattern, result);

      // 5. Parentheses matching
      this.validateParentheses(pattern, result);

      // Determine overall validity
      result.valid =
        result.syntax.valid &&
        result.fields.valid &&
        result.operators.valid &&
        result.values.valid;
    } catch (error) {
      result.valid = false;
      result.syntax.valid = false;
      result.syntax.errors.push(`Parse error: ${error.message}`);
    }

    return result;
  }

  /**
   * Validate basic syntax structure
   */
  validateBasicSyntax(pattern, result) {
    // Check for empty conditions
    if (/\(\s*\)/.test(pattern)) {
      const match = pattern.match(/\(\s*\)/);
      result.syntax.valid = false;
      result.syntax.errors.push("Empty parentheses are not allowed");
      result.highlights.push({
        start: match.index,
        end: match.index + match[0].length,
        type: "error",
        message: "Empty parentheses",
      });
    }

    // Check for orphaned logical operators
    const orphanedOps = pattern.match(/\b(AND|OR)\s+(AND|OR)\b/gi);
    if (orphanedOps) {
      orphanedOps.forEach((match) => {
        const index = pattern.indexOf(match);
        result.syntax.valid = false;
        result.syntax.errors.push(`Consecutive logical operators: ${match}`);
        result.highlights.push({
          start: index,
          end: index + match.length,
          type: "error",
          message: "Consecutive logical operators",
        });
      });
    }

    // Check for unmatched quotes
    const quotes = pattern.match(/"/g);
    if (quotes && quotes.length % 2 !== 0) {
      const lastQuoteIndex = pattern.lastIndexOf('"');
      result.syntax.valid = false;
      result.syntax.errors.push("Unmatched quote - missing closing quote");
      result.highlights.push({
        start: lastQuoteIndex,
        end: lastQuoteIndex + 1,
        type: "error",
        message: "Unmatched quote",
      });
    }

    // Check for patterns that look like typos or common mistakes
    const typoPatterns = [
      { regex: /\b(contans|conatins|cotains)\b/gi, correct: "contains" },
      { regex: /\b(equels|eqals|euals)\b/gi, correct: "equals" },
      { regex: /\b(matchs|matche)\b/gi, correct: "matches" },
      { regex: /\b(starts_whith|start_with)\b/gi, correct: "starts_with" },
      { regex: /\b(ends_whith|end_with)\b/gi, correct: "ends_with" },
    ];

    typoPatterns.forEach(({ regex, correct }) => {
      let match;
      while ((match = regex.exec(pattern)) !== null) {
        // Only add typo error if this word hasn't already been flagged as an invalid operator
        const existingOperatorError = result.highlights.find(
          (h) =>
            h.start === match.index &&
            h.end === match.index + match[0].length &&
            h.message.includes("Invalid operator"),
        );

        if (!existingOperatorError) {
          result.syntax.valid = false;
          result.syntax.errors.push(
            `Possible typo: "${match[0]}" - did you mean "${correct}"?`,
          );
          result.highlights.push({
            start: match.index,
            end: match.index + match[0].length,
            type: "error",
            message: `Possible typo - did you mean "${correct}"?`,
          });
        }
      }
    });

    // Check for missing conditions around operators
    const invalidPatterns = [
      /^\s*(AND|OR)/i, // Starting with operator
      /(AND|OR)\s*$/i, // Ending with operator
    ];

    invalidPatterns.forEach((regex) => {
      const match = pattern.match(regex);
      if (match) {
        result.syntax.valid = false;
        result.syntax.errors.push(
          `Invalid operator placement: ${match[0].trim()}`,
        );
        result.highlights.push({
          start: match.index,
          end: match.index + match[0].length,
          type: "error",
          message: "Invalid operator placement",
        });
      }
    });
  }

  /**
   * Validate field names against available fields
   */
  validateFields(pattern, result) {
    if (this.availableFields.length === 0) return; // Skip if fields not loaded

    const validFieldNames = this.availableFields.map((f) => f.name);

    // Match field names in conditions: word followed by any operator-like word
    const fieldRegex = /\b(\w+)\s+(?:not\s+)?(?:case_sensitive\s+)?(\w+)/gi;
    let match;

    while ((match = fieldRegex.exec(pattern)) !== null) {
      const fieldName = match[1];
      const operator = match[2];

      // Skip logical operators
      if (
        ["and", "or"].includes(fieldName.toLowerCase()) ||
        ["and", "or"].includes(operator.toLowerCase())
      ) {
        continue;
      }

      if (!validFieldNames.includes(fieldName)) {
        result.fields.valid = false;
        result.fields.errors.push(`Unknown field: ${fieldName}`);

        // Find the best suggestion for the field name
        const suggestion = this.findClosestFieldName(
          fieldName,
          validFieldNames,
        );
        const suggestionText = suggestion
          ? ` Did you mean '${suggestion}'?`
          : "";

        result.highlights.push({
          start: match.index,
          end: match.index + fieldName.length,
          type: "error",
          message: `Unknown field '${fieldName}'.${suggestionText} Valid fields: ${validFieldNames.join(", ")}`,
        });
      }
    }
  }

  /**
   * Validate operators
   */
  validateOperators(pattern, result) {
    const validOperators = [
      "contains",
      "equals",
      "matches",
      "starts_with",
      "ends_with",
      "not_contains",
      "not_equals",
      "not_matches",
    ];

    // Match the complete field + operator + value pattern to validate operators
    const operatorRegex =
      /(\w+)\s+((?:not\s+)?(?:case_sensitive\s+)?(\w+))\s+["'][^"']*["']/gi;
    let match;

    while ((match = operatorRegex.exec(pattern)) !== null) {
      const fieldName = match[1];
      const fullOperator = match[2];
      const baseOperator = match[3].toLowerCase();

      // Skip if this looks like a logical operator context
      if (
        ["and", "or"].includes(fieldName.toLowerCase()) ||
        ["and", "or"].includes(baseOperator)
      ) {
        continue;
      }

      if (
        !validOperators.includes(baseOperator) &&
        !validOperators.includes(`not_${baseOperator}`)
      ) {
        result.operators.valid = false;
        result.operators.errors.push(`Invalid operator: ${baseOperator}`);

        // Find the position of just the base operator within the full operator
        const operatorStart = match.index + match[0].indexOf(fullOperator);
        const baseOperatorStart =
          operatorStart + fullOperator.indexOf(match[3]);

        result.highlights.push({
          start: baseOperatorStart,
          end: baseOperatorStart + match[3].length,
          type: "error",
          message: `Invalid operator '${baseOperator}'. Valid operators: ${validOperators.join(", ")}`,
        });
      }
    }

    // Check for missing quotes after operators
    const missingQuoteRegex =
      /\b(?:contains|equals|matches|starts_with|ends_with)\s+([^"'\s][^\s]*)/gi;
    let quoteMatch;

    while ((quoteMatch = missingQuoteRegex.exec(pattern)) !== null) {
      const valueStart =
        quoteMatch.index + quoteMatch[0].indexOf(quoteMatch[1]);
      result.operators.valid = false;
      result.operators.errors.push(
        `Missing quotes around value: ${quoteMatch[1]}`,
      );
      result.highlights.push({
        start: valueStart,
        end: valueStart + quoteMatch[1].length,
        type: "error",
        message: `Value should be quoted: "${quoteMatch[1]}"`,
      });
    }
  }

  /**
   * Validate quoted values
   */
  validateValues(pattern, result) {
    // Check for unmatched quotes
    const quotes = pattern.match(/["']/g);
    if (quotes && quotes.length % 2 !== 0) {
      result.values.valid = false;
      result.values.errors.push("Unmatched quotes in pattern");

      // Find the last quote position
      const lastQuotePos = pattern.lastIndexOf(quotes[quotes.length - 1]);
      result.highlights.push({
        start: lastQuotePos,
        end: lastQuotePos + 1,
        type: "error",
        message: "Unmatched quote",
      });
    }

    // Check for empty quoted values
    const emptyValues = pattern.match(/['"]\s*['"]/g);
    if (emptyValues) {
      emptyValues.forEach((match) => {
        const index = pattern.indexOf(match);
        result.values.valid = false;
        result.values.errors.push("Empty quoted value");
        result.highlights.push({
          start: index,
          end: index + match.length,
          type: "warning",
          message: "Empty value",
        });
      });
    }
  }

  /**
   * Validate parentheses matching
   */
  validateParentheses(pattern, result) {
    const stack = [];
    const positions = [];

    for (let i = 0; i < pattern.length; i++) {
      const char = pattern[i];
      if (char === "(") {
        stack.push(i);
        positions.push({ pos: i, type: "open" });
      } else if (char === ")") {
        if (stack.length === 0) {
          result.syntax.valid = false;
          result.syntax.errors.push("Unmatched closing parenthesis");
          result.highlights.push({
            start: i,
            end: i + 1,
            type: "error",
            message: "Unmatched closing parenthesis",
          });
        } else {
          stack.pop();
          positions.push({ pos: i, type: "close" });
        }
      }
    }

    // Check for unmatched opening parentheses
    if (stack.length > 0) {
      stack.forEach((pos) => {
        result.syntax.valid = false;
        result.syntax.errors.push("Unmatched opening parenthesis");
        result.highlights.push({
          start: pos,
          end: pos + 1,
          type: "error",
          message: "Unmatched opening parenthesis",
        });
      });
    }
  }

  /**
   * Apply visual highlights to the textarea
   */
  updateValidationHighlights(textarea, validation) {
    // Remove existing highlights
    this.clearValidationHighlights(textarea);

    // Show simple error messages below textarea instead of overlay
    if (validation.highlights.length > 0) {
      this.showValidationMessages(textarea, validation.highlights);
    }
  }

  /**
   * Clear validation highlights
   */
  clearValidationHighlights(textarea) {
    const messagesContainer = document.getElementById(
      "validation-messages-container",
    );
    if (messagesContainer) {
      messagesContainer.remove();
    }
  }

  /**
   * Show validation messages below textarea
   */
  showValidationMessages(textarea, highlights) {
    // Create messages container if it doesn't exist
    let container = document.getElementById("validation-messages-container");
    if (!container) {
      container = document.createElement("div");
      container.id = "validation-messages-container";
      container.className = "validation-messages-container mt-2";
      textarea.parentNode.insertBefore(container, textarea.nextSibling);
    }

    // Deduplicate highlights by text and message
    const uniqueHighlights = [];
    const seen = new Set();

    highlights.forEach((highlight) => {
      const text = textarea.value.slice(highlight.start, highlight.end);
      const key = `${text}:${highlight.message}`;
      if (!seen.has(key)) {
        seen.add(key);
        uniqueHighlights.push({ ...highlight, text });
      }
    });

    // Group highlights by type
    const errors = uniqueHighlights.filter((h) => h.type === "error");
    const warnings = uniqueHighlights.filter((h) => h.type === "warning");

    let html = "";

    if (errors.length > 0) {
      html += `<div class="validation-message-group">`;
      html += `<div class="validation-message-header text-danger">⚠ ${errors.length} Error${errors.length > 1 ? "s" : ""}:</div>`;
      errors.forEach((error) => {
        html += `<div class="validation-message-item">• "${this.escapeHtml(error.text)}" - ${this.escapeHtml(error.message)}</div>`;
      });
      html += `</div>`;
    }

    if (warnings.length > 0) {
      html += `<div class="validation-message-group">`;
      html += `<div class="validation-message-header text-warning">⚠ ${warnings.length} Warning${warnings.length > 1 ? "s" : ""}:</div>`;
      warnings.forEach((warning) => {
        html += `<div class="validation-message-item">• "${this.escapeHtml(warning.text)}" - ${this.escapeHtml(warning.message)}</div>`;
      });
      html += `</div>`;
    }

    container.innerHTML = html;
  }

  /**
   * Find the closest field name using simple string similarity
   */
  findClosestFieldName(input, validFields) {
    let bestMatch = null;
    let bestScore = 0;

    for (const field of validFields) {
      const score = this.calculateSimilarity(
        input.toLowerCase(),
        field.toLowerCase(),
      );
      if (score > bestScore && score > 0.5) {
        // Require at least 50% similarity
        bestScore = score;
        bestMatch = field;
      }
    }

    return bestMatch;
  }

  /**
   * Calculate simple string similarity score
   */
  calculateSimilarity(str1, str2) {
    if (str1 === str2) return 1;

    const longer = str1.length > str2.length ? str1 : str2;
    const shorter = str1.length > str2.length ? str2 : str1;

    if (longer.length === 0) return 1;

    // Count matching characters
    let matches = 0;
    for (let i = 0; i < shorter.length; i++) {
      if (longer.includes(shorter[i])) {
        matches++;
      }
    }

    return matches / longer.length;
  }

  /**
   * Update validation UI indicators
   */
  updateValidationUI(validation) {
    // Update validation icons
    this.updateValidationIcon("syntax", validation.syntax);
    this.updateValidationIcon("fields", validation.fields);
    this.updateValidationIcon("operators", validation.operators);
    this.updateValidationIcon("values", validation.values);

    // Update server validation status
    this.updateServerValidationStatus(validation.serverValidation);

    // Update overall status
    const textarea = document.getElementById("filterPattern");
    if (validation.valid) {
      textarea.classList.remove("is-invalid");
      textarea.classList.add("is-valid");
    } else {
      textarea.classList.remove("is-valid");
      textarea.classList.add("is-invalid");
    }

    // Update test button state
    this.updateTestButton();
  }

  /**
   * Update server validation status indicator
   */
  updateServerValidationStatus(serverValidation) {
    let statusElement = document.getElementById("server-validation-status");

    if (!statusElement) {
      // Create server validation status element
      let container = document.getElementById("validation-icons");
      if (!container) {
        container = document.createElement("div");
        container.id = "validation-icons";
        container.className = "validation-icons mt-2";
        const textarea = document.getElementById("filterPattern");
        textarea.parentNode.appendChild(container);
      }

      statusElement = document.createElement("div");
      statusElement.id = "server-validation-status";
      statusElement.className = "server-validation-status mt-1";
      statusElement.style.minHeight = "20px";
      container.appendChild(statusElement);
    }

    if (!serverValidation) {
      statusElement.innerHTML = "";
      return;
    }

    if (serverValidation.loading) {
      statusElement.innerHTML = `
        <div class="d-flex align-items-center text-muted">
          <div class="spinner-border spinner-border-sm" role="status" style="width: 10px; height: 10px; border-width: 1px;">
            <span class="visually-hidden">Loading...</span>
          </div>
        </div>
      `;
    } else if (serverValidation.error) {
      statusElement.innerHTML = `
        <div class="text-warning" style="font-size: 0.7rem; opacity: 0.8;">
          ⚠ ${this.escapeHtml(serverValidation.error)}
        </div>
      `;
    } else if (serverValidation.valid) {
      const matchText =
        serverValidation.matchedChannels !== undefined
          ? ` ${serverValidation.matchedChannels}/${serverValidation.totalChannels} matches`
          : "";
      statusElement.innerHTML = `
        <div class="text-success" style="font-size: 0.7rem; opacity: 0.8;">
          ✓${matchText}
        </div>
      `;
    }
  }

  /**
   * Update individual validation icon
   */
  updateValidationIcon(type, validationResult) {
    const iconId = `validation-${type}`;
    let icon = document.getElementById(iconId);

    if (!icon) {
      // Create validation icons container if it doesn't exist
      let container = document.getElementById("validation-icons");
      if (!container) {
        container = document.createElement("div");
        container.id = "validation-icons";
        container.className = "validation-icons mt-2";
        const textarea = document.getElementById("filterPattern");
        textarea.parentNode.appendChild(container);
      }

      icon = document.createElement("span");
      icon.id = iconId;
      icon.className = "validation-icon badge me-2";
      container.appendChild(icon);
    }

    // Update icon appearance and content
    icon.className = "validation-icon badge me-2";
    const label = type.charAt(0).toUpperCase() + type.slice(1);

    if (validationResult.valid) {
      icon.classList.add("bg-success");
      icon.textContent = `✓ ${label}`;
      icon.title = `${label}: Valid`;
    } else {
      icon.classList.add("bg-danger");
      icon.textContent = `✗ ${label}`;
      icon.title = `${label}: ${validationResult.errors.join(", ")}`;
    }

    // Add detailed tooltips for some validators
    if (type === "fields" && this.availableFields.length > 0) {
      const validFields = this.availableFields.map(
        (f) => `${f.name} (${f.display_name})`,
      );
      icon.title += `\n\nValid Fields:\n${validFields.join("\n")}`;
    } else if (type === "operators") {
      const validOperators = [
        "contains",
        "equals",
        "matches",
        "starts_with",
        "ends_with",
      ];
      icon.title += `\n\nValid Operators:\n${validOperators.join("\n")}\nAdd "not " prefix for negation`;
    }
  }

  // Pattern population methods

  // Populate pattern from conditions array or condition_tree
  populatePatternFromFilter(filter) {
    let textPattern = "";

    // Try to parse condition_tree first (current format)
    if (filter.condition_tree && filter.condition_tree.trim() !== "") {
      try {
        const tree = JSON.parse(filter.condition_tree);
        textPattern = this.convertTreeToPattern(tree);
      } catch (e) {
        console.error("Failed to parse condition_tree:", e);
        textPattern = "// Invalid filter data - please reconfigure";
      }
    }
    // Fall back to conditions array (legacy format)
    else if (filter.conditions && filter.conditions.length > 0) {
      const operator =
        filter.logical_operator === "any" ||
        (filter.logical_operator &&
          filter.logical_operator.toLowerCase() === "or")
          ? " OR "
          : " AND ";
      textPattern = filter.conditions
        .map((condition) => {
          const field = this.availableFields.find(
            (f) => f.name === condition.field_name,
          );
          const fieldName = field ? field.name : condition.field_name;
          // Convert database format to modifier syntax for input
          let displayOperator = condition.operator;

          // Handle not_ prefix
          if (condition.operator.startsWith("not_")) {
            displayOperator = `not ${condition.operator.substring(4)}`;
          }
          // Handle case_sensitive_ prefix
          else if (condition.operator.startsWith("case_sensitive_")) {
            displayOperator = `case_sensitive ${condition.operator.substring(15)}`;
          }
          // Handle not_case_sensitive_ prefix (both modifiers)
          else if (condition.operator.startsWith("not_case_sensitive_")) {
            displayOperator = `not case_sensitive ${condition.operator.substring(19)}`;
          }

          return `${fieldName} ${displayOperator} "${condition.value}"`;
        })
        .join(operator);
    }
    // Handle legacy filters with missing condition data
    else {
      textPattern =
        '// Legacy filter detected - please define a new pattern below\n// Examples:\n//   channel_name contains "sport"\n//   group_title not_contains "adult"\n//   tvg_id matches "^BBC.*"';
      console.warn("Legacy filter found with no condition data:", filter.name);
    }

    document.getElementById("filterPattern").value = textPattern;
    this.debouncedValidatePattern();
  }

  // Convert condition tree to natural language pattern
  convertTreeToPattern(tree) {
    if (!tree) return "";

    // Handle empty or invalid tree structures
    if (typeof tree !== "object") {
      console.warn("Invalid tree structure - not an object:", tree);
      return "";
    }

    // Handle tree structure with root property
    if (tree.root && typeof tree.root === "object") {
      return this.convertTreeToPattern(tree.root);
    }

    if (tree.type === "condition") {
      const fieldName = tree.field || "field";
      let operator = tree.operator || "equals";
      const value = tree.value || "";

      // Convert database format to modifier syntax for consistency
      let displayOperator = operator;

      // Handle not_ prefix
      if (operator.startsWith("not_")) {
        displayOperator = `not ${operator.substring(4)}`;
      }
      // Handle case_sensitive_ prefix
      else if (operator.startsWith("case_sensitive_")) {
        displayOperator = `case_sensitive ${operator.substring(15)}`;
      }
      // Handle not_case_sensitive_ prefix (both modifiers)
      else if (operator.startsWith("not_case_sensitive_")) {
        displayOperator = `not case_sensitive ${operator.substring(19)}`;
      }

      return `${fieldName} ${displayOperator} "${value}"`;
    }

    if (
      tree.type === "group" &&
      tree.children &&
      Array.isArray(tree.children)
    ) {
      const logicalOp =
        tree.operator && tree.operator.toLowerCase() === "or"
          ? " OR "
          : " AND ";
      const childPatterns = tree.children
        .map((child) => this.convertTreeToPattern(child))
        .filter((pattern) => pattern.length > 0);

      if (childPatterns.length === 0) return "";
      if (childPatterns.length === 1) return childPatterns[0];

      // Wrap in parentheses for grouping clarity
      return `(${childPatterns.join(logicalOp)})`;
    }

    // Handle unknown tree structure
    console.warn("Unknown tree structure:", tree);
    return "";
  }

  generateFilterExpressionTreeHtml(expression_tree) {
    if (!expression_tree) return "";

    try {
      return `
        <div class="expression-tree-container mt-3">
          <div class="expression-tree-header">
            <h6>🌳 Logical Structure</h6>
          </div>
          <div class="expression-tree">
            ${this.renderFilterTreeNode(expression_tree, 0)}
          </div>
        </div>
      `;
    } catch (error) {
      console.warn("Failed to render filter expression tree:", error);
      return "";
    }
  }

  renderFilterTreeNode(node, depth = 0) {
    if (!node) return "";

    const indent = "  ".repeat(depth);

    if (node.type === "group") {
      let html = `<div class="tree-node tree-operator">${indent}${node.operator.toUpperCase()}</div>`;

      if (node.children && node.children.length > 0) {
        node.children.forEach((child, index) => {
          const isLast = index === node.children.length - 1;
          const connector = isLast ? "└── " : "├── ";

          if (child.type === "group") {
            html += `<div class="tree-node tree-operator">${indent}${connector}${child.operator.toUpperCase()}</div>`;
            if (child.children && child.children.length > 0) {
              child.children.forEach((grandchild, grandIndex) => {
                const isLastGrand = grandIndex === child.children.length - 1;
                const grandConnector = isLastGrand ? "└── " : "├── ";
                const grandIndent = "  ".repeat(depth + 1);

                if (grandchild.type === "group") {
                  html += this.renderFilterTreeNode(grandchild, depth + 2);
                } else {
                  const negatePrefix = grandchild.negate ? "NOT " : "";
                  const caseDisplay = grandchild.case_sensitive
                    ? " (case-sensitive)"
                    : " (case-insensitive)";
                  html += `<div class="tree-node tree-condition">${grandIndent}${grandConnector}(${this.escapeHtml(grandchild.field)} ${negatePrefix}${grandchild.operator} "${this.escapeHtml(grandchild.value)}"${caseDisplay})</div>`;
                }
              });
            }
          } else {
            const negatePrefix = child.negate ? "NOT " : "";
            const caseDisplay = child.case_sensitive
              ? " (case-sensitive)"
              : " (case-insensitive)";
            html += `<div class="tree-node tree-condition">${indent}${connector}(${this.escapeHtml(child.field)} ${negatePrefix}${child.operator} "${this.escapeHtml(child.value)}"${caseDisplay})</div>`;
          }
        });
      }

      return html;
    } else if (node.type === "condition") {
      const negatePrefix = node.negate ? "NOT " : "";
      const caseDisplay = node.case_sensitive
        ? " (case-sensitive)"
        : " (case-insensitive)";
      return `<div class="tree-node tree-condition">${indent}(${this.escapeHtml(node.field)} ${negatePrefix}${node.operator} "${this.escapeHtml(node.value)}"${caseDisplay})</div>`;
    }

    return "";
  }

  // Generate expression tree for a filter on the main page
  generateFilterExpressionTree(filterData) {
    // Check if we have expression_tree from the API
    if (filterData.expression_tree) {
      return `
        <div class="expression-tree-container">
          <div class="expression-tree-header">
            <strong>Logical Structure:</strong>
          </div>
          <div class="expression-tree">
            ${this.renderFilterTreeNode(filterData.expression_tree, 0)}
          </div>
        </div>
      `;
    }

    // Fallback: try to parse condition_tree directly (for legacy support)
    const filter = filterData.filter || filterData;
    if (!filter.condition_tree || filter.condition_tree.trim() === "") {
      return "";
    }

    try {
      const tree = JSON.parse(filter.condition_tree);
      if (!tree) return "";

      return `
        <div class="expression-tree-container">
          <div class="expression-tree-header">
            <strong>Logical Structure:</strong>
          </div>
          <div class="expression-tree">
            ${this.renderFilterTreeNode(tree, 0)}
          </div>
        </div>
      `;
    } catch (error) {
      console.warn(
        "Failed to render filter expression tree for filter",
        filter.id,
        ":",
        error,
      );
      return "";
    }
  }

  // Validation methods for live expression preview
  validateFilterExpression() {
    const expression = document.getElementById("filterPattern")?.value?.trim();
    const sourceType = document.getElementById("filterSourceType")?.value;
    const textarea = document.getElementById("filterPattern");
    const validationDiv = document.getElementById("filterExpressionValidation");

    if (!validationDiv || !textarea) return;

    // Clear previous validation
    validationDiv.innerHTML = "";

    if (!expression) {
      textarea.classList.remove("is-invalid", "is-valid");
      return;
    }

    if (!sourceType) {
      validationDiv.innerHTML = `
        <div class="alert alert-warning py-2">
          <small>⚠️ Please select a source type first</small>
        </div>
      `;
      textarea.classList.remove("is-valid");
      textarea.classList.add("is-invalid");
      return;
    }

    // Show loading indicator
    validationDiv.innerHTML = `
      <div class="d-flex align-items-center text-muted small">
        <div class="spinner-border spinner-border-sm me-2" role="status" style="width: 12px; height: 12px;">
          <span class="visually-hidden">Loading...</span>
        </div>
        Validating filter expression...
      </div>
    `;

    // Validate filter expression
    this.validateFilterExpressionAsync(expression, sourceType)
      .then((result) => {
        if (result.is_valid) {
          let validationHtml = `
            <div class="alert alert-success py-2">
              <small>✅ Filter expression is valid</small>
            </div>
          `;

          // Add expression tree if available
          if (result.expression_tree) {
            validationHtml += this.generateFilterExpressionTreeHtml(
              result.expression_tree,
            );
          }

          validationDiv.innerHTML = validationHtml;
          textarea.classList.remove("is-invalid");
          textarea.classList.add("is-valid");
        } else {
          validationDiv.innerHTML = `
            <div class="alert alert-danger py-2">
              <small>❌ ${result.error || "Filter validation failed"}</small>
            </div>
          `;
          textarea.classList.remove("is-valid");
          textarea.classList.add("is-invalid");
        }
      })
      .catch((error) => {
        validationDiv.innerHTML = `
          <div class="alert alert-danger py-2">
            <small>❌ Validation error: ${error.message}</small>
          </div>
        `;
        textarea.classList.remove("is-valid");
        textarea.classList.add("is-invalid");
      });
  }

  async validateFilterExpressionAsync(expression, sourceType) {
    const response = await fetch("/api/v1/filters/validate", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        filter_expression: expression,
        source_type: sourceType,
        source_id: "00000000-0000-0000-0000-000000000000", // Dummy UUID for validation
        is_inverse: false,
      }),
    });

    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }

    return await response.json();
  }

  debouncedValidateFilterExpression() {
    if (this.validationTimeout) {
      clearTimeout(this.validationTimeout);
    }
    this.validationTimeout = setTimeout(() => {
      this.validateFilterExpression();
    }, 500);
  }
}

let filtersManager;

async function initializeFiltersManager() {
  console.log("Initializing FiltersManager..."); // Debug log
  filtersManager = new FiltersManager();
  await filtersManager.init();

  // Setup standard modal close handlers
  SharedUtils.setupStandardModalCloseHandlers("filterModal");
  SharedUtils.setupStandardModalCloseHandlers("examplesModal");
}

// Check if DOM is already loaded, if so initialize immediately
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", initializeFiltersManager);
} else {
  // DOM is already loaded, initialize immediately
  initializeFiltersManager();
}

// Helper function to show examples modal
function showFilterExamples() {
  filtersManager.showExamplesModal();
}

// Debug function for validation highlighting (console: testValidationHighlighting())
window.testValidationHighlighting = function () {
  if (!filtersManager || !document.getElementById("filterPattern")) {
    console.error("Open a filter modal first");
    return;
  }

  const tests = [
    'channel_name contans "test"',
    'invalid_field contains "test"',
    "channel_name contains test",
    'channel_name contains "test',
    '() AND channel_name contains "test"',
  ];

  tests.forEach((pattern, i) => {
    const textarea = document.getElementById("filterPattern");
    textarea.value = pattern;
    const result = filtersManager.validatePatternSyntax(pattern);
    console.log(
      `${i + 1}. "${pattern}" → ${result.highlights.length} highlights`,
    );
    if (result.highlights.length > 0)
      filtersManager.updateValidationHighlights(textarea, result);
  });
};
