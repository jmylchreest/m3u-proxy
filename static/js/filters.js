// Filters Management JavaScript
class FiltersManager {
  constructor() {
    this.filters = [];
    this.sources = [];
    this.isEditing = false;
    this.currentFilter = null;
    this.availableFields = [];
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

    // Optional close button (may not exist after removing visual builder)
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
    });

    // Pattern textarea change
    document.getElementById("filterPattern").addEventListener("input", () => {
      this.updateTestButton();
      this.updateFilterPreview();
      this.validatePattern();
    });

    // Also handle paste events in pattern textarea
    document.getElementById("filterPattern").addEventListener("paste", () => {
      setTimeout(() => {
        this.updateTestButton();
        this.updateFilterPreview();
        this.validatePattern();
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
      const response = await fetch("/api/filters/fields");
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
      this.availableFields = await response.json();
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
    }
  }

  async loadSources() {
    try {
      const response = await fetch("/api/sources");
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
      this.sources = await response.json();
      this.populateSourceSelect();
    } catch (error) {
      console.error("Failed to load sources:", error);
      this.showAlert("Failed to load sources", "danger");
    }
  }

  populateSourceSelect() {
    const select = document.getElementById("testSource");
    if (!select) {
      console.warn("testSource element not found");
      return;
    }

    select.innerHTML =
      '<option value="">Select a source to test the pattern...</option>';

    this.sources.forEach((sourceData) => {
      const option = document.createElement("option");
      option.value = sourceData.id;
      option.textContent = `${sourceData.name} (${sourceData.channel_count} channels)`;
      select.appendChild(option);
    });
  }

  async loadFilters() {
    this.showLoading();
    try {
      console.log("Loading filters..."); // Debug log
      const response = await fetch("/api/filters?" + new Date().getTime());
      console.log("Filters API response status:", response.status); // Debug log
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
      const data = await response.json();
      console.log("Filters data:", data); // Debug log
      console.log(
        "Filters data type:",
        typeof data,
        "Array?",
        Array.isArray(data),
      ); // Debug log
      this.filters = Array.isArray(data) ? data : [];
      console.log("Processed filters count:", this.filters.length); // Debug log
      this.renderFilters();
    } catch (error) {
      console.error("Failed to load filters:", error);
      this.showAlert("Failed to load filters", "danger");
    } finally {
      this.hideLoading();
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
    const sortedFilters = [...this.filters].sort((a, b) =>
      a.name.localeCompare(b.name, undefined, {
        numeric: true,
        sensitivity: "base",
      }),
    );

    tbody.innerHTML = sortedFilters
      .map((filterData, index) => {
        // Generate filter preview from condition_tree or conditions
        let filterPreview = "No conditions";
        try {
          if (filterData.condition_tree) {
            const tree = JSON.parse(filterData.condition_tree);
            filterPreview = this.convertTreeToPattern(tree);
          } else if (
            filterData.conditions &&
            filterData.conditions.length > 0
          ) {
            filterPreview = this.generateFilterPreview(filterData);
          }
        } catch (e) {
          console.warn("Failed to parse filter for preview:", e);
          filterPreview = "Invalid filter pattern";
        }

        return `
                <div class="filter-card">
                    <div class="filter-header">
                        <div class="filter-info">
                            <div class="filter-name">
                                <strong>${this.escapeHtml(filterData.name)}</strong>
                            </div>
                            <div class="filter-meta">
                                <span class="badge ${filterData.is_inverse ? "badge-danger" : "badge-success"}">
                                    ${filterData.is_inverse ? "Exclude" : "Include"}
                                </span>
                                <span class="filter-starting-number">Start: ${filterData.starting_channel_number}</span>
                                <span class="badge ${filterData.usage_count > 0 ? "badge-primary" : "badge-secondary"}">
                                    ${filterData.usage_count} ${filterData.usage_count === 1 ? "proxy" : "proxies"}
                                </span>
                            </div>
                        </div>
                        <div class="filter-actions">
                            ${this.renderActionsCell(filterData, filterData.usage_count)}
                        </div>
                    </div>
                    <div class="filter-pattern">
                        <div class="pattern-label">Filter:</div>
                        <pre class="pattern-code"><code>${this.escapeHtml(filterPreview)}</code></pre>
                    </div>
                </div>
            `;
      })
      .join("");
  }

  renderActionsCell(filter, usageCount) {
    return `
            <div class="filter-action-buttons">
                <button class="btn btn-primary btn-sm btn-edit" onclick="filtersManager.editFilter('${filter.id}')">
                    Edit
                </button>
                <button class="btn btn-success btn-sm btn-duplicate" onclick="filtersManager.duplicateFilter('${filter.id}')">
                    Duplicate
                </button>
                <button class="btn btn-danger btn-sm btn-delete" onclick="filtersManager.deleteFilter('${filter.id}')" ${usageCount > 0 ? 'title="Cannot delete: filter is in use by ' + usageCount + ' proxy/proxies"' : ""} ${usageCount > 0 ? "disabled" : ""}>
                    Delete
                </button>
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

    // Filter builder state removed - only pattern mode remains

    if (filter) {
      document.getElementById("filterName").value = filter.name;
      document.getElementById("isInverse").checked = filter.is_inverse;
      document.getElementById("startingChannelNumber").value =
        filter.starting_channel_number;

      // Populate the pattern field with text representation from conditions
      this.populateAdvancedTabFromConditions(filter);
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
        ? `/api/filters/${this.currentFilter.id}`
        : "/api/filters";
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
    const advancedFilter = this.convertPatternToAdvancedFilter();

    // Extract conditions and logical operator from the advanced filter structure
    let conditions = [];
    let logicalOperator = "AND";

    if (advancedFilter && advancedFilter.root_group) {
      conditions = advancedFilter.root_group.conditions.map((condition) => ({
        field_name: condition.field,
        operator: condition.operator,
        value: condition.value,
      }));
      logicalOperator = advancedFilter.root_group.logical_operator;
    }

    return {
      name: document.getElementById("filterName").value.trim(),
      starting_channel_number: parseInt(
        document.getElementById("startingChannelNumber").value,
      ),
      is_inverse: document.getElementById("isInverse").checked,
      conditions: conditions,
      logical_operator: logicalOperator,
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

    return true;
  }

  async editFilter(filterId) {
    const filterData = this.filters.find((f) => f.id === filterId);
    if (filterData) {
      this.showFilterModal(filterData);
    }
  }

  async duplicateFilter(filterId) {
    const filterData = this.filters.find((f) => f.id === filterId);
    if (filterData) {
      const duplicateFilter = {
        ...filterData,
        name: `${filterData.name} (Copy)`,
        id: null,
      };
      this.showFilterModal(duplicateFilter);
    }
  }

  async deleteFilter(filterId) {
    const filterData = this.filters.find((f) => f.id === filterId);
    if (!filterData) return;

    if (filterData.usage_count > 0) {
      this.showAlert(
        `Cannot delete filter "${filterData.name}" as it is being used by ${filterData.usage_count} ${filterData.usage_count === 1 ? "proxy" : "proxies"}`,
        "danger",
      );
      return;
    }

    if (
      !confirm(
        `Are you sure you want to delete the filter "${filterData.name}"? This action cannot be undone.`,
      )
    ) {
      return;
    }

    try {
      const response = await fetch(`/api/filters/${filterData.id}`, {
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

    const advancedFilter = this.convertPatternToAdvancedFilter();

    const pattern = document.getElementById("filterPattern").value.trim();
    if (!sourceId || !pattern) {
      this.showAlert(
        "Please select a source and enter a pattern to test",
        "warning",
      );
      return;
    }

    // Extract conditions and logical operator from the advanced filter structure
    let conditions = [];
    let logicalOperator = "AND";

    if (advancedFilter && advancedFilter.root_group) {
      conditions = advancedFilter.root_group.conditions.map((condition) => ({
        field_name: condition.field,
        operator: condition.operator,
        value: condition.value,
      }));
      logicalOperator = advancedFilter.root_group.logical_operator;
    }

    const testData = {
      source_id: sourceId,
      conditions: conditions,
      logical_operator: logicalOperator,
      is_inverse: isInverse,
    };

    const testBtn = document.getElementById("testPatternBtn");
    const originalText = testBtn.textContent;
    testBtn.textContent = "Testing...";
    testBtn.disabled = true;

    try {
      const response = await fetch("/api/filters/test", {
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
    if (!result.is_valid) {
      this.showAlert(result.error || "Filter test failed", "danger");
      return;
    }

    // Get source name for display
    const sourceId = document.getElementById("testSource").value;
    const sourceSelect = document.getElementById("testSource");
    const sourceName =
      sourceSelect.options[sourceSelect.selectedIndex]?.text ||
      "Unknown Source";

    const percentage =
      result.total_channels > 0
        ? Math.round((result.matched_count / result.total_channels) * 100)
        : 0;

    // Show success message with summary
    this.showAlert(
      `Filter test completed: ${result.matched_count} of ${result.total_channels} channels matched (${percentage}%)`,
      "success",
    );

    // Use shared channel browser to display filtered results
    if (result.matching_channels && result.matching_channels.length > 0) {
      // Format the channels for the shared viewer
      const formattedChannels = result.matching_channels.map((channel) => ({
        channel_name: channel.channel_name,
        tvg_name: channel.tvg_name || channel.channel_name,
        group_title: channel.group_title || null,
        tvg_id: channel.tvg_id || null,
        tvg_logo: channel.tvg_logo || null,
      }));

      // Show channels in the shared modal with a descriptive title
      const modalTitle = `Filter Test Results - ${sourceName} (${result.matched_count} matches)`;
      channelsViewer.showChannels(sourceId, modalTitle, formattedChannels);
    } else {
      this.showAlert("No channels matched the filter criteria", "info");
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
      const fieldName = field
        ? field.display_name || field.name
        : condition.field_name;
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
      const fieldName = field
        ? field.display_name || field.name
        : condition.field_name;
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
    const operatorMap = {
      matches: "matches pattern",
      notmatches: "does not match pattern",
      equals: "equals",
      not_equals: "does not equal",
      contains: "contains",
      not_contains: "does not contain",
      starts_with: "starts with",
      ends_with: "ends with",
    };

    return operatorMap[operator] || operator.replace(/_/g, " ").toLowerCase();
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

  // Visual builder methods removed - only text pattern mode remains

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

  convertPatternToAdvancedFilter() {
    const pattern = document.getElementById("filterPattern").value.trim();
    if (!pattern) {
      return null;
    }

    // Try to parse as text pattern first, fallback to regex
    const textFilter = this.parseTextPattern(pattern);
    if (textFilter) {
      return textFilter;
    }

    // Fallback: treat as raw regex for channel_name
    return {
      root_group: {
        conditions: [
          {
            field: "channel_name",
            operator: "matches",
            value: pattern,
          },
        ],
        groups: [],
        logical_operator: "and",
      },
    };
  }

  // Convert visual conditions to text representation
  convertAdvancedFilterToText(advancedFilter) {
    if (!advancedFilter || !advancedFilter.root_group) {
      return "";
    }

    const group = advancedFilter.root_group;
    const conditions = group.conditions || [];

    if (conditions.length === 0) {
      return "";
    }

    const conditionTexts = conditions.map((condition) => {
      const field = condition.field;
      const operator = condition.operator;
      const value = condition.value;

      // Quote the value if it contains spaces or special characters
      const quotedValue =
        value.includes(" ") || value.includes('"') || value.includes("'")
          ? `"${value.replace(/"/g, '\\"')}"`
          : `"${value}"`;

      return `${field} ${operator} ${quotedValue}`;
    });

    const logicalOp = group.logical_operator.toUpperCase();
    return conditionTexts.join(` ${logicalOp} `);
  }

  // Parse text pattern into advanced filter
  parseTextPattern(text) {
    try {
      // Split by AND/OR while preserving the operators
      const tokens = text.split(/\s+(AND|OR)\s+/i);
      const conditions = [];
      let currentLogicalOp = "and";

      for (let i = 0; i < tokens.length; i += 2) {
        const conditionText = tokens[i].trim();
        if (!conditionText) continue;

        const condition = this.parseCondition(conditionText);
        if (condition) {
          conditions.push(condition);
        }

        // Get the logical operator for next iteration
        if (i + 1 < tokens.length) {
          const nextOp = tokens[i + 1].toLowerCase();
          if (nextOp === "or") {
            currentLogicalOp = "or";
          }
          // Note: Mixed AND/OR not supported in current structure
        }
      }

      if (conditions.length === 0) {
        return null;
      }

      return {
        root_group: {
          conditions: conditions,
          groups: [],
          logical_operator: currentLogicalOp,
        },
      };
    } catch (error) {
      console.warn("Failed to parse text pattern:", error);
      return null;
    }
  }

  // Parse individual condition like "channel_name contains 'sport'"
  parseCondition(conditionText) {
    // Match pattern: field operator "value" or field operator 'value'
    const match = conditionText.match(
      /^(\w+)\s+(\w+(?:_\w+)*)\s+["']([^"']+)["']$/,
    );
    if (!match) {
      console.warn("Could not parse condition:", conditionText);
      return null;
    }

    const [, field, operator, value] = match;

    // Validate field
    const validFields = [
      "channel_name",
      "group_title",
      "tvg_id",
      "tvg_name",
      "stream_url",
    ];
    if (!validFields.includes(field)) {
      console.warn("Invalid field:", field);
      return null;
    }

    // Validate operator
    const validOperators = [
      "contains",
      "equals",
      "matches",
      "starts_with",
      "ends_with",
      "not_contains",
      "not_equals",
      "not matches",
    ];
    if (!validOperators.includes(operator)) {
      console.warn("Invalid operator:", operator);
      return null;
    }

    return {
      field: field,
      operator: operator,
      value: value,
    };
  }

  convertCurrentFilterToAdvanced() {
    return this.convertPatternToAdvancedFilter();
  }

  // loadAdvancedFilter method removed - no longer needed without visual builder

  // Populate the advanced tab with text representation of the filter
  populateAdvancedTab(advancedFilter) {
    const textPattern = this.convertAdvancedFilterToText(
      typeof advancedFilter === "string"
        ? JSON.parse(advancedFilter)
        : advancedFilter,
    );
    document.getElementById("filterPattern").value = textPattern;
    this.validatePattern();
  }

  // Visual builder methods removed - only text pattern mode remains

  // Populate pattern from conditions array or condition_tree
  populateAdvancedTabFromConditions(filter) {
    let textPattern = "";

    // Try to parse condition_tree first (current format)
    if (filter.condition_tree) {
      try {
        const tree = JSON.parse(filter.condition_tree);
        textPattern = this.convertTreeToPattern(tree);
      } catch (e) {
        console.error("Failed to parse condition_tree:", e);
        textPattern = "// Complex filter - edit in JSON format if needed";
      }
    }
    // Fall back to conditions array (legacy format)
    else if (filter.conditions && filter.conditions.length > 0) {
      const operator = filter.logical_operator === "any" ? " OR " : " AND ";
      textPattern = filter.conditions
        .map((condition) => {
          const field = this.availableFields.find(
            (f) => f.name === condition.field_name,
          );
          const fieldName = field ? field.name : condition.field_name;
          return `${fieldName} ${condition.operator.replace("_", " ")} "${condition.value}"`;
        })
        .join(operator);
    }

    document.getElementById("filterPattern").value = textPattern;
    this.validatePattern();
  }

  // Convert condition tree to natural language pattern
  convertTreeToPattern(tree) {
    if (!tree) return "";

    if (tree.type === "condition") {
      const fieldName = tree.field || "field";
      let operator = tree.operator || "equals";
      const value = tree.value || "";

      // Convert database operators to natural language
      const operatorMap = {
        matches: "matches",
        not_matches: "not matches",
        equals: "equals",
        not_equals: "not equals",
        contains: "contains",
        not_contains: "not contains",
        starts_with: "starts_with",
        ends_with: "ends_with",
      };

      const mappedOperator = operatorMap[operator] || operator;

      return `${fieldName} ${mappedOperator} "${value}"`;
    }

    if (tree.type === "group" && tree.children) {
      const logicalOp = tree.operator === "any" ? " OR " : " AND ";
      const childPatterns = tree.children
        .map((child) => this.convertTreeToPattern(child))
        .filter((pattern) => pattern.length > 0);

      if (childPatterns.length === 0) return "";
      if (childPatterns.length === 1) return childPatterns[0];

      // Wrap in parentheses if multiple conditions
      return `(${childPatterns.join(logicalOp)})`;
    }

    return "";
  }

  // Visual builder methods removed - only text pattern mode remains

  // Comprehensive pattern validation
  validatePattern() {
    const textarea = document.getElementById("filterPattern");
    const validationContainer = document.getElementById("patternValidation");

    if (!textarea) {
      console.warn("filterPattern element not found, skipping validation");
      return;
    }

    const pattern = textarea.value.trim();

    // Show validation container if there's content and the container exists
    if (validationContainer) {
      if (pattern) {
        validationContainer.style.display = "block";
      } else {
        validationContainer.style.display = "none";
        textarea.classList.remove("valid", "invalid", "warning");
        return;
      }
    }

    if (!pattern) {
      textarea.classList.remove("valid", "invalid", "warning");
      return;
    }

    const validation = this.performPatternValidation(pattern);
    this.updateValidationUI(validation);
  }

  // Perform comprehensive validation checks
  performPatternValidation(pattern) {
    const result = {
      syntax: { valid: true, messages: [] },
      fields: { valid: true, messages: [] },
      operators: { valid: true, messages: [] },
      regex: { valid: true, messages: [] },
      overall: "valid",
    };

    try {
      // 1. Syntax validation
      const syntaxCheck = this.validateSyntax(pattern);
      result.syntax = syntaxCheck;

      if (syntaxCheck.valid) {
        // 2. Field validation
        const fieldCheck = this.validateFields(pattern);
        result.fields = fieldCheck;

        // 3. Operator validation
        const operatorCheck = this.validateOperators(pattern);
        result.operators = operatorCheck;

        // 4. Regex validation (for values using 'matches' operator)
        const regexCheck = this.validateRegexPatterns(pattern);
        result.regex = regexCheck;
      }

      // Determine overall status
      if (
        !result.syntax.valid ||
        !result.fields.valid ||
        !result.operators.valid
      ) {
        result.overall = "invalid";
      } else if (!result.regex.valid) {
        result.overall = "warning";
      } else {
        result.overall = "valid";
      }
    } catch (error) {
      result.syntax.valid = false;
      result.syntax.messages = [`Parse error: ${error.message}`];
      result.overall = "invalid";
    }

    return result;
  }

  // Validate syntax structure
  validateSyntax(pattern) {
    const result = { valid: true, messages: [] };

    // Check for balanced quotes
    const singleQuotes = (pattern.match(/'/g) || []).length;
    const doubleQuotes = (pattern.match(/"/g) || []).length;

    if (singleQuotes % 2 !== 0) {
      result.valid = false;
      result.messages.push("Unmatched single quotes");
    }
    if (doubleQuotes % 2 !== 0) {
      result.valid = false;
      result.messages.push("Unmatched double quotes");
    }

    // Check for balanced parentheses
    let parenCount = 0;
    for (let i = 0; i < pattern.length; i++) {
      if (pattern[i] === "(") parenCount++;
      if (pattern[i] === ")") parenCount--;
      if (parenCount < 0) {
        result.valid = false;
        result.messages.push("Unmatched closing parenthesis");
        break;
      }
    }
    if (parenCount > 0) {
      result.valid = false;
      result.messages.push("Unmatched opening parenthesis");
    }

    // If basic checks failed, don't proceed with further validation
    if (!result.valid) {
      return result;
    }

    // Improved validation for patterns with parentheses
    if (pattern.includes("(") && pattern.includes(")")) {
      // Remove parentheses ONLY outside of quoted strings to preserve regex patterns
      let withoutParens = pattern;
      let inQuotes = false;
      let quoteChar = "";
      let result_str = "";

      for (let i = 0; i < pattern.length; i++) {
        const char = pattern[i];
        if (!inQuotes && (char === '"' || char === "'")) {
          inQuotes = true;
          quoteChar = char;
          result_str += char;
        } else if (inQuotes && char === quoteChar) {
          inQuotes = false;
          quoteChar = "";
          result_str += char;
        } else if (inQuotes) {
          // Preserve all characters inside quotes, including parentheses
          result_str += char;
        } else if (char === "(" || char === ")") {
          // Replace parentheses outside of quotes with spaces
          result_str += " ";
        } else {
          result_str += char;
        }
      }
      withoutParens = result_str.trim();

      // Check that we have valid field operator "value" patterns with optional modifiers
      const basicConditionPattern =
        /(?:(?:not|case_sensitive)\s+)*\w+\s+(?:(?:not|case_sensitive)\s+)*(?:contains|equals|matches|starts_with|ends_with)\s+["'][^"']*["']/g;
      const conditions = withoutParens.match(basicConditionPattern);

      if (!conditions || conditions.length === 0) {
        result.valid = false;
        result.messages.push("No valid conditions found in pattern");
      } else {
        // Validate that logical operators are used correctly
        const logicalOps = withoutParens.match(/\b(?:AND|OR)\b/gi);
        // For complex patterns, be more lenient - just check that we have conditions
        // The backend will handle the actual parsing
        if (conditions.length === 0) {
          result.valid = false;
          result.messages.push("No valid conditions found in pattern");
        }
      }
    } else {
      // For simple patterns without parentheses, use strict validation
      const conditionPattern =
        /^\s*(?:(?:not|case_sensitive)\s+)*\w+\s+(?:(?:not|case_sensitive)\s+)*(?:contains|equals|matches|starts_with|ends_with)\s+["'][^"']*["']\s*(?:\s+(?:AND|OR)\s+(?:(?:not|case_sensitive)\s+)*\w+\s+(?:(?:not|case_sensitive)\s+)*(?:contains|equals|matches|starts_with|ends_with)\s+["'][^"']*["']\s*)*$/i;

      if (!conditionPattern.test(pattern)) {
        result.valid = false;
        result.messages.push(
          'Invalid syntax structure. Expected: [not] [case_sensitive] field_name operator "value" [AND|OR [not] [case_sensitive] field_name operator "value"]. Example: channel_name contains "sport" AND group_title not contains "adult"',
        );
      }
    }

    if (result.valid) {
      result.messages.push("Syntax structure is correct");
    }

    return result;
  }

  // Validate field names
  validateFields(pattern) {
    const result = { valid: true, messages: [] };
    const validFields = [
      "channel_name",
      "group_title",
      "tvg_id",
      "tvg_name",
      "stream_url",
    ];

    // Improved regex to handle modifiers properly (modifiers can come before or after field name)
    const fieldMatches = pattern.match(
      /(?:(?:not|case_sensitive)\s+)*\b(\w+)\s+(?:(?:not|case_sensitive)\s+)*(?:contains|equals|matches|starts_with|ends_with)\s+["']/g,
    );
    const foundFields = new Set();
    const invalidFields = [];

    if (fieldMatches) {
      fieldMatches.forEach((match) => {
        // Extract field name, accounting for modifiers that might come before it
        const parts = match.split(/\s+/);
        let field = null;

        // Find the field name (first word that's not a modifier)
        for (const part of parts) {
          if (
            part !== "not" &&
            part !== "case_sensitive" &&
            !part.match(/^(?:contains|equals|matches|starts_with|ends_with)$/)
          ) {
            field = part;
            break;
          }
        }

        if (field) {
          foundFields.add(field);
          if (!validFields.includes(field)) {
            invalidFields.push(field);
          }
        }
      });
    }

    if (invalidFields.length > 0) {
      result.valid = false;
      result.messages.push(
        `Invalid field(s): ${invalidFields.join(", ")}. Valid fields are: ${validFields.join(", ")}`,
      );
    } else if (foundFields.size > 0) {
      result.messages.push(
        `Using valid field(s): ${Array.from(foundFields).join(", ")}`,
      );
    }

    return result;
  }

  // Validate operators
  validateOperators(pattern) {
    const result = { valid: true, messages: [] };
    const baseOperators = [
      "contains",
      "equals",
      "matches",
      "starts_with",
      "ends_with",
    ];
    const modifiers = ["not", "case_sensitive"];

    // Extract operators from pattern using more flexible regex
    const operatorMatches = pattern.match(
      /\w+\s+((?:(?:not|case_sensitive)\s+)*(?:contains|equals|matches|starts_with|ends_with))\s+["']/g,
    );
    const foundOperators = new Set();
    const invalidOperators = [];

    if (operatorMatches) {
      operatorMatches.forEach((match) => {
        // Extract the operator part (everything between field and value)
        const parts = match.split(/\s+/);
        const operatorPart = parts.slice(1, -1).join(" "); // Remove field and quoted value
        foundOperators.add(operatorPart);

        // Validate the operator part
        const operatorWords = operatorPart.split(/\s+/);
        const baseOp = operatorWords[operatorWords.length - 1]; // Last word should be base operator
        const mods = operatorWords.slice(0, -1); // Everything before should be modifiers

        let isValid = baseOperators.includes(baseOp);
        if (isValid) {
          // Check all modifiers are valid
          for (const mod of mods) {
            if (!modifiers.includes(mod)) {
              isValid = false;
              break;
            }
          }
        }

        if (!isValid) {
          invalidOperators.push(operatorPart);
        }
      });
    }

    if (invalidOperators.length > 0) {
      result.valid = false;
      result.messages.push(
        `Invalid operator(s): ${invalidOperators.join(", ")}. Valid operators: ${baseOperators.join(", ")}. You can add 'not' or 'case_sensitive' before operators (e.g., "not contains", "case_sensitive equals")`,
      );
    }

    return result;
  }

  // Validate regex patterns in values
  validateRegexPatterns(pattern) {
    const result = { valid: true, messages: [] };

    // Find all conditions with 'matches' operator
    const matchesConditions = pattern.match(
      /\w+\s+(?:(?:not|case_sensitive)\s+)*matches\s+["']([^"']+)["']/gi,
    );
    const warningRegex = [];
    let regexCount = 0;

    if (matchesConditions) {
      matchesConditions.forEach((match) => {
        const regexMatch = match.match(/["']([^"']+)["']/);
        if (regexMatch) {
          const regexPattern = regexMatch[1];
          regexCount++;

          // Check for unsupported regex features
          if (regexPattern.includes("(?i)")) {
            result.valid = false;
            result.messages.push(
              `"${regexPattern}": (?i) flag is not supported. The 'matches' operator is case-insensitive by default.`,
            );
          } else if (regexPattern.includes("(?")) {
            result.valid = false;
            result.messages.push(
              `"${regexPattern}": Advanced regex flags are not supported by the server's regex engine.`,
            );
          } else {
            // Test with Rust-compatible regex features only
            try {
              // Basic validation - don't try to emulate Rust regex exactly,
              // just catch obvious syntax errors
              new RegExp(regexPattern);
              // Additional validation for common regex patterns that are valid in Rust
              // Allow patterns with alternation (|), quantifiers (?*+), and character classes
              // These are supported by the Rust regex engine
            } catch (error) {
              // Only mark as invalid for genuine syntax errors
              // Don't fail on advanced regex features that might work in Rust
              if (error.message.includes("Invalid regular expression")) {
                result.valid = false;
                result.messages.push(`"${regexPattern}": ${error.message}`);
              } else {
                // For other errors, show as warning but allow submission
                result.messages.push(
                  `"${regexPattern}": Warning - ${error.message}. Pattern will be validated by server.`,
                );
              }
            }
          }
        }
      });
    }

    if (regexCount > 0 && result.valid) {
      result.messages.push(`${regexCount} valid regex pattern(s) found`);
    }

    return result;
  }

  // Update validation UI elements
  updateValidationUI(validation) {
    const textarea = document.getElementById("filterPattern");
    const syntaxIcon = document.getElementById("syntaxIcon");
    const fieldsIcon = document.getElementById("fieldsIcon");
    const operatorsIcon = document.getElementById("operatorsIcon");
    const regexIcon = document.getElementById("regexIcon");
    const messagesContainer = document.getElementById("validationMessages");

    // Update textarea styling
    if (textarea) {
      textarea.classList.remove("valid", "invalid", "warning");
      textarea.classList.add(validation.overall);

      // Add precise highlighting only for specific field/operator errors
      // Don't highlight for general syntax issues
      if (
        validation.overall === "invalid" &&
        (!validation.fields.valid ||
          !validation.operators.valid ||
          !validation.regex.valid)
      ) {
        this.addPreciseHighlighting(textarea, validation);
      } else {
        this.removePreciseHighlighting(textarea);
      }
    }

    // Update validation icons (only if they exist)
    if (syntaxIcon || fieldsIcon || operatorsIcon || regexIcon) {
      this.updateValidationIcon(syntaxIcon, validation.syntax, "Syntax");
      this.updateValidationIcon(fieldsIcon, validation.fields, "Fields");
      this.updateValidationIcon(
        operatorsIcon,
        validation.operators,
        "Operators",
      );
      this.updateValidationIcon(regexIcon, validation.regex, "Regex");
    }

    // Update messages (only if container exists)
    if (!messagesContainer) {
      // Silently skip if validation messages container doesn't exist
      return;
    }

    messagesContainer.innerHTML = "";

    const allMessages = [
      ...validation.syntax.messages,
      ...validation.fields.messages,
      ...validation.operators.messages,
      ...validation.regex.messages,
    ];

    allMessages.forEach((message) => {
      const messageDiv = document.createElement("div");
      messageDiv.className = "validation-message";

      if (message.includes("Invalid") || message.includes("error")) {
        messageDiv.classList.add("error");
      } else if (
        message.includes("pattern(s) validated") ||
        message.includes("correct") ||
        message.includes("Using")
      ) {
        messageDiv.classList.add("success");
      } else {
        messageDiv.classList.add("warning");
      }

      messageDiv.textContent = message;
      messagesContainer.appendChild(messageDiv);
    });

    // Show overall status if no specific messages
    if (allMessages.length === 0) {
      const messageDiv = document.createElement("div");
      messageDiv.className = "validation-message success";
      messageDiv.textContent = "Pattern is valid and ready to use";
      messagesContainer.appendChild(messageDiv);
    }
  }

  // Update individual validation icon
  updateValidationIcon(icon, validation, label) {
    if (!icon) return; // Skip if element doesn't exist

    icon.classList.remove("valid", "invalid", "warning");
    icon.textContent = label;

    if (validation.valid) {
      icon.classList.add("valid");
      icon.title = `${label}: Valid`;
    } else {
      icon.classList.add("invalid");
      icon.title = `${label}: ${validation.messages[0] || "Invalid"}`;
    }
  }

  // Add precise highlighting for validation errors
  addPreciseHighlighting(textarea, validation) {
    this.removePreciseHighlighting(textarea);

    const pattern = textarea.value;
    const invalidRanges = this.findInvalidRanges(pattern, validation);

    if (invalidRanges.length === 0) {
      // No specific errors found, don't highlight anything
      return;
    }

    // Create wrapper if it doesn't exist
    let wrapper = textarea.parentNode;
    if (!wrapper.classList.contains("validation-wrapper")) {
      const newWrapper = document.createElement("div");
      newWrapper.className = "validation-wrapper";
      newWrapper.style.position = "relative";
      textarea.parentNode.insertBefore(newWrapper, textarea);
      newWrapper.appendChild(textarea);
      wrapper = newWrapper;
    }

    // Create overlay for highlighting
    const overlay = document.createElement("div");
    overlay.className = "validation-overlay";
    overlay.style.cssText = `
      position: absolute;
      top: 0;
      left: 0;
      width: 100%;
      height: 100%;
      pointer-events: none;
      font-family: ${getComputedStyle(textarea).fontFamily};
      font-size: ${getComputedStyle(textarea).fontSize};
      line-height: ${getComputedStyle(textarea).lineHeight};
      padding: ${getComputedStyle(textarea).padding};
      border: ${getComputedStyle(textarea).borderWidth} solid transparent;
      white-space: pre-wrap;
      word-wrap: break-word;
      overflow: hidden;
      color: transparent;
      z-index: 1;
      background: transparent;
    `;

    // Build highlighted content
    let highlightedContent = "";
    let lastIndex = 0;

    for (const range of invalidRanges) {
      // Add normal text before error (with pointer-events: none)
      highlightedContent += `<span style="pointer-events: none;">${this.escapeHtml(
        pattern.substring(lastIndex, range.start),
      )}</span>`;

      // Add highlighted error text with tooltip
      highlightedContent += `<span class="error-highlight" title="${this.escapeHtml(range.message)}" style="cursor: help; position: relative; pointer-events: auto;" data-tooltip="${this.escapeHtml(range.message)}">${this.escapeHtml(pattern.substring(range.start, range.end))}</span>`;

      lastIndex = range.end;
    }

    // Add remaining text (with pointer-events: none)
    highlightedContent += `<span style="pointer-events: none;">${this.escapeHtml(pattern.substring(lastIndex))}</span>`;

    overlay.innerHTML = highlightedContent;
    wrapper.appendChild(overlay);

    // Add hover and click events for tooltips
    overlay.addEventListener("mouseover", (e) => {
      if (e.target.classList.contains("error-highlight")) {
        const message = e.target.getAttribute("data-tooltip");
        if (message) {
          this.showTooltip(e.target, message);
        }
      }
    });

    overlay.addEventListener("mouseout", (e) => {
      if (e.target.classList.contains("error-highlight")) {
        this.hideTooltip();
      }
    });

    overlay.addEventListener("click", (e) => {
      if (e.target.classList.contains("error-highlight")) {
        e.preventDefault();
        e.stopPropagation();
        const message = e.target.getAttribute("data-tooltip");
        if (message) {
          this.showTooltip(e.target, message, true); // persistent tooltip
        }
      }
    });
  }

  // Remove precise highlighting
  removePreciseHighlighting(textarea) {
    textarea.classList.remove("invalid-syntax-blue");

    const wrapper = textarea.parentNode;
    if (wrapper && wrapper.classList.contains("validation-wrapper")) {
      const overlay = wrapper.querySelector(".validation-overlay");
      if (overlay) {
        overlay.remove();
      }
    }
  }

  // Find invalid text ranges for highlighting
  findInvalidRanges(pattern, validation) {
    const ranges = [];

    // Only find ranges if there are actual validation errors
    // Don't highlight anything if it's just a syntax structure issue with parentheses
    if (validation.syntax.valid) {
      // Find invalid operators
      if (!validation.operators.valid) {
        const baseOperators = [
          "contains",
          "equals",
          "matches",
          "starts_with",
          "ends_with",
        ];
        const modifiers = ["not", "case_sensitive"];

        const operatorRegex =
          /(?:(?:not|case_sensitive)\s+)*\b(\w+)\s+((?:(?:not|case_sensitive)\s+)*(\w+(?:_\w+)*))\s+["'][^"']*["']/g;
        let match;

        while ((match = operatorRegex.exec(pattern)) !== null) {
          const operatorPart = match[2];
          const operatorWords = operatorPart.split(/\s+/);
          const baseOp = operatorWords[operatorWords.length - 1];
          const mods = operatorWords.slice(0, -1);

          let isValid = baseOperators.includes(baseOp);
          if (isValid) {
            for (const mod of mods) {
              if (!modifiers.includes(mod)) {
                isValid = false;
                break;
              }
            }
          }

          if (!isValid) {
            const operatorStart = match.index + match[1].length + 1;
            ranges.push({
              start: operatorStart,
              end: operatorStart + operatorPart.length,
              message: `Invalid operator: "${operatorPart}". Valid operators: contains, equals, matches, starts_with, ends_with. You can prefix with 'not' or 'case_sensitive' (e.g., "not contains", "case_sensitive equals")`,
            });
          }
        }
      }

      // Find invalid fields
      if (!validation.fields.valid) {
        const validFields = [
          "channel_name",
          "group_title",
          "tvg_id",
          "tvg_name",
          "stream_url",
        ];
        const fieldRegex =
          /(?:(?:not|case_sensitive)\s+)*(\w+)\s+(?:(?:not|case_sensitive)\s+)*(?:contains|equals|matches|starts_with|ends_with)\s+["']/g;
        let match;

        while ((match = fieldRegex.exec(pattern)) !== null) {
          const fullMatch = match[0];
          const field = match[1];

          if (!validFields.includes(field)) {
            // Find the actual position of the field name within the match
            const fieldStartInMatch = fullMatch.indexOf(field);
            const fieldStart = match.index + fieldStartInMatch;

            ranges.push({
              start: fieldStart,
              end: fieldStart + field.length,
              message: `Invalid field: "${field}". Valid fields are: ${validFields.join(", ")}`,
            });
          }
        }
      }
    }

    // Sort ranges by start position
    ranges.sort((a, b) => a.start - b.start);
    return ranges;
  }

  // Show a temporary tooltip for better mobile/accessibility support
  showTooltip(element, message, persistent = false) {
    // Remove any existing tooltips
    this.hideTooltip();

    // Create tooltip element
    const tooltip = document.createElement("div");
    tooltip.className = "validation-tooltip";
    tooltip.textContent = message;
    tooltip.style.cssText = `
      position: absolute;
      background: #333;
      color: white;
      padding: 8px 12px;
      border-radius: 4px;
      font-size: 12px;
      z-index: 1000;
      pointer-events: none;
      max-width: 300px;
      word-wrap: break-word;
      white-space: normal;
      box-shadow: 0 2px 8px rgba(0,0,0,0.3);
      opacity: 0;
      transition: opacity 0.2s ease-in;
    `;

    // Position tooltip
    document.body.appendChild(tooltip);
    const rect = element.getBoundingClientRect();
    const tooltipRect = tooltip.getBoundingClientRect();

    tooltip.style.left =
      Math.max(10, rect.left + rect.width / 2 - tooltipRect.width / 2) + "px";
    tooltip.style.top = rect.top - tooltipRect.height - 8 + "px";

    // Fade in
    requestAnimationFrame(() => {
      tooltip.style.opacity = "1";
    });

    // Store reference for cleanup
    this.currentTooltip = tooltip;

    // Auto-remove tooltip after delay (unless persistent)
    if (!persistent) {
      this.tooltipTimeout = setTimeout(() => {
        this.hideTooltip();
      }, 3000);
    }
  }

  // Hide tooltip
  hideTooltip() {
    if (this.currentTooltip) {
      this.currentTooltip.style.opacity = "0";
      setTimeout(() => {
        if (this.currentTooltip && this.currentTooltip.parentNode) {
          this.currentTooltip.remove();
        }
        this.currentTooltip = null;
      }, 200);
    }
    if (this.tooltipTimeout) {
      clearTimeout(this.tooltipTimeout);
      this.tooltipTimeout = null;
    }
  }
}

// Initialize the filters manager when the page loads
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
