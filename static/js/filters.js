// Filters Management JavaScript
class FiltersManager {
  constructor() {
    this.filters = [];
    this.sources = [];
    this.isEditing = false;
    this.currentFilter = null;
    this.filterType = "visual"; // 'visual' or 'advanced'
    this.rootGroup = {
      conditions: [],
      groups: [],
      logical_operator: "and",
    };
    this.conditionCounter = 0;
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
    
    document.getElementById("closeFilterModal").addEventListener("click", () => {
      this.hideFilterModal();
    });

    // Save filter button
    document.getElementById("saveFilter").addEventListener("click", () => {
      this.saveFilter();
    });

    // Test pattern button
    document.getElementById("testPatternBtn").addEventListener("click", () => {
      this.testPattern();
    });

    // Filter type switching
    this.setupFilterBuilder();

    // Test source change
    document.getElementById("testSource").addEventListener("change", (e) => {
      this.updateTestButton();
    });

    // Pattern textarea change
    document.getElementById("filterPattern").addEventListener("input", () => {
      this.updateTestButton();
      this.updateFilterPreview();
      this.syncAdvancedToVisual();
      this.validatePattern();
    });

    // Also handle paste events in pattern textarea
    document.getElementById("filterPattern").addEventListener("paste", () => {
      setTimeout(() => {
        this.updateTestButton();
        this.updateFilterPreview();
        this.syncAdvancedToVisual();
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
      console.log("Filters data type:", typeof data, "Array?", Array.isArray(data)); // Debug log
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
        // Generate filter preview from conditions
        let filterPreview = "No conditions";
        try {
          if (filterData.conditions && filterData.conditions.length > 0) {
            filterPreview = this.generateFilterPreview(filterData);
          }
        } catch (e) {
          console.warn("Failed to parse filter for preview:", e);
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

    // Reset filter builder state
    this.rootGroup = {
      conditions: [],
      groups: [],
      logical_operator: "and",
    };
    this.conditionCounter = 0;

    if (filter) {
      document.getElementById("filterName").value = filter.name;
      document.getElementById("isInverse").checked = filter.is_inverse;
      document.getElementById("startingChannelNumber").value =
        filter.starting_channel_number;

      // Always use visual mode since all filters are stored as conditions
      this.switchFilterType("visual");
      this.loadConditionsIntoBuilder(filter);

      // Also populate the advanced tab with text representation
      this.populateAdvancedTabFromConditions(filter);
    } else {
      document.getElementById("startingChannelNumber").value = 1;
      // Clear the pattern field and ensure placeholder is visible
      document.getElementById("filterPattern").value = "";
      this.switchFilterType("visual");
    }

    // Render conditions and update UI after everything is set up
    setTimeout(() => {
      this.renderConditions();
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
    const advancedFilter =
      this.filterType === "advanced"
        ? this.convertPatternToAdvancedFilter()
        : this.buildAdvancedFilter();

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

    // Validate filter conditions
    if (this.filterType === "advanced") {
      const pattern = document.getElementById("filterPattern").value.trim();
      if (!pattern) {
        this.showAlert("Filter pattern is required", "danger");
        document.getElementById("filterPattern").focus();
        return false;
      }
      // Test regex validity
      try {
        new RegExp(pattern);
      } catch (e) {
        this.showAlert(`Invalid regular expression: ${e.message}`, "danger");
        document.getElementById("filterPattern").focus();
        return false;
      }
    } else {
      if (this.rootGroup.conditions.length === 0) {
        this.showAlert("At least one filter condition is required", "danger");
        return false;
      }
      for (const condition of this.rootGroup.conditions) {
        if (!condition.value.trim()) {
          this.showAlert("All filter conditions must have values", "danger");
          return false;
        }
      }
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

    const advancedFilter =
      this.filterType === "advanced"
        ? this.convertPatternToAdvancedFilter()
        : this.buildAdvancedFilter();

    if (this.filterType === "advanced") {
      const pattern = document.getElementById("filterPattern").value.trim();
      if (!sourceId || !pattern) {
        this.showAlert(
          "Please select a source and enter a pattern to test",
          "danger",
        );
        return;
      }
    } else {
      if (!sourceId || this.rootGroup.conditions.length === 0) {
        this.showAlert(
          "Please select a source and add filter conditions to test",
          "danger",
        );
        return;
      }
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
    const operatorText = operator === "and" ? "ALL" : "ANY";

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

  // Filter Builder Methods
  setupFilterBuilder() {
    // Enable/disable test button based on filter type and content
    this.updateTestButton();
  }

  switchFilterType(type) {
    this.filterType = type;

    // Update tab appearance
    document
      .getElementById("visualFilterTab")
      .classList.toggle("active", type === "visual");
    document
      .getElementById("advancedFilterTab")
      .classList.toggle("active", type === "advanced");

    // Update panel visibility
    document
      .getElementById("visualFilterPanel")
      .classList.toggle("active", type === "visual");
    document
      .getElementById("advancedFilterPanel")
      .classList.toggle("active", type === "advanced");

    // If switching to visual mode and no conditions exist, add a default one
    if (type === "visual" && this.rootGroup.conditions.length === 0) {
      this.addCondition();
    }

    // If switching to advanced mode, validate the current pattern
    if (type === "advanced") {
      this.validatePattern();
    }

    this.updateTestButton();
    this.updateFilterPreview();
  }

  addCondition() {
    const conditionId = ++this.conditionCounter;
    const defaultField =
      this.availableFields.length > 0
        ? this.availableFields[0].name
        : "channel_name";
    const condition = {
      id: conditionId,
      field: defaultField,
      operator: "contains",
      value: "",
    };

    this.rootGroup.conditions.push(condition);
    this.renderConditions();
    this.updateFilterPreview();
    this.updateTestButton();
    this.updateAdvancedTab();

    // Focus on the newly added condition's value input
    setTimeout(() => {
      const newCondition = document.querySelector(
        `[data-condition-id="${conditionId}"] input[type="text"]`,
      );
      if (newCondition) {
        newCondition.focus();
      }
    }, 100);
  }

  removeCondition(conditionId) {
    this.rootGroup.conditions = this.rootGroup.conditions.filter(
      (c) => c.id !== conditionId,
    );
    this.renderConditions();
    this.updateFilterPreview();
    this.updateTestButton();
  }

  updateCondition(conditionId, field, value) {
    const condition = this.rootGroup.conditions.find(
      (c) => c.id === conditionId,
    );
    if (condition) {
      condition[field] = value;
      this.updateFilterPreview();
      this.updateTestButton();
      this.updateAdvancedTab();
    }
  }

  toggleLogicalOperator() {
    this.rootGroup.logical_operator =
      this.rootGroup.logical_operator === "and" ? "or" : "and";
    this.renderConditions();
    this.updateFilterPreview();
    this.updateTestButton();
  }

  renderConditions() {
    const container = document.getElementById("conditionsContainer");
    if (!container) {
      console.warn("filterConditions element not found");
      return;
    }

    if (this.rootGroup.conditions.length === 0) {
      container.innerHTML = `
        <div class="empty-conditions" onclick="filtersManager.addCondition()">
          <p>No conditions added yet.</p>
          <p>Click here or "Add Condition" to start building your filter.</p>
          <button type="button" class="btn btn-sm btn-primary" onclick="event.stopPropagation(); filtersManager.addCondition()">
            + Add Your First Condition
          </button>
        </div>
      `;
      return;
    }

    // Render logical operator selector if there are multiple conditions
    let logicalOperatorSelector = "";
    if (this.rootGroup.conditions.length > 1) {
      logicalOperatorSelector = `
        <div class="logical-operator-selector">
          <label>All conditions must be:</label>
          <div class="operator-buttons">
            <button type="button" class="btn btn-sm ${this.rootGroup.logical_operator === "and" ? "btn-primary" : "btn-outline-primary"}" onclick="filtersManager.setLogicalOperator('and')">
              ALL TRUE (AND)
            </button>
            <button type="button" class="btn btn-sm ${this.rootGroup.logical_operator === "or" ? "btn-primary" : "btn-outline-primary"}" onclick="filtersManager.setLogicalOperator('or')">
              ANY TRUE (OR)
            </button>
          </div>
          <small class="text-muted">
            ${this.rootGroup.logical_operator === "and" ? "All conditions must match for a channel to be included." : "Any condition can match for a channel to be included."}
          </small>
        </div>
      `;
    }

    const conditionsHtml = this.rootGroup.conditions
      .map((condition, index) => {
        const fieldSelectOptions = this.availableFields
          .map(
            (field) =>
              `<option value="${field.name}" ${condition.field === field.name ? "selected" : ""}>${field.display_name}</option>`,
          )
          .join("");

        return `
        <div class="filter-condition" data-condition-id="${condition.id}">
          <div class="condition-number">${index + 1}.</div>
          <div class="filter-condition-field">
            <select onchange="filtersManager.updateCondition(${condition.id}, 'field', this.value)">
              ${fieldSelectOptions}
            </select>

            <select onchange="filtersManager.updateCondition(${condition.id}, 'operator', this.value)">
              <option value="contains" ${condition.operator === "contains" ? "selected" : ""}>contains</option>
              <option value="equals" ${condition.operator === "equals" ? "selected" : ""}>equals</option>
              <option value="matches" ${condition.operator === "matches" ? "selected" : ""}>matches (regex)</option>
              <option value="starts_with" ${condition.operator === "starts_with" ? "selected" : ""}>starts with</option>
              <option value="ends_with" ${condition.operator === "ends_with" ? "selected" : ""}>ends with</option>
              <option value="not_contains" ${condition.operator === "not_contains" ? "selected" : ""}>does not contain</option>
              <option value="not_equals" ${condition.operator === "not_equals" ? "selected" : ""}>does not equal</option>
              <option value="not_matches" ${condition.operator === "not_matches" ? "selected" : ""}>does not match (regex)</option>
            </select>

            <input
              type="text"
              placeholder="Enter value..."
              value="${this.escapeHtml(condition.value)}"
              onchange="filtersManager.updateCondition(${condition.id}, 'value', this.value)"
            />
          </div>

          <div class="filter-condition-actions">
            <button type="button" class="btn btn-sm btn-outline-danger" onclick="filtersManager.removeCondition(${condition.id})">
              ✕
            </button>
          </div>
        </div>
      `;
      })
      .join("");

    container.innerHTML = logicalOperatorSelector + conditionsHtml;
  }

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

    if (this.filterType === "advanced") {
      const patternInput = document.getElementById("filterPattern");
      const pattern = patternInput ? patternInput.value.trim() : "";
      preview.textContent =
        pattern || "Enter a regex pattern above (e.g., (?i)(sport|news|live))";
      return;
    }

    if (this.rootGroup.conditions.length === 0) {
      preview.textContent = "Add conditions above to see the generated filter";
      return;
    }

    // Build filter data object in the format expected by generateFilterPreview
    const filterData = {
      conditions: this.rootGroup.conditions.map((condition) => ({
        field_name: condition.field,
        operator: condition.operator,
        value: condition.value,
      })),
      logical_operator: this.rootGroup.logical_operator,
    };

    // Use the generateFilterPreview method for consistent formatting and truncation
    preview.textContent = this.generateFilterPreview(filterData);
  }

  updateTestButton() {
    const testBtn = document.getElementById("testPatternBtn");
    const sourceSelect = document.getElementById("testSource");

    let hasValidFilter = false;

    if (this.filterType === "advanced") {
      const pattern = document.getElementById("filterPattern").value.trim();
      hasValidFilter = pattern.length > 0;
    } else {
      hasValidFilter =
        this.rootGroup.conditions.length > 0 &&
        this.rootGroup.conditions.every((c) => c.value.trim().length > 0);
    }

    testBtn.disabled = !hasValidFilter || !sourceSelect.value;
  }

  buildAdvancedFilter() {
    if (this.rootGroup.conditions.length === 0) {
      return null;
    }

    const filterConditions = this.rootGroup.conditions.map((condition) => ({
      field: condition.field,
      operator: condition.operator,
      value: condition.value,
    }));

    return {
      root_group: {
        conditions: filterConditions,
        groups: this.rootGroup.groups,
        logical_operator: this.rootGroup.logical_operator,
      },
    };
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
      "not_matches",
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
    if (this.filterType === "advanced") {
      return this.convertPatternToAdvancedFilter();
    } else {
      return {
        root_group: this.rootGroup,
      };
    }
  }

  loadAdvancedFilter(advancedFilter) {
    if (typeof advancedFilter === "string") {
      try {
        advancedFilter = JSON.parse(advancedFilter);
      } catch (e) {
        console.error("Failed to parse advanced filter JSON:", e);
        return;
      }
    }

    if (!advancedFilter || !advancedFilter.root_group) {
      // If no advanced filter, start with empty conditions
      this.rootGroup = {
        conditions: [],
        groups: [],
        logical_operator: "and",
      };
      this.conditionCounter = 0;
      return;
    }

    this.rootGroup = {
      conditions: [],
      groups: advancedFilter.root_group.groups || [],
      logical_operator: advancedFilter.root_group.logical_operator || "and",
    };
    this.conditionCounter = 0;

    const conditions = advancedFilter.root_group.conditions || [];
    conditions.forEach((condition) => {
      const conditionId = ++this.conditionCounter;
      this.rootGroup.conditions.push({
        id: conditionId,
        field: condition.field,
        operator: condition.operator,
        value: condition.value,
      });
    });

    this.renderConditions();
    this.updateFilterPreview();
  }

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

  // Load conditions from backend into the visual builder
  loadConditionsIntoBuilder(filter) {
    if (filter.conditions && filter.conditions.length > 0) {
      this.rootGroup.logical_operator = filter.logical_operator || "AND";
      this.rootGroup.conditions = filter.conditions.map((condition) => ({
        id: ++this.conditionCounter,
        field: condition.field_name,
        operator: condition.operator,
        value: condition.value,
      }));
    }
  }

  // Populate advanced tab from conditions array
  populateAdvancedTabFromConditions(filter) {
    if (filter.conditions && filter.conditions.length > 0) {
      const operator = filter.logical_operator === "OR" ? " OR " : " AND ";
      const textPattern = filter.conditions
        .map((condition) => {
          const field = this.availableFields.find(
            (f) => f.name === condition.field_name,
          );
          const fieldName = field ? field.name : condition.field_name;
          return `${fieldName} ${condition.operator.replace("_", " ")} "${condition.value}"`;
        })
        .join(operator);
      document.getElementById("filterPattern").value = textPattern;
      this.validatePattern();
    } else {
      document.getElementById("filterPattern").value = "";
    }
  }

  setLogicalOperator(operator) {
    this.rootGroup.logical_operator = operator;
    this.renderConditions();
    this.updateFilterPreview();
    this.updateTestButton();

    // Update advanced tab when visual conditions change
    this.updateAdvancedTab();
  }

  // Update the advanced tab with current visual conditions
  updateAdvancedTab() {
    if (this.filterType === "visual") {
      const advancedFilter = this.buildAdvancedFilter();
      if (advancedFilter) {
        const textPattern = this.convertAdvancedFilterToText(advancedFilter);
        document.getElementById("filterPattern").value = textPattern;
        this.validatePattern();
      }
    }
  }

  // Sync changes from advanced tab back to visual tab
  syncAdvancedToVisual() {
    if (this.filterType === "advanced") {
      const pattern = document.getElementById("filterPattern").value.trim();
      const parsedFilter = this.parseTextPattern(pattern);
      if (parsedFilter) {
        // Update the visual representation
        this.rootGroup = parsedFilter.root_group;
        this.renderConditions();
        this.updateFilterPreview();
      }
    }
  }

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

    // Basic structure validation using regex
    const conditionPattern =
      /^\s*\w+\s+\w+(?:_\w+)*\s+["'][^"']*["']\s*(?:(?:AND|OR)\s+\w+\s+\w+(?:_\w+)*\s+["'][^"']*["']\s*)*$/i;

    if (!conditionPattern.test(pattern)) {
      result.valid = false;
      result.messages.push(
        'Invalid syntax structure. Expected: field operator "value" [AND|OR field operator "value"]',
      );
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

    // Extract field names from pattern
    const fieldMatches = pattern.match(/\b(\w+)\s+\w+(?:_\w+)*\s+["']/g);
    const foundFields = new Set();
    const invalidFields = [];

    if (fieldMatches) {
      fieldMatches.forEach((match) => {
        const field = match.split(/\s+/)[0];
        foundFields.add(field);
        if (!validFields.includes(field)) {
          invalidFields.push(field);
        }
      });
    }

    if (invalidFields.length > 0) {
      result.valid = false;
      result.messages.push(`Invalid fields: ${invalidFields.join(", ")}`);
      result.messages.push(`Valid fields: ${validFields.join(", ")}`);
    } else if (foundFields.size > 0) {
      result.messages.push(
        `Using fields: ${Array.from(foundFields).join(", ")}`,
      );
    }

    return result;
  }

  // Validate operators
  validateOperators(pattern) {
    const result = { valid: true, messages: [] };
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

    // Extract operators from pattern
    const operatorMatches = pattern.match(/\w+\s+(\w+(?:_\w+)*)\s+["']/g);
    const foundOperators = new Set();
    const invalidOperators = [];

    if (operatorMatches) {
      operatorMatches.forEach((match) => {
        const operator = match.split(/\s+/)[1];
        foundOperators.add(operator);
        if (!validOperators.includes(operator)) {
          invalidOperators.push(operator);
        }
      });
    }

    if (invalidOperators.length > 0) {
      result.valid = false;
      result.messages.push(`Invalid operators: ${invalidOperators.join(", ")}`);
      result.messages.push(`Valid operators: ${validOperators.join(", ")}`);
    } else if (foundOperators.size > 0) {
      result.messages.push(
        `Using operators: ${Array.from(foundOperators).join(", ")}`,
      );
    }

    return result;
  }

  // Validate regex patterns in values
  validateRegexPatterns(pattern) {
    const result = { valid: true, messages: [] };

    // Find all conditions with 'matches' operator
    const matchesConditions = pattern.match(
      /\w+\s+(?:not_)?matches\s+["']([^"']+)["']/gi,
    );
    const invalidRegex = [];
    let regexCount = 0;

    if (matchesConditions) {
      matchesConditions.forEach((match) => {
        const regexMatch = match.match(/["']([^"']+)["']/);
        if (regexMatch) {
          const regexPattern = regexMatch[1];
          regexCount++;
          try {
            new RegExp(regexPattern);
          } catch (error) {
            invalidRegex.push(`"${regexPattern}": ${error.message}`);
          }
        }
      });
    }

    if (invalidRegex.length > 0) {
      result.valid = false;
      result.messages.push("Invalid regex patterns:");
      result.messages = result.messages.concat(invalidRegex);
    } else if (regexCount > 0) {
      result.messages.push(
        `${regexCount} regex pattern(s) validated successfully`,
      );
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
    }

    // Update validation icons
    this.updateValidationIcon(syntaxIcon, validation.syntax, "Syntax");
    this.updateValidationIcon(fieldsIcon, validation.fields, "Fields");
    this.updateValidationIcon(operatorsIcon, validation.operators, "Operators");
    this.updateValidationIcon(regexIcon, validation.regex, "Regex");

    // Update messages
    if (!messagesContainer) {
      console.warn("validationMessages container not found, skipping message updates");
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
}

// Initialize the filters manager when the page loads
let filtersManager;

async function initializeFiltersManager() {
  console.log("Initializing FiltersManager..."); // Debug log
  filtersManager = new FiltersManager();
  await filtersManager.init();
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
