// Data Mapping Management JavaScript

let currentRules = [];
let editingRule = null;
let currentLogoAction = null;
let selectedLogoId = null;
let conditionCounter = 0;
let actionCounter = 0;

// Available field options
const FIELD_OPTIONS = [
  { value: "channel_name", label: "Channel Name" },
  { value: "tvg_id", label: "TVG ID" },
  { value: "tvg_name", label: "TVG Name" },
  { value: "tvg_logo", label: "TVG Logo" },
  { value: "group_title", label: "Group Title" },
  { value: "stream_url", label: "Stream URL" },
];

// Available operators
const OPERATOR_OPTIONS = [
  { value: "equals", label: "Equals" },
  { value: "contains", label: "Contains" },
  { value: "starts_with", label: "Starts With" },
  { value: "ends_with", label: "Ends With" },
  { value: "matches", label: "Regex Match" },
  { value: "not_equals", label: "Not Equals" },
  { value: "not_contains", label: "Does Not Contain" },
  { value: "not_matches", label: "Does Not Match Regex" },
];

// Available action types
const ACTION_TYPES = [
  { value: "set_value", label: "Set Value" },
  { value: "set_default_if_empty", label: "Set Default If Empty" },
  { value: "set_logo", label: "Set Logo" },
  { value: "set_label", label: "Set Label" },
  { value: "transform_value", label: "Transform Value" },
];

// Initialize page
function initializeDataMappingPage() {
  console.log("Initializing data mapping page..."); // Debug log
  loadRules();

  // Setup standard modal close handlers
  SharedUtils.setupStandardModalCloseHandlers("ruleModal");
  SharedUtils.setupStandardModalCloseHandlers("logoPickerModal");
  SharedUtils.setupStandardModalCloseHandlers("sourceModal");
}

// Check if DOM is already loaded, if so initialize immediately
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", initializeDataMappingPage);
} else {
  // DOM is already loaded, initialize immediately
  initializeDataMappingPage();
}

// Load all data mapping rules
async function loadRules() {
  try {
    const response = await fetch("/api/data-mapping?" + new Date().getTime());
    console.log("Data mapping API response status:", response.status); // Debug log
    if (!response.ok) throw new Error("Failed to load rules");

    const data = await response.json();
    console.log("Data mapping API response:", data); // Debug log
    console.log(
      "Data mapping data type:",
      typeof data,
      "Array?",
      Array.isArray(data),
    ); // Debug log
    currentRules = Array.isArray(data) ? data : [];
    console.log("Current rules count:", currentRules.length); // Debug log
    renderRules();
  } catch (error) {
    console.error("Error loading rules:", error);
    // Ensure currentRules is still a valid array even if API fails
    currentRules = [];
    renderRules(); // Render empty state
    showError("Failed to load data mapping rules");
  }
}

// Render rules list
function renderRules() {
  const container = document.getElementById("rulesContainer");

  if (currentRules.length === 0) {
    container.innerHTML = `
            <div class="empty-state">
                <h3>No Data Mapping Rules</h3>
                <p>Create your first rule to start transforming channel data</p>
                <button class="btn btn-primary" onclick="createRule()">
                    ➕ Add Your First Rule
                </button>
            </div>
        `;
    return;
  }

  let html = '<div id="sortableRules">';

  currentRules.forEach((rule, index) => {
    if (!rule || typeof rule !== "object" || !rule.id) return; // Skip if rule is null/undefined, not an object, or has no id

    // Provide defaults for missing properties
    const isActive = rule.is_active !== undefined ? rule.is_active : true;
    const statusClass = isActive ? "active" : "inactive";
    const statusText = isActive ? "Active" : "Inactive";
    const ruleName = rule.name || "Unnamed Rule";
    const ruleDescription = rule.description || "";
    const conditions = rule.conditions || [];
    const actions = rule.actions || [];

    html += `
            <div class="rule-card" data-rule-id="${rule.id}" draggable="true" style="cursor: move; margin-bottom: 8px; border: 1px solid #ddd; border-radius: 5px; padding: 12px; background: white;">
                <div class="rule-header" style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 6px;">
                    <div style="display: flex; align-items: center; gap: 10px;">
                        <span class="drag-handle" style="cursor: move; color: #999; font-size: 16px; user-select: none; background: #f8f9fa; padding: 2px 4px; border-radius: 3px;">⋮</span>
                        <strong class="rule-title">${escapeHtml(ruleName)}</strong>
                        <span class="status-badge badge ${isActive ? "badge-success" : "badge-secondary"}" style="font-size: 12px;">${statusText}</span>
                    </div>
                    <div class="rule-buttons" style="display: flex; gap: 5px;">
                        <button class="btn btn-sm btn-primary" onclick="editRule('${rule.id}')">
                            ✏️ Edit
                        </button>
                        <button class="btn btn-sm ${isActive ? "btn-warning" : "btn-success"}" onclick="toggleRule('${rule.id}')">
                            ${isActive ? "⏸️ Disable" : "▶️ Enable"}
                        </button>
                        <button class="btn btn-sm btn-danger" onclick="deleteRule('${rule.id}')">
                            🗑️ Delete
                        </button>
                    </div>
                </div>
                ${ruleDescription ? `<div style="margin-bottom: 8px; font-style: italic; color: #666; font-size: 13px;">${escapeHtml(ruleDescription)}</div>` : ""}
                <div class="rule-content" style="display: grid; grid-template-columns: 1fr 1fr; gap: 15px; margin-top: 5px;">
                    <div class="conditions-column">
                        <h6 style="margin: 0 0 5px 0; color: #333; font-weight: bold; font-size: 14px;">Conditions (${conditions.length})</h6>
                        ${renderConditionsSummary(conditions)}
                    </div>
                    <div class="actions-column">
                        <h6 style="margin: 0 0 5px 0; color: #333; font-weight: bold; font-size: 14px;">Actions (${actions.length})</h6>
                        ${renderActionsSummary(actions)}
                    </div>
                </div>
            </div>
        `;
  });

  html += "</div>";
  container.innerHTML = html;

  // Initialize drag and drop
  initializeSortable();
}

// Render conditions summary
function renderConditionsSummary(conditions) {
  if (conditions.length === 0) {
    return '<div style="color: #999; font-style: italic; font-size: 13px;">Always applies</div>';
  }

  let html = '<div style="font-size: 13px; line-height: 1.4;">';
  conditions.forEach((condition, index) => {
    if (index > 0 && condition.logical_operator) {
      html += `<div style="margin: 3px 0; font-weight: bold; color: #666;">${condition.logical_operator.toUpperCase()}</div>`;
    }

    const field =
      FIELD_OPTIONS.find((f) => f.value === condition.field_name)?.label ||
      condition.field_name;
    const operator =
      OPERATOR_OPTIONS.find((o) => o.value === condition.operator)?.label ||
      condition.operator;

    html += `<div style="margin: 2px 0; padding: 2px 6px; background: #f8f9fa; border-radius: 3px; border-left: 3px solid #007bff;">
            <strong>${field}</strong> ${operator}<br>
            <code style="font-size: 11px; color: #d63384;">"${escapeHtml(condition.value)}"</code>
        </div>`;
  });
  html += "</div>";

  return html;
}

// Render actions summary
function renderActionsSummary(actions) {
  if (actions.length === 0) {
    return '<div style="color: #999; font-style: italic; font-size: 13px;">No actions</div>';
  }

  let html = '<div style="font-size: 13px; line-height: 1.4;">';
  actions.forEach((action) => {
    const actionType =
      ACTION_TYPES.find((a) => a.value === action.action_type)?.label ||
      action.action_type;
    const field =
      FIELD_OPTIONS.find((f) => f.value === action.target_field)?.label ||
      action.target_field;

    let valueDisplay = "";
    if (action.value) {
      valueDisplay = `<br><code style="font-size: 11px; color: #198754;">"${escapeHtml(action.value)}"</code>`;
    } else if (action.logo_asset_id) {
      valueDisplay =
        '<br><span style="font-size: 11px; color: #6f42c1;">🖼️ Custom Logo</span>';
    } else if (action.label_key && action.label_value) {
      valueDisplay = `<br><code style="font-size: 11px; color: #fd7e14; background: #fff3cd; padding: 1px 3px; border-radius: 2px;">${escapeHtml(action.label_key)}=${escapeHtml(action.label_value)}</code>`;
    }

    html += `<div style="margin: 2px 0; padding: 2px 6px; background: #f8f9fa; border-radius: 3px; border-left: 3px solid #198754;">
            <strong>${actionType}</strong> → ${field}${valueDisplay}
        </div>`;
  });
  html += "</div>";

  return html;
}

// Initialize sortable drag and drop
function initializeSortable() {
  const container = document.getElementById("sortableRules");
  if (!container) return;

  let draggedElement = null;

  // Add event listeners to all rule cards
  const ruleCards = container.querySelectorAll(".rule-card");
  ruleCards.forEach((card) => {
    card.addEventListener("dragstart", handleDragStart);
    card.addEventListener("dragover", handleDragOver);
    card.addEventListener("drop", handleDrop);
    card.addEventListener("dragend", handleDragEnd);

    // Prevent text selection when dragging
    card.addEventListener("selectstart", (e) => {
      if (e.target.closest(".drag-handle")) {
        e.preventDefault();
      }
    });
  });

  function handleDragStart(e) {
    draggedElement = this;
    this.style.opacity = "0.5";
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/html", this.outerHTML);
  }

  function handleDragOver(e) {
    if (e.preventDefault) {
      e.preventDefault();
    }
    e.dataTransfer.dropEffect = "move";
    return false;
  }

  function handleDrop(e) {
    if (e.stopPropagation) {
      e.stopPropagation();
    }

    if (draggedElement !== this) {
      // Get the dragged element's position
      const draggedId = draggedElement.dataset.ruleId;
      const targetId = this.dataset.ruleId;

      // Move in DOM
      const parent = this.parentNode;
      const targetIndex = Array.from(parent.children).indexOf(this);
      const draggedIndex = Array.from(parent.children).indexOf(draggedElement);

      if (draggedIndex < targetIndex) {
        parent.insertBefore(draggedElement, this.nextSibling);
      } else {
        parent.insertBefore(draggedElement, this);
      }

      // Save new order
      saveRuleOrder();
    }

    return false;
  }

  function handleDragEnd(e) {
    this.style.opacity = "1";
    draggedElement = null;
  }
}

// Save the new rule order
async function saveRuleOrder() {
  const container = document.getElementById("sortableRules");
  const ruleCards = container.querySelectorAll(".rule-card");

  // Create array of [rule_id, priority] pairs
  const ruleOrder = Array.from(ruleCards).map((card, index) => [
    card.dataset.ruleId,
    index + 1, // Priority starts at 1
  ]);

  console.log("Saving rule order:", ruleOrder);

  try {
    const response = await fetch("/api/data-mapping/reorder", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(ruleOrder),
    });

    if (response.ok) {
      showSuccess("Rule order updated successfully");
    } else {
      const errorText = await response.text();
      console.error("Failed to save rule order:", response.status, errorText);
      showError("Failed to save rule order: " + errorText);
    }
  } catch (error) {
    console.error("Error saving rule order:", error);
    showError("Failed to save rule order: " + error.message);
  }
}

// Create new rule
function createRule() {
  editingRule = null;
  resetRuleForm();
  document.getElementById("ruleModalTitle").textContent =
    "Create Data Mapping Rule";
  loadSourcesForTesting();
  SharedUtils.showStandardModal("ruleModal");
}

// Edit existing rule
function editRule(ruleId) {
  const rule = currentRules.find((r) => r.id === ruleId);
  if (!rule) return;

  editingRule = rule;
  populateRuleForm(rule);
  document.getElementById("ruleModalTitle").textContent =
    "Edit Data Mapping Rule";
  loadSourcesForTesting();
  SharedUtils.showStandardModal("ruleModal");
}

// Reset rule form
function resetRuleForm() {
  document.getElementById("ruleForm").reset();
  document.getElementById("ruleActive").checked = true;
  document.getElementById("conditionsContainer").innerHTML = "";
  document.getElementById("actionsContainer").innerHTML = "";
  document.getElementById("testResults").style.display = "none";
  document.getElementById("runTestBtn").disabled = true;
  conditionCounter = 0;
  actionCounter = 0;

  // Add default condition and action
  addCondition();
  addAction();
}

// Populate form with rule data
function populateRuleForm(ruleData) {
  console.log("Populating form with rule data:", ruleData); // Debug log

  // The API returns a flat structure, not nested
  const nameField = document.getElementById("ruleName");
  const descField = document.getElementById("ruleDescription");
  const activeField = document.getElementById("ruleActive");

  if (nameField) nameField.value = ruleData.name || "";
  if (descField) descField.value = ruleData.description || "";
  if (activeField) activeField.checked = ruleData.is_active;

  console.log("Populated form fields:", {
    name: ruleData.name,
    description: ruleData.description,
    isActive: ruleData.is_active,
    nameFieldValue: nameField?.value,
    descFieldValue: descField?.value,
    activeFieldChecked: activeField?.checked,
  });

  // Clear containers
  document.getElementById("conditionsContainer").innerHTML = "";
  document.getElementById("actionsContainer").innerHTML = "";
  conditionCounter = 0;
  actionCounter = 0;

  // Populate conditions
  const conditions = ruleData.conditions || [];
  if (conditions.length === 0) {
    addCondition();
  } else {
    conditions.forEach((condition) => {
      addCondition(condition);
    });
  }

  // Populate actions
  const actions = ruleData.actions || [];
  console.log("Populating actions:", actions); // Debug log
  if (actions.length === 0) {
    addAction();
  } else {
    actions.forEach((action, index) => {
      console.log(`Adding action ${index}:`, action); // Debug log
      addAction(action);
    });
  }
}

// Add condition row
function addCondition(conditionData = null) {
  const container = document.getElementById("conditionsContainer");
  const conditionId = ++conditionCounter;

  const showLogicalOperator = container.children.length > 0;

  const html = `
        <div class="condition-row" data-condition-id="${conditionId}">
            ${
              showLogicalOperator
                ? `
                <select name="logical_operator_${conditionId}" class="logical-operator-select">
                    <option value="and" ${conditionData?.logical_operator === "and" ? "selected" : ""}>AND</option>
                    <option value="or" ${conditionData?.logical_operator === "or" ? "selected" : ""}>OR</option>
                </select>
            `
                : ""
            }
            <select name="field_${conditionId}" required>
                <option value="">Select Field</option>
                ${FIELD_OPTIONS.map(
                  (option) =>
                    `<option value="${option.value}" ${conditionData?.field_name === option.value ? "selected" : ""}>${option.label}</option>`,
                ).join("")}
            </select>
            <select name="operator_${conditionId}" required>
                <option value="">Select Operator</option>
                ${OPERATOR_OPTIONS.map(
                  (option) =>
                    `<option value="${option.value}" ${conditionData?.operator === option.value ? "selected" : ""}>${option.label}</option>`,
                ).join("")}
            </select>
            <input type="text" name="condition_value_${conditionId}" placeholder="Value" required
                   value="${conditionData?.value || ""}">
            <button type="button" class="btn btn-sm btn-outline-danger" onclick="removeCondition(${conditionId})">
                ✖️
            </button>
        </div>
    `;

  container.insertAdjacentHTML("beforeend", html);
}

// Remove condition
function removeCondition(conditionId) {
  const element = document.querySelector(
    `[data-condition-id="${conditionId}"]`,
  );
  if (element) {
    element.remove();

    // If this was the first condition, remove logical operator from new first condition
    const firstCondition = document.querySelector(".condition-row");
    if (firstCondition) {
      const logicalSelect = firstCondition.querySelector(
        ".logical-operator-select",
      );
      if (logicalSelect) {
        logicalSelect.remove();
      }
    }
  }
}

// Add action row
function addAction(actionData = null) {
  const container = document.getElementById("actionsContainer");
  const actionId = ++actionCounter;

  const html = `
        <div class="action-row" data-action-id="${actionId}">
            <select name="action_type_${actionId}" required onchange="updateActionType(${actionId})">
                <option value="">Select Action Type</option>
                ${ACTION_TYPES.map(
                  (option) =>
                    `<option value="${option.value}" ${actionData?.action_type === option.value ? "selected" : ""}>${option.label}</option>`,
                ).join("")}
            </select>
            <select name="target_field_${actionId}" required>
                <option value="">Target Field</option>
                ${FIELD_OPTIONS.map(
                  (option) =>
                    `<option value="${option.value}" ${actionData?.target_field === option.value ? "selected" : ""}>${option.label}</option>`,
                ).join("")}
            </select>
            <div class="action-value-container" id="actionValue_${actionId}">
                ${renderActionValueInput(actionId, actionData)}
            </div>
            <button type="button" class="btn btn-sm btn-outline-danger" onclick="removeAction(${actionId})">
                ✖️
            </button>
        </div>
    `;

  container.insertAdjacentHTML("beforeend", html);

  // Don't call updateActionType here as it will overwrite the data
  // The renderActionValueInput already handles the existing data
}

// Render action value input based on action type
function renderActionValueInput(actionId, actionData = null) {
  const actionType = actionData?.action_type;

  if (actionType === "set_logo") {
    const logoId = actionData?.logo_asset_id;
    return `
            <div class="logo-selector">
                <input type="hidden" name="logo_asset_id_${actionId}" value="${logoId || ""}">
                <button type="button" class="btn btn-outline-secondary" onclick="openLogoPicker(${actionId})">
                    ${logoId ? "🖼️ Change Logo" : "🖼️ Choose Logo"}
                </button>
                ${logoId ? `<img class="logo-preview-small ml-2" src="/api/logos/${logoId}" alt="Selected logo">` : ""}
            </div>
        `;
  } else if (actionType === "set_label") {
    const labelKey = actionData?.label_key || "";
    const labelValue = actionData?.label_value || "";
    return `
            <div class="label-inputs" style="display: flex; gap: 5px; align-items: center;">
                <input type="text" name="label_key_${actionId}" placeholder="Label Key"
                       value="${labelKey}" style="flex: 0 0 120px; font-weight: bold;" required>
                <span style="color: #666;">=</span>
                <input type="text" name="label_value_${actionId}" placeholder="Label Value"
                       value="${labelValue}" style="flex: 1;" required>
                <small style="color: #666; font-size: 11px; margin-left: 5px;">e.g. category=sports</small>
            </div>
        `;
  } else {
    return `
            <input type="text" name="action_value_${actionId}" placeholder="Value"
                   value="${actionData?.value || ""}" ${actionType === "set_logo" ? 'style="display:none"' : ""} />
        `;
  }
}

// Update action type and regenerate value input
function updateActionType(actionId) {
  const select = document.querySelector(`[name="action_type_${actionId}"]`);
  const container = document.getElementById(`actionValue_${actionId}`);

  if (select && container) {
    container.innerHTML = renderActionValueInput(actionId, {
      action_type: select.value,
    });
  }
}

// Remove action
function removeAction(actionId) {
  const element = document.querySelector(`[data-action-id="${actionId}"]`);
  if (element) {
    element.remove();
  }
}

// Open logo picker
function openLogoPicker(actionId) {
  currentLogoAction = actionId;
  selectedLogoId = null;

  // Load logos
  loadLogosForPicker();
  SharedUtils.showStandardModal("logoPickerModal");
}

// Load logos for picker
async function loadLogosForPicker() {
  try {
    const response = await fetch("/api/logos?limit=50");
    if (!response.ok) throw new Error("Failed to load logos");

    const data = await response.json();
    renderLogoPickerGrid(data.assets);
  } catch (error) {
    console.error("Error loading logos:", error);
    document.getElementById("logoPickerGrid").innerHTML =
      '<div class="empty-state"><p>Failed to load logos</p></div>';
  }
}

// Render logo picker grid
function renderLogoPickerGrid(logos) {
  const grid = document.getElementById("logoPickerGrid");

  if (logos.length === 0) {
    grid.innerHTML = `
            <div class="empty-state col-12">
                <h4>No Logos Available</h4>
                <p>Upload logos first to use them in mapping rules</p>
                <a href="/logos" class="btn btn-primary">Manage Logos</a>
            </div>
        `;
    return;
  }

  let html = "";
  logos.forEach((logoWithUrl) => {
    html += `
            <div class="logo-card" onclick="selectLogoForPicker('${logoWithUrl.id}')" data-logo-id="${logoWithUrl.id}">
                <img class="logo-preview" src="${logoWithUrl.url}" alt="${escapeHtml(logoWithUrl.name)}"
                     onerror="this.src='data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMTAwIiBoZWlnaHQ9IjEwMCIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj48cmVjdCB3aWR0aD0iMTAwIiBoZWlnaHQ9IjEwMCIgZmlsbD0iI2YwZjBmMCIvPjx0ZXh0IHg9IjUwIiB5PSI1NSIgZm9udC1mYW1pbHk9IkFyaWFsIiBmb250LXNpemU9IjE0IiBmaWxsPSIjOTk5IiB0ZXh0LWFuY2hvcj0ibWlkZGxlIj5ObyBJbWFnZTwvdGV4dD48L3N2Zz4='">
                <div class="logo-info">
                    <div class="logo-name">${escapeHtml(logoWithUrl.name)}</div>
                    <div class="logo-meta">${formatFileSize(logoWithUrl.file_size)}</div>
                </div>
            </div>
        `;
  });

  grid.innerHTML = html;
}

// Select logo in picker
function selectLogoForPicker(logoId) {
  // Remove previous selection
  document.querySelectorAll(".logo-card").forEach((card) => {
    card.classList.remove("selected");
  });

  // Add selection to clicked card
  const card = document.querySelector(`[data-logo-id="${logoId}"]`);
  if (card) {
    card.classList.add("selected");
    selectedLogoId = logoId;
  }
}

// Confirm logo selection
function selectLogo() {
  if (selectedLogoId && currentLogoAction) {
    const input = document.querySelector(
      `[name="logo_asset_id_${currentLogoAction}"]`,
    );
    if (input) {
      input.value = selectedLogoId;

      // Update the action value container
      const container = document.getElementById(
        `actionValue_${currentLogoAction}`,
      );
      container.innerHTML = renderActionValueInput(currentLogoAction, {
        action_type: "set_logo",
        logo_asset_id: selectedLogoId,
      });
    }

    closeLogoPicker();
  }
}

// Close logo picker
function closeLogoPicker() {
  SharedUtils.hideStandardModal("logoPickerModal");
  currentLogoAction = null;
  selectedLogoId = null;
}

// Search logos in picker
function searchLogos() {
  const query = document.getElementById("logoSearch").value;
  // Implement logo search functionality
  // For now, just reload all logos
  loadLogosForPicker();
}

// Test rule
async function testRule() {
  // First, we need to select a source
  showSourceSelector();
}

// Load sources for testing in rule modal
async function loadSourcesForTesting() {
  try {
    const response = await fetch("/api/sources");
    if (!response.ok) throw new Error("Failed to load sources");

    const sources = await response.json();
    populateTestSourceSelect(sources);
  } catch (error) {
    console.error("Error loading sources:", error);
    showError("Failed to load sources for testing");
  }
}

// Populate test source selector
function populateTestSourceSelect(sources) {
  const select = document.getElementById("testSourceSelect");

  // Clear existing options except the first one
  select.innerHTML =
    '<option value="">Select a source to test against...</option>';

  sources.forEach((source) => {
    const option = document.createElement("option");
    option.value = source.id;
    option.textContent = `${source.name} (${source.channel_count} channels)`;
    select.appendChild(option);
  });

  // Enable test button when source is selected
  select.addEventListener("change", () => {
    document.getElementById("runTestBtn").disabled = !select.value;
  });
}

// Run rule test
async function runRuleTest() {
  const sourceId = document.getElementById("testSourceSelect").value;
  if (!sourceId) {
    showError("Please select a source to test");
    return;
  }

  const testBtn = document.getElementById("runTestBtn");
  const originalText = testBtn.textContent;
  testBtn.textContent = "🔄 Testing...";
  testBtn.disabled = true;

  try {
    await testRuleWithSource(sourceId);
  } catch (error) {
    console.error("Test failed:", error);
    showError("Failed to run test");
  } finally {
    testBtn.textContent = originalText;
    testBtn.disabled = false;
  }
}

// Show source selector for testing (legacy function for backward compatibility)
async function showSourceSelector() {
  try {
    const response = await fetch("/api/sources");
    if (!response.ok) throw new Error("Failed to load sources");

    const sources = await response.json();
    renderSourceSelector(sources);
    SharedUtils.showStandardModal("sourceModal");
  } catch (error) {
    console.error("Error loading sources:", error);
    showError("Failed to load sources for testing");
  }
}

// Render source selector
function renderSourceSelector(sources) {
  const container = document.getElementById("sourcesList");

  if (sources.length === 0) {
    container.innerHTML = `
            <div class="empty-state">
                <p>No sources available for testing</p>
                <a href="/sources" class="btn btn-primary">Add Sources</a>
            </div>
        `;
    return;
  }

  let html = '<div class="list-group">';
  sources.forEach((source) => {
    html += `
            <button class="list-group-item list-group-item-action"
                    onclick="testRuleWithSource('${source.id}')">
                <strong>${escapeHtml(source.name)}</strong>
                <br>
                <small class="text-muted">${source.channel_count} channels</small>
            </button>
        `;
  });
  html += "</div>";

  container.innerHTML = html;
}

// Test rule with selected source
async function testRuleWithSource(sourceId) {
  // Collect form data
  const formData = collectRuleFormData();
  if (!formData) return;

  try {
    const testData = {
      source_id: sourceId,
      conditions: formData.conditions,
      actions: formData.actions,
    };

    const response = await fetch("/api/data-mapping/test", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(testData),
    });

    if (!response.ok) throw new Error("Test failed");

    const result = await response.json();
    displayTestResults(result);
  } catch (error) {
    console.error("Error testing rule:", error);
    showError("Failed to test rule");
  }
}

// Display test results
function displayTestResults(result) {
  const container = document.getElementById("testResultsContent");

  if (!result.is_valid) {
    container.innerHTML = `
            <div class="alert alert-danger">
                <strong>Test Failed:</strong> ${escapeHtml(result.error || "Unknown error")}
            </div>
        `;
  } else if (result.matched_count === 0) {
    container.innerHTML = `
            <div class="alert alert-warning">
                <strong>No Matches:</strong> No channels matched the specified conditions.
            </div>
        `;
  } else {
    let html = `
            <div class="alert alert-success">
                <strong>Test Successful:</strong> ${result.matched_count} channels matched
            </div>
            <h6>Sample Results (showing first 5):</h6>
        `;

    const sampleChannels = result.matching_channels.slice(0, 5);
    sampleChannels.forEach((channel) => {
      html += `
                <div class="test-channel">
                    <div class="test-channel-name">${escapeHtml(channel.channel_name)}</div>
                    <div class="test-changes">
                        ${renderTestChanges(channel.original_values, channel.mapped_values)}
                    </div>
                </div>
            `;
    });

    container.innerHTML = html;
  }

  document.getElementById("testResults").style.display = "block";
}

// Render test changes
function renderTestChanges(original, mapped) {
  const changes = [];

  Object.keys(mapped).forEach((field) => {
    const originalValue = original[field];
    const mappedValue = mapped[field];

    if (originalValue !== mappedValue) {
      const fieldLabel =
        FIELD_OPTIONS.find((f) => f.value === field)?.label || field;
      changes.push(
        `${fieldLabel}: "${originalValue || ""}" → "${mappedValue || ""}"`,
      );
    }
  });

  return changes.length > 0 ? changes.join("<br>") : "No changes";
}

// Close source modal
function closeSourceModal() {
  SharedUtils.hideStandardModal("sourceModal");
}

// Collect form data
function collectRuleFormData() {
  console.log("Collecting rule form data..."); // Debug log
  const form = document.getElementById("ruleForm");
  if (!form) {
    console.error("Rule form not found");
    return null;
  }

  const formData = new FormData(form);

  // Get basic form fields by direct element access for reliability
  const nameElement = document.getElementById("ruleName");
  const descriptionElement = document.getElementById("ruleDescription");
  const activeElement = document.getElementById("ruleActive");

  const name = nameElement ? nameElement.value.trim() : "";
  const description = descriptionElement ? descriptionElement.value.trim() : "";
  const isActive = activeElement ? activeElement.checked : true;

  console.log("Form elements found:", {
    nameElement: !!nameElement,
    descriptionElement: !!descriptionElement,
    activeElement: !!activeElement,
  });
  console.log("Raw element values:", {
    nameValue: nameElement?.value,
    descriptionValue: descriptionElement?.value,
    activeChecked: activeElement?.checked,
  });
  console.log("Form data collected:", { name, description, isActive }); // Debug log

  if (!name) {
    showError("Rule name is required");
    return null;
  }

  // Collect conditions
  const conditions = [];
  const conditionRows = document.querySelectorAll(".condition-row");

  conditionRows.forEach((row, index) => {
    const conditionId = row.dataset.conditionId;
    const field = formData.get(`field_${conditionId}`);
    const operator = formData.get(`operator_${conditionId}`);
    const value = formData.get(`condition_value_${conditionId}`);
    const logicalOperator =
      index > 0 ? formData.get(`logical_operator_${conditionId}`) : null;

    console.log(`Condition ${index}:`, {
      conditionId,
      field,
      operator,
      value,
      logicalOperator,
    }); // Debug log

    if (field && operator && value) {
      conditions.push({
        field_name: field,
        operator: operator,
        value: value,
        logical_operator: logicalOperator,
      });
    }
  });

  // Collect actions
  const actions = [];
  const actionRows = document.querySelectorAll(".action-row");
  console.log("Found action rows:", actionRows.length); // Debug log

  actionRows.forEach((row, index) => {
    const actionId = row.dataset.actionId;
    const actionType = formData.get(`action_type_${actionId}`);
    const targetField = formData.get(`target_field_${actionId}`);
    const value = formData.get(`action_value_${actionId}`);
    const logoAssetId = formData.get(`logo_asset_id_${actionId}`);
    const labelKey = formData.get(`label_key_${actionId}`);
    const labelValue = formData.get(`label_value_${actionId}`);

    console.log(`Action ${index}:`, {
      actionId,
      actionType,
      targetField,
      value,
      logoAssetId,
      labelKey,
      labelValue,
    }); // Debug log

    if (actionType && targetField) {
      const action = {
        action_type: actionType,
        target_field: targetField,
        value:
          actionType === "set_logo" || actionType === "set_label"
            ? null
            : value,
        logo_asset_id: actionType === "set_logo" ? logoAssetId : null,
        label_key: actionType === "set_label" ? labelKey : null,
        label_value: actionType === "set_label" ? labelValue : null,
      };
      actions.push(action);
    }
  });

  return {
    name,
    description,
    is_active: isActive,
    conditions,
    actions,
  };
}

// Save rule
async function saveRule() {
  const formData = collectRuleFormData();
  if (!formData) return;

  try {
    const url = editingRule
      ? `/api/data-mapping/${editingRule.id}`
      : "/api/data-mapping";
    const method = editingRule ? "PUT" : "POST";

    const response = await fetch(url, {
      method,
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(formData),
    });

    if (!response.ok) throw new Error("Failed to save rule");

    showSuccess(
      editingRule ? "Rule updated successfully" : "Rule created successfully",
    );
    closeRuleModal();
    loadRules();
  } catch (error) {
    console.error("Error saving rule:", error);
    showError("Failed to save rule");
  }
}

// Close rule modal
function closeRuleModal() {
  SharedUtils.hideStandardModal("ruleModal");
  editingRule = null;
}

// Delete rule
async function deleteRule(ruleId) {
  if (!confirm("Are you sure you want to delete this rule?")) return;

  try {
    const response = await fetch(`/api/data-mapping/${ruleId}`, {
      method: "DELETE",
    });

    if (!response.ok) throw new Error("Failed to delete rule");

    showSuccess("Rule deleted successfully");
    loadRules();
  } catch (error) {
    console.error("Error deleting rule:", error);
    showError("Failed to delete rule");
  }
}

// Toggle rule active/inactive
async function toggleRule(ruleId) {
  const rule = currentRules.find((r) => r.id === ruleId);
  if (!rule) return;

  const updatedRule = {
    name: rule.name,
    description: rule.description,
    conditions: rule.conditions.map((c) => ({
      field_name: c.field_name,
      operator: c.operator,
      value: c.value,
      logical_operator: c.logical_operator,
    })),
    actions: rule.actions.map((a) => ({
      action_type: a.action_type,
      target_field: a.target_field,
      value: a.value,
      logo_asset_id: a.logo_asset_id,
    })),
    is_active: !rule.is_active,
  };

  try {
    const response = await fetch(`/api/data-mapping/${ruleId}`, {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(updatedRule),
    });

    if (!response.ok) throw new Error("Failed to toggle rule");

    showSuccess(
      `Rule ${updatedRule.is_active ? "enabled" : "disabled"} successfully`,
    );
    loadRules();
  } catch (error) {
    console.error("Error toggling rule:", error);
    showError("Failed to toggle rule");
  }
}

// Utility functions
function escapeHtml(text) {
  return SharedUtils.escapeHtml(text);
}

function formatFileSize(bytes) {
  return SharedUtils.formatFileSize(bytes);
}

function showError(message) {
  if (window.SharedUtils) {
    SharedUtils.showError(message);
  } else {
    console.error(message);
  }
}

function showSuccess(message) {
  if (window.SharedUtils) {
    SharedUtils.showSuccess(message);
  } else {
    console.log(message);
  }
}

// Preview data mapping rules
async function previewRules() {
  const button = document.querySelector('[onclick="previewRules()"]');
  const originalText = button.innerHTML;

  try {
    button.innerHTML = "🔄 Generating Preview...";
    button.disabled = true;

    const response = await fetch("/api/data-mapping/preview?view=final");
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }

    const data = await response.json();

    if (data.success) {
      showPreviewModal(data);
    } else {
      showAlert("error", `Preview failed: ${data.message || "Unknown error"}`);
    }
  } catch (error) {
    console.error("Error previewing rules:", error);
    showAlert("error", `Failed to generate preview: ${error.message}`);
  } finally {
    button.innerHTML = originalText;
    button.disabled = false;
  }
}

// Show rule preview modal
function showPreviewModal(data) {
  // Use final_channels from backend if available, otherwise aggregate manually
  const finalChannels = data.final_channels || [];

  // Create modal HTML with improved layout
  const modalHtml = `
    <div id="previewModal" class="modal">
      <div class="modal-content standard-modal preview-modal-large">
        <div class="modal-header">
          <h3 class="modal-title">Data Mapping Rules Preview</h3>
        </div>
        <div class="modal-body preview-modal-body">
          <div class="preview-summary mb-3">
            <div class="d-flex justify-content-between align-items-center">
              <div class="summary-text">
                <strong>${data.total_rules}</strong> active rules affecting <strong>${finalChannels.length}</strong> channels
              </div>
              <div class="form-check">
                <input class="form-check-input" type="checkbox" id="showPerRuleView">
                <label class="form-check-label" for="showPerRuleView">
                  Show per-rule breakdown
                </label>
              </div>
            </div>
          </div>

          <!-- Final Aggregated View (Default) -->
          <div id="finalView" class="preview-content">
            <div class="row">
              <div class="col-md-6">
                <h5>Rules Summary</h5>
                <div class="rules-summary-horizontal">
                  ${data.rules
                    .map(
                      (rule) => `
                    <div class="rule-summary-card">
                      <div class="rule-card-header">
                        <div class="rule-name">${escapeHtml(rule.rule_name)}</div>
                        <div class="rule-stats">${rule.affected_channels_count} ch</div>
                      </div>
                      <div class="rule-details">
                        ${rule.conditions.length}c • ${rule.actions.length}a
                      </div>
                    </div>
                  `,
                    )
                    .join("")}
                </div>
              </div>

              <div class="col-md-6">
                <h5>Final Channel Results (${finalChannels.length})</h5>
                <div class="channels-list">
                  ${finalChannels
                    .map((channel) => {
                      const hasChanges =
                        channel.channel_name !==
                          channel.original_channel_name ||
                        (channel.tvg_id || "") !==
                          (channel.original_tvg_id || "") ||
                        (channel.tvg_name || "") !==
                          (channel.original_tvg_name || "") ||
                        (channel.tvg_logo || "") !==
                          (channel.original_tvg_logo || "") ||
                        (channel.group_title || "") !==
                          (channel.original_group_title || "");

                      return `
                        <div class="channel-item">
                          <div class="channel-header">
                            <span class="channel-name">${escapeHtml(channel.channel_name)} • ${escapeHtml(channel.source_name)} • ${channel.tvg_id || "No TVG-ID"}</span>
                          </div>
                          ${
                            hasChanges
                              ? `
                            <div class="channel-changes">
                              ${
                                channel.channel_name !==
                                channel.original_channel_name
                                  ? `
                                <div class="change-item">
                                  <span class="field-name">name:</span>
                                  <span class="change-arrow">→</span>
                                  <span class="new-value">${escapeHtml(channel.channel_name)}</span>
                                </div>
                              `
                                  : ""
                              }
                              ${
                                (channel.tvg_id || "") !==
                                (channel.original_tvg_id || "")
                                  ? `
                                <div class="change-item">
                                  <span class="field-name">tvg-id:</span>
                                  <span class="change-arrow">→</span>
                                  <span class="new-value">${escapeHtml(channel.tvg_id || "")}</span>
                                </div>
                              `
                                  : ""
                              }
                              ${
                                (channel.tvg_name || "") !==
                                (channel.original_tvg_name || "")
                                  ? `
                                <div class="change-item">
                                  <span class="field-name">tvg-name:</span>
                                  <span class="change-arrow">→</span>
                                  <span class="new-value">${escapeHtml(channel.tvg_name || "")}</span>
                                </div>
                              `
                                  : ""
                              }
                              ${
                                (channel.tvg_logo || "") !==
                                (channel.original_tvg_logo || "")
                                  ? `
                                <div class="change-item">
                                  <span class="field-name">tvg-logo:</span>
                                  <span class="change-arrow">→</span>
                                  <span class="new-value">${escapeHtml(channel.tvg_logo || "")}</span>
                                </div>
                              `
                                  : ""
                              }
                              ${
                                (channel.group_title || "") !==
                                (channel.original_group_title || "")
                                  ? `
                                <div class="change-item">
                                  <span class="field-name">group:</span>
                                  <span class="change-arrow">→</span>
                                  <span class="new-value">${escapeHtml(channel.group_title || "")}</span>
                                </div>
                              `
                                  : ""
                              }
                            </div>
                          `
                              : `
                            <div class="text-muted" style="font-size: 0.75rem;">No changes</div>
                          `
                          }
                          <div class="applied-rules">
                            Rules: ${Array.isArray(channel.applied_rules) ? channel.applied_rules.join(", ") : "None"}
                          </div>
                        </div>
                      `;
                    })
                    .join("")}
                </div>
              </div>
            </div>
          </div>

          <!-- Per-Rule View (Hidden by default) -->
          <div id="perRuleView" class="preview-content" style="display: none;">
            ${data.rules
              .map(
                (rule) => `
              <div class="card mb-3">
                <div class="card-header">
                  <h5>${escapeHtml(rule.rule_name)}</h5>
                  <small class="text-muted">Affects ${rule.affected_channels_count} channels</small>
                </div>
                <div class="card-body">
                  ${rule.rule_description ? `<p class="text-muted">${escapeHtml(rule.rule_description)}</p>` : ""}

                  <div class="row">
                    <div class="col-md-6">
                      <h6>Conditions:</h6>
                      <ul class="list-unstyled">
                        ${rule.conditions
                          .map(
                            (condition) => `
                          <li class="mb-1">
                            <code>${condition.field_name}</code>
                            <span class="badge badge-secondary">${condition.operator}</span>
                            <code>"${escapeHtml(condition.value)}"</code>
                          </li>
                        `,
                          )
                          .join("")}
                      </ul>

                      <h6>Actions:</h6>
                      <ul class="list-unstyled">
                        ${rule.actions
                          .map(
                            (action) => `
                          <li class="mb-1">
                            <span class="badge badge-primary">${action.action_type}</span>
                            <code>${action.target_field}</code>
                            ${action.value ? `= "${escapeHtml(action.value)}"` : ""}
                            ${action.logo_asset_id ? `= Custom Logo` : ""}
                          </li>
                        `,
                          )
                          .join("")}
                      </ul>
                    </div>

                    <div class="col-md-6">
                      ${
                        rule.affected_channels_count > 0
                          ? `
                        <h6>Affected Channels (${rule.affected_channels_count})</h6>
                        <div class="channels-list">
                          ${rule.affected_channels
                            .map(
                              (channel) => `
                            <div class="channel-item">
                              <div class="channel-header">
                                <span class="channel-name">${escapeHtml(channel.channel_name)} • ${escapeHtml(channel.source_name)} • ${channel.tvg_id || "No TVG-ID"}</span>
                              </div>
                              <div class="channel-changes">
                                ${channel.actions_preview
                                  .filter((a) => a.will_change)
                                  .map(
                                    (action) => `
                                  <div class="change-item">
                                    <span class="field-name">${action.target_field}:</span>
                                    <span class="change-arrow">→</span>
                                    <span class="new-value">${escapeHtml(action.new_value || "null")}</span>
                                  </div>
                                `,
                                  )
                                  .join("")}
                              </div>
                            </div>
                          `,
                            )
                            .join("")}
                        </div>
                      `
                          : `<p class="text-muted">No channels match this rule's conditions.</p>`
                      }
                    </div>
                  </div>
                </div>
              </div>
            `,
              )
              .join("")}
          </div>
        </div>
        <div class="modal-footer">
          <button type="button" class="btn btn-secondary" id="previewModalCloseBtn">
            Close
          </button>
        </div>
      </div>
    </div>
  `;

  // Add modal to DOM
  document.body.insertAdjacentHTML("beforeend", modalHtml);

  // Add event listener for the toggle
  document
    .getElementById("showPerRuleView")
    .addEventListener("change", function () {
      const finalView = document.getElementById("finalView");
      const perRuleView = document.getElementById("perRuleView");

      if (this.checked) {
        finalView.style.display = "none";
        perRuleView.style.display = "block";
      } else {
        finalView.style.display = "block";
        perRuleView.style.display = "none";
      }
    });

  // Setup close handlers and show modal
  SharedUtils.setupStandardModalCloseHandlers("previewModal");

  // Add close button functionality to footer
  document
    .getElementById("previewModalCloseBtn")
    .addEventListener("click", () => {
      closePreviewModal();
    });

  // Show the modal
  SharedUtils.showStandardModal("previewModal");
}

// Close preview modal
function closePreviewModal() {
  SharedUtils.hideStandardModal("previewModal");
  // Remove modal from DOM after hiding
  setTimeout(() => {
    const modal = document.getElementById("previewModal");
    if (modal) {
      modal.remove();
    }
  }, 300);
}
