// Data Mapping Management JavaScript - Expression-Only Mode

let currentRules = [];
let editingRule = null;
let validationTimeout = null;
let lastValidationExpression = "";
let availableSources = [];

// Available field options by source type (loaded from API)
let STREAM_FIELDS = [];
let EPG_FIELDS = [];
let FIELD_DESCRIPTIONS = {};

// Available operators for expressions
const OPERATORS = [
  "equals",
  "contains",
  "starts_with",
  "ends_with",
  "matches",
  "not_equals",
  "not_contains",
  "not_matches",
];

// Initialize the data mapping manager
async function init() {
  await loadFieldDefinitions();
  await loadRules();
  setupEventListeners();
}

// Setup event listeners
function setupEventListeners() {
  // Note: Removed backdrop click to close for rule modal to prevent accidental closing

  // Logo picker modal close
  document.getElementById("logoPickerModal").addEventListener("click", (e) => {
    if (e.target === document.getElementById("logoPickerModal")) {
      closeLogoPickerModal();
    }
  });

  // Expression examples modal close
  document
    .getElementById("expressionExamplesModal")
    .addEventListener("click", (e) => {
      if (e.target === document.getElementById("expressionExamplesModal")) {
        closeExpressionExamples();
      }
    });
}

// Load field definitions from API
async function loadFieldDefinitions() {
  try {
    // Load stream fields
    const streamResponse = await fetch("/api/data-mapping/fields/stream");
    if (streamResponse.ok) {
      const streamData = await streamResponse.json();
      STREAM_FIELDS = streamData.fields.map((f) => f.name);
      streamData.fields.forEach((f) => {
        FIELD_DESCRIPTIONS[f.name] = f.description;
      });
    }

    // Load EPG fields
    const epgResponse = await fetch("/api/data-mapping/fields/epg");
    if (epgResponse.ok) {
      const epgData = await epgResponse.json();
      EPG_FIELDS = epgData.fields.map((f) => f.name);
      epgData.fields.forEach((f) => {
        FIELD_DESCRIPTIONS[f.name] = f.description;
      });
    }
  } catch (error) {
    console.error("Failed to load field definitions:", error);
    // Fallback to hardcoded fields
    STREAM_FIELDS = [
      "channel_name",
      "tvg_id",
      "tvg_name",
      "tvg_logo",
      "tvg_shift",
      "group_title",
      "stream_url",
    ];
    EPG_FIELDS = [
      "channel_id",
      "channel_name",
      "channel_logo",
      "channel_group",
      "language",
    ];
  }
}

// Helper function to get available fields based on source type
function getAvailableFields(sourceType) {
  return sourceType === "stream" ? STREAM_FIELDS : EPG_FIELDS;
}

// Update rule scope options based on source type
function updateRuleScope() {
  const sourceType = document.getElementById("ruleSourceType")?.value;
  const scopeSelect = document.getElementById("ruleScope");

  if (!sourceType || !scopeSelect) return;

  // Hide/show scope options based on source type
  const streamWideOption = scopeSelect.querySelector(
    'option[value="streamwide"]',
  );
  const epgWideOption = scopeSelect.querySelector('option[value="epgwide"]');

  if (sourceType === "stream") {
    if (streamWideOption) streamWideOption.style.display = "";
    if (epgWideOption) epgWideOption.style.display = "none";
    if (scopeSelect.value === "epgwide") {
      scopeSelect.value = "";
    }
  } else if (sourceType === "epg") {
    if (streamWideOption) streamWideOption.style.display = "none";
    if (epgWideOption) epgWideOption.style.display = "";
    if (scopeSelect.value === "streamwide") {
      scopeSelect.value = "";
    }
  }
}

// Update fields and load sources when source type changes
function updateFieldsAndActions() {
  updateRuleScope();
  loadTestSources();
  validateExpression();
}

// Load available sources for testing based on source type
async function loadTestSources() {
  const sourceType = document.getElementById("ruleSourceType")?.value;
  if (!sourceType) {
    console.log("No source type selected");
    return;
  }

  console.log("Loading test sources for type:", sourceType);

  try {
    const response = await fetch("/api/sources/unified");
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }
    const allSources = await response.json();
    console.log("All sources loaded:", allSources.length);
    console.log(
      "First source structure:",
      allSources.length > 0 ? allSources[0] : "No sources",
    );

    // Filter sources by type using source_kind property
    availableSources = allSources.filter((source) => {
      if (sourceType === "stream") {
        return source.source_kind === "stream";
      } else if (sourceType === "epg") {
        return source.source_kind === "epg";
      }
      return false;
    });

    console.log(
      "Filtered sources for",
      sourceType,
      ":",
      availableSources.length,
    );
    updateTestSourceUI();
  } catch (error) {
    console.error("Failed to load sources:", error);
    availableSources = [];
    updateTestSourceUI();
  }
}

// Update test source UI based on available sources (like filters)
function updateTestSourceUI() {
  const testSourceSelect = document.getElementById("testSourceSelect");
  const testSourceContainer = document.getElementById("testSourceContainer");

  if (!testSourceSelect || !testSourceContainer) {
    console.log("Test source elements not found");
    return;
  }

  console.log(
    "Updating test source UI with",
    availableSources.length,
    "sources",
  );

  // Clear existing options
  testSourceSelect.innerHTML =
    '<option value="">Select a source to test against...</option>';

  // Add available sources
  availableSources.forEach((source) => {
    const option = document.createElement("option");
    option.value = source.id;
    option.textContent = source.name;
    testSourceSelect.appendChild(option);
  });

  // Add event listener for source selection changes
  testSourceSelect.onchange = function () {
    console.log("Source selected:", this.value);
    updateTestButton();
  };

  // Hide/show source selector based on number of sources (like filters UI)
  if (availableSources.length === 0) {
    testSourceContainer.style.display = "none";
  } else if (availableSources.length === 1) {
    // Hide dropdown but show test button - auto-select the only source
    testSourceSelect.style.display = "none";
    testSourceSelect.value = availableSources[0].id;
    testSourceContainer.style.display = "block";

    // Show a label indicating which source is selected
    let sourceLabel = testSourceContainer.querySelector(
      ".auto-selected-source",
    );
    if (!sourceLabel) {
      sourceLabel = document.createElement("div");
      sourceLabel.className = "auto-selected-source text-muted small";
      testSourceContainer.insertBefore(sourceLabel, testSourceSelect);
    }
    sourceLabel.textContent = `Testing against: ${availableSources[0].name}`;
  } else {
    // Show dropdown for multiple sources
    testSourceSelect.style.display = "block";
    testSourceContainer.style.display = "block";

    // Remove auto-selected label if it exists
    const sourceLabel = testSourceContainer.querySelector(
      ".auto-selected-source",
    );
    if (sourceLabel) {
      sourceLabel.remove();
    }
  }

  updateTestButton();
}

// Update test button state
function updateTestButton() {
  const testButton = document.getElementById("runTestBtn");
  const expression = document.getElementById("ruleExpression")?.value?.trim();
  const sourceId = document.getElementById("testSourceSelect")?.value;

  if (!testButton) {
    console.log("Test button not found");
    return;
  }

  const canTest = expression && sourceId && availableSources.length > 0;
  console.log("Test button update:", {
    expression: !!expression,
    sourceId: !!sourceId,
    availableSourcesCount: availableSources.length,
    canTest: canTest,
  });

  testButton.disabled = !canTest;
}

// Debounced expression validation
function debouncedValidateExpression() {
  clearTimeout(validationTimeout);
  validationTimeout = setTimeout(() => {
    validateExpression();
  }, 500);
}

// Validate expression syntax
function validateExpression() {
  const expression = document.getElementById("ruleExpression")?.value?.trim();
  const sourceType = document.getElementById("ruleSourceType")?.value;
  const textarea = document.getElementById("ruleExpression");
  const validationDiv = document.getElementById("expressionValidation");

  if (!validationDiv || !textarea) return;

  // Clear previous validation
  clearValidationHighlights(textarea);
  validationDiv.innerHTML = "";

  if (!expression) {
    textarea.classList.remove("is-invalid", "is-valid");
    updateTestButton();
    return;
  }

  if (!sourceType) {
    validationDiv.innerHTML = `
      <div class="alert alert-warning">
        <small>⚠️ Please select a source type first</small>
      </div>
    `;
    textarea.classList.remove("is-valid");
    textarea.classList.add("is-invalid");
    updateTestButton();
    return;
  }

  // Skip validation if expression hasn't changed
  if (expression === lastValidationExpression) {
    updateTestButton();
    return;
  }

  lastValidationExpression = expression;

  // Start with lightweight client-side validation for immediate feedback
  const clientValidation = validateExpressionSyntax(expression, sourceType);
  updateValidationHighlights(textarea, clientValidation);

  // Skip server validation if critical client-side errors (like unmatched quotes)
  if (
    !clientValidation.valid &&
    clientValidation.errors.some(
      (error) => error.includes("quote") || error.includes("parenthes"),
    )
  ) {
    validationDiv.innerHTML = `
      <div class="alert alert-danger py-2">
        <small>❌ ${clientValidation.errors.join(", ")}</small>
      </div>
    `;
    textarea.classList.remove("is-valid");
    textarea.classList.add("is-invalid");
    updateTestButton();
    return;
  }

  // Show loading indicator for server validation
  validationDiv.innerHTML = `
    <div class="d-flex align-items-center text-muted small">
      <div class="spinner-border spinner-border-sm me-2" role="status" style="width: 12px; height: 12px;">
        <span class="visually-hidden">Loading...</span>
      </div>
      Validating expression...
    </div>
  `;

  // Validate expression on server - this is the authoritative validation
  validateExpressionAsync(expression, sourceType)
    .then((result) => {
      if (result.isValid) {
        validationDiv.innerHTML = `
          <div class="alert alert-success py-2">
            <small>✅ Expression is valid</small>
          </div>
        `;
        textarea.classList.remove("is-invalid");
        textarea.classList.add("is-valid");

        // Clear client-side error highlights on successful server validation
        clearValidationHighlights(textarea);
        updateTestButton();
      } else {
        validationDiv.innerHTML = `
          <div class="alert alert-danger py-2">
            <small>❌ ${result.error}</small>
          </div>
        `;
        textarea.classList.remove("is-valid");
        textarea.classList.add("is-invalid");

        // Show server error with less intrusive highlighting
        const serverValidation = {
          valid: false,
          highlights: [
            {
              start: 0,
              end: expression.length,
              type: "error",
              message: result.error,
            },
          ],
        };
        updateValidationHighlights(textarea, serverValidation);
        updateTestButton();
      }
    })
    .catch((error) => {
      console.error("Validation error:", error);
      validationDiv.innerHTML = `
        <div class="alert alert-warning py-2">
          <small>⚠️ Could not validate expression - server unavailable</small>
        </div>
      `;
      textarea.classList.remove("is-valid");
      textarea.classList.add("is-invalid");
      updateTestButton();
    });
}

// Client-side validation for immediate feedback
function validateExpressionSyntax(expression, sourceType) {
  const result = {
    valid: true,
    errors: [],
    highlights: [],
  };

  const availableFields = getAvailableFields(sourceType);

  try {
    // Basic syntax checks only - let server handle complex validation

    // Check for unmatched quotes
    const quotes = expression.match(/"/g);
    if (quotes && quotes.length % 2 !== 0) {
      const lastQuoteIndex = expression.lastIndexOf('"');
      result.valid = false;
      result.errors.push("Unmatched quote - missing closing quote");
      result.highlights.push({
        start: lastQuoteIndex,
        end: lastQuoteIndex + 1,
        type: "error",
        message: "Unmatched quote",
      });
    }

    // Check for empty parentheses
    if (/\(\s*\)/.test(expression)) {
      const match = expression.match(/\(\s*\)/);
      result.valid = false;
      result.errors.push("Empty parentheses are not allowed");
      result.highlights.push({
        start: match.index,
        end: match.index + match[0].length,
        type: "error",
        message: "Empty parentheses",
      });
    }

    // Basic parentheses matching
    let parenCount = 0;
    let lastUnmatched = -1;
    for (let i = 0; i < expression.length; i++) {
      if (expression[i] === "(") {
        parenCount++;
        if (lastUnmatched === -1) lastUnmatched = i;
      } else if (expression[i] === ")") {
        parenCount--;
        if (parenCount === 0) lastUnmatched = -1;
        if (parenCount < 0) {
          result.valid = false;
          result.errors.push("Unmatched closing parenthesis");
          result.highlights.push({
            start: i,
            end: i + 1,
            type: "error",
            message: "Unmatched closing parenthesis",
          });
          break;
        }
      }
    }

    if (parenCount > 0) {
      result.valid = false;
      result.errors.push("Unmatched opening parenthesis");
      if (lastUnmatched >= 0) {
        result.highlights.push({
          start: lastUnmatched,
          end: lastUnmatched + 1,
          type: "error",
          message: "Unmatched opening parenthesis",
        });
      }
    }

    // Only do lightweight field validation - let server handle modifiers and complex patterns
    if (availableFields.length > 0) {
      // Use same regex pattern as filters for consistency
      const fieldRegex = /\b(\w+)\s+(?:not\s+)?(?:case_sensitive\s+)?(\w+)/gi;
      let match;
      while ((match = fieldRegex.exec(expression)) !== null) {
        const fieldName = match[1];
        const operator = match[2];

        // Skip logical operators and keywords
        if (
          ["and", "or", "set"].includes(fieldName.toLowerCase()) ||
          ["and", "or", "set"].includes(operator.toLowerCase())
        ) {
          continue;
        }

        if (!availableFields.includes(fieldName)) {
          // Only warn, don't fail validation - let server decide
          const suggestions = findSimilarFields(fieldName, availableFields);
          const suggestionText =
            suggestions.length > 0
              ? ` Did you mean: ${suggestions.slice(0, 3).join(", ")}?`
              : "";

          result.highlights.push({
            start: match.index,
            end: match.index + fieldName.length,
            type: "warning",
            message: `Possible unknown field '${fieldName}'.${suggestionText} Available fields: ${availableFields.join(", ")}`,
          });
        }
      }
    }
  } catch (error) {
    result.valid = false;
    result.errors.push("Syntax validation error");
  }

  return result;
}

// Async expression validation
async function validateExpressionAsync(expression, sourceType) {
  try {
    const response = await fetch("/api/data-mapping/validate", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        expression: expression,
        source_type: sourceType,
      }),
    });

    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }

    return await response.json();
  } catch (error) {
    return { isValid: false, error: "Validation service unavailable" };
  }
}

// Update validation highlights (similar to filters)
function updateValidationHighlights(textarea, validation) {
  clearValidationHighlights(textarea);

  if (validation.highlights && validation.highlights.length > 0) {
    showValidationMessages(textarea, validation.highlights);
  }
}

// Clear validation highlights
function clearValidationHighlights(textarea) {
  const messagesContainer = document.getElementById(
    "validation-messages-container",
  );
  if (messagesContainer) {
    messagesContainer.remove();
  }
}

// Show validation messages below textarea
function showValidationMessages(textarea, highlights) {
  let container = document.getElementById("validation-messages-container");
  if (!container) {
    container = document.createElement("div");
    container.id = "validation-messages-container";
    container.className = "validation-messages-container mt-2";
    textarea.parentNode.insertBefore(container, textarea.nextSibling);
  }

  // Deduplicate highlights
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
    html += `<div class="validation-message-header text-danger small">⚠ ${errors.length} Error${errors.length > 1 ? "s" : ""}:</div>`;
    errors.forEach((error) => {
      const fieldDesc = FIELD_DESCRIPTIONS[error.text]
        ? ` (${FIELD_DESCRIPTIONS[error.text]})`
        : "";
      html += `<div class="validation-message-item small text-muted" title="${escapeHtml(error.message)}">• <code>${escapeHtml(error.text)}</code>${fieldDesc} - ${escapeHtml(error.message)}</div>`;
    });
    html += `</div>`;
  }

  if (warnings.length > 0) {
    html += `<div class="validation-message-group">`;
    html += `<div class="validation-message-header text-warning small">⚠ ${warnings.length} Warning${warnings.length > 1 ? "s" : ""}:</div>`;
    warnings.forEach((warning) => {
      const fieldDesc = FIELD_DESCRIPTIONS[warning.text]
        ? ` (${FIELD_DESCRIPTIONS[warning.text]})`
        : "";
      html += `<div class="validation-message-item small text-muted" title="${escapeHtml(warning.message)}">• <code>${escapeHtml(warning.text)}</code>${fieldDesc} - ${escapeHtml(warning.message)}</div>`;
    });
    html += `</div>`;
  }

  container.innerHTML = html;
}

// Find similar field names for suggestions
function findSimilarFields(input, validFields) {
  const inputLower = input.toLowerCase();
  const suggestions = [];

  // Exact partial matches first
  validFields.forEach((field) => {
    if (
      field.toLowerCase().includes(inputLower) ||
      inputLower.includes(field.toLowerCase())
    ) {
      suggestions.push(field);
    }
  });

  // If no partial matches, find fields that start with same letter
  if (suggestions.length === 0) {
    validFields.forEach((field) => {
      if (field.toLowerCase().charAt(0) === inputLower.charAt(0)) {
        suggestions.push(field);
      }
    });
  }

  return suggestions;
}

// Escape HTML for safe display
function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}

// Generate a visual tree representation of an expression
function generateExpressionTree(expression) {
  if (!expression || typeof expression !== "string") {
    return "";
  }

  try {
    // Parse the expression to extract logical structure
    const tree = parseExpressionToTree(expression);
    if (!tree) return "";

    return `
      <div class="expression-tree-container">
        <div class="expression-tree-header">
          <strong>Logical Structure:</strong>
        </div>
        <div class="expression-tree">
          ${renderTreeNode(tree, 0)}
        </div>
      </div>
    `;
  } catch (error) {
    console.warn("Failed to parse expression tree:", error);
    return "";
  }
}

// Simple expression parser for tree visualization
function parseExpressionToTree(expression) {
  // Remove the SET action part for tree visualization
  let conditionPart = expression.split(" SET ")[0].trim();

  // Handle parentheses and logical operators
  if (conditionPart.includes(" AND ") || conditionPart.includes(" OR ")) {
    return parseLogicalExpression(conditionPart);
  } else {
    return parseCondition(conditionPart);
  }
}

// Parse logical expressions with AND/OR
function parseLogicalExpression(expr) {
  // Simple parsing - look for main logical operator
  // Handle parentheses by finding the main operator outside parentheses

  let depth = 0;
  let mainOperatorPos = -1;
  let mainOperator = null;

  // Find the main logical operator (AND/OR) outside parentheses
  for (let i = 0; i < expr.length; i++) {
    if (expr[i] === "(") depth++;
    else if (expr[i] === ")") depth--;
    else if (depth === 0) {
      if (expr.substr(i, 5) === " AND ") {
        mainOperatorPos = i;
        mainOperator = "AND";
        break;
      } else if (expr.substr(i, 4) === " OR ") {
        mainOperatorPos = i;
        mainOperator = "OR";
        break;
      }
    }
  }

  if (mainOperator && mainOperatorPos > 0) {
    const left = expr.substring(0, mainOperatorPos).trim();
    const right = expr
      .substring(mainOperatorPos + mainOperator.length + 2)
      .trim();

    return {
      type: "operator",
      operator: mainOperator,
      children: [parseExpressionToTree(left), parseExpressionToTree(right)],
    };
  }

  // Remove outer parentheses if present
  if (expr.startsWith("(") && expr.endsWith(")")) {
    return parseExpressionToTree(expr.slice(1, -1));
  }

  return parseCondition(expr);
}

// Parse individual conditions
function parseCondition(condition) {
  // Match patterns like "field operator value"
  const match = condition.match(
    /^(.+?)\s+(equals|contains|matches|starts_with|ends_with|not_equals|not_contains|not_matches)\s+"?([^"]*)"?$/,
  );

  if (match) {
    return {
      type: "condition",
      field: match[1].trim(),
      operator: match[2],
      value: match[3],
    };
  }

  return {
    type: "condition",
    field: "unknown",
    operator: "unknown",
    value: condition,
  };
}

// Render a tree node with proper indentation
function renderTreeNode(node, depth) {
  if (!node) return "";

  const indent = "  ".repeat(depth);

  if (node.type === "operator") {
    let html = `<div class="tree-node tree-operator">${indent}${node.operator}</div>`;

    node.children.forEach((child, index) => {
      const isLast = index === node.children.length - 1;
      const connector = isLast ? "└── " : "├── ";

      if (child.type === "operator") {
        html += `<div class="tree-node tree-connector">${indent}${connector}</div>`;
        html += renderTreeNode(child, depth + 1);
      } else {
        html += `<div class="tree-node tree-condition">${indent}${connector}(${escapeHtml(child.field)} ${child.operator} "${escapeHtml(child.value)}")</div>`;
      }
    });

    return html;
  } else {
    return `<div class="tree-node tree-condition">${indent}(${escapeHtml(node.field)} ${node.operator} "${escapeHtml(node.value)}")</div>`;
  }
}

// Load all data mapping rules
async function loadRules() {
  try {
    const response = await fetch("/api/data-mapping");
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }
    currentRules = await response.json();
    renderRules();
  } catch (error) {
    console.error("Failed to load rules:", error);
    showAlert("error", "Failed to load data mapping rules");
  }
}

// Render rules in the UI
function renderRules() {
  const container = document.getElementById("rulesContainer");
  if (!container) return;

  if (currentRules.length === 0) {
    container.innerHTML = `
      <div class="text-center py-4">
        <p class="text-muted">No data mapping rules configured yet</p>
        <button class="btn btn-primary" onclick="createRule()">
          ➕ Create First Rule
        </button>
      </div>
    `;
    return;
  }

  container.innerHTML = currentRules
    .map(
      (rule, index) => `
    <div class="rule-card" data-rule-id="${rule.id}" data-sort-order="${rule.sort_order}">
      <div class="rule-header">
        <div class="drag-handle" title="Drag to reorder" draggable="true">
          <span class="drag-icon">⋮</span>
          <span class="rule-order">#${index + 1}</span>
        </div>
        <div class="rule-info">
          <h5 class="rule-name">${escapeHtml(rule.name)}</h5>
          <div class="rule-meta">
            <span class="badge badge-${rule.source_type === "stream" ? "primary" : "info"}">
              ${rule.source_type.toUpperCase()}
            </span>
            <span class="badge badge-secondary">${rule.scope}</span>
            <span class="badge badge-${rule.is_active ? "success" : "warning"}">
              ${rule.is_active ? "Active" : "Inactive"}
            </span>
          </div>
        </div>
        <div class="rule-actions">
          <button class="btn btn-sm btn-success" onclick="editRule('${rule.id}')">
            ✏️ Edit
          </button>
          <button class="btn btn-sm btn-danger" onclick="deleteRule('${rule.id}')">
            🗑️ Delete
          </button>
        </div>
      </div>
      <div class="rule-content">
        ${rule.description ? `<div class="rule-description">${escapeHtml(rule.description)}</div>` : ""}
        <div class="rule-expression">
          <strong>Expression:</strong>
          <pre><code>${escapeHtml(rule.expression || "No expression defined")}</code></pre>
          ${rule.expression ? generateExpressionTree(rule.expression) : ""}
        </div>
      </div>
    </div>
  `,
    )
    .join("");

  // Setup drag and drop functionality
  enableDragAndDrop();
}

// Create new rule
function createRule() {
  editingRule = null;
  document.getElementById("ruleModalTitle").textContent =
    "Create Data Mapping Rule";
  clearRuleForm();
  showRuleModal();
}

// Edit existing rule
function editRule(ruleId) {
  const rule = currentRules.find((r) => r.id === ruleId);
  if (!rule) return;

  editingRule = rule;
  document.getElementById("ruleModalTitle").textContent =
    "Edit Data Mapping Rule";
  populateRuleForm(rule);
  showRuleModal();
}

// Delete rule
async function deleteRule(ruleId) {
  const rule = currentRules.find((r) => r.id === ruleId);
  if (!rule) return;

  if (!confirm(`Are you sure you want to delete the rule "${rule.name}"?`)) {
    return;
  }

  try {
    const response = await fetch(`/api/data-mapping/rules/${ruleId}`, {
      method: "DELETE",
    });

    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }

    showAlert("success", "Rule deleted successfully");
    await loadRules();
  } catch (error) {
    console.error("Failed to delete rule:", error);
    showAlert("error", "Failed to delete rule");
  }
}

// Show rule modal
function showRuleModal() {
  const modal = document.getElementById("ruleModal");
  modal.style.display = "flex";
  modal.classList.add("show");
  document.body.classList.add("modal-open");

  // Initialize autocomplete for expression textarea
  const textarea = document.getElementById("ruleExpression");
  if (textarea && !textarea.expressionAutocomplete) {
    textarea.expressionAutocomplete = new ExpressionAutocomplete(textarea);
  }
}

// Close rule modal
function closeRuleModal() {
  const modal = document.getElementById("ruleModal");
  modal.style.display = "none";
  modal.classList.remove("show");
  document.body.classList.remove("modal-open");
  clearRuleForm();
}

// Clear rule form
function clearRuleForm() {
  document.getElementById("ruleForm").reset();
  document.getElementById("expressionValidation").innerHTML = "";
  availableSources = [];
  updateTestSourceUI();
}

// Populate rule form with existing rule data
function populateRuleForm(rule) {
  document.getElementById("ruleName").value = rule.name;
  document.getElementById("ruleDescription").value = rule.description || "";
  document.getElementById("ruleSourceType").value = rule.source_type;
  document.getElementById("ruleScope").value = rule.scope;
  document.getElementById("ruleActive").checked = rule.is_active;
  document.getElementById("ruleExpression").value = rule.expression || "";

  // Update dependent fields
  updateFieldsAndActions();
}

// Save rule
async function saveRule() {
  const form = document.getElementById("ruleForm");
  if (!form.checkValidity()) {
    form.reportValidity();
    return;
  }

  const formData = new FormData(form);
  const ruleData = {
    name: formData.get("name"),
    description: formData.get("description") || null,
    source_type: formData.get("source_type"),
    expression: formData.get("expression"),
  };

  // Add is_active for updates
  if (editingRule) {
    ruleData.is_active = formData.has("is_active");
  }

  try {
    const url = editingRule
      ? `/api/data-mapping/${editingRule.id}`
      : "/api/data-mapping";
    const method = editingRule ? "PUT" : "POST";

    const response = await fetch(url, {
      method: method,
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(ruleData),
    });

    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }

    const action = editingRule ? "updated" : "created";
    showAlert("success", `Rule ${action} successfully`);
    closeRuleModal();
    await loadRules();
  } catch (error) {
    console.error("Failed to save rule:", error);
    showAlert("error", "Failed to save rule");
  }
}

// Test rule expression
async function runRuleTest() {
  const expression = document.getElementById("ruleExpression").value.trim();
  const sourceType = document.getElementById("ruleSourceType").value;
  const sourceId = document.getElementById("testSourceSelect").value;

  if (!expression || !sourceType || !sourceId) {
    showAlert(
      "warning",
      "Please fill in expression, source type, and select a test source",
    );
    return;
  }

  const testButton = document.getElementById("runTestBtn");
  const originalText = testButton.textContent;
  testButton.disabled = true;
  testButton.textContent = "🧪 Testing...";

  try {
    const response = await fetch("/api/data-mapping/test", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        source_id: sourceId,
        source_type: sourceType,
        expression: expression,
      }),
    });

    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }

    const result = await response.json();
    displayTestResults(result);
  } catch (error) {
    console.error("Test failed:", error);
    showAlert("error", "Failed to test rule");
  } finally {
    testButton.disabled = false;
    testButton.textContent = originalText;
  }
}

// Display test results
function displayTestResults(result) {
  const resultsDiv = document.getElementById("testResults");
  const resultsContent = document.getElementById("testResultsContent");

  if (!resultsDiv || !resultsContent) return;

  if (!result.is_valid) {
    resultsContent.innerHTML = `
      <div class="alert alert-danger">
        <strong>Test Failed:</strong> ${escapeHtml(result.error)}
      </div>
    `;
  } else {
    resultsContent.innerHTML = `
      <div class="alert alert-success">
        <strong>Test Successful!</strong>
        Matched ${result.matched_count} out of ${result.total_channels} channels
      </div>
      <div class="test-results-channels">
        <h6>Matching Channels (${result.matching_channels.length}):</h6>
        ${result.matching_channels
          .map(
            (channel) => `
          <div class="test-result-channel">
            <strong>${escapeHtml(channel.channel_name)}</strong>
            ${channel.group_title ? `<span class="badge badge-secondary">${escapeHtml(channel.group_title)}</span>` : ""}
            ${
              channel.applied_actions && channel.applied_actions.length > 0
                ? `
              <div class="applied-actions">
                <small>Applied actions: ${channel.applied_actions.length}</small>
                <div class="action-details">
                  ${channel.applied_actions
                    .map((action) => {
                      // Check if action contains a logo URL
                      const logoUrlMatch = action.match(
                        /\(http[s]?:\/\/[^\)]+\/api\/logos\/[^)]+\)/,
                      );
                      if (logoUrlMatch) {
                        const logoUrl = logoUrlMatch[0].slice(1, -1); // Remove parentheses
                        const actionText = action.replace(logoUrlMatch[0], "");
                        return `
                        <div class="action-item">
                          <span>${escapeHtml(actionText)}</span>
                          <a href="${logoUrl}" target="_blank" class="logo-link" title="View logo">
                            <img src="${logoUrl}" alt="Logo" style="width: 24px; height: 24px; object-fit: contain; margin-left: 8px; border-radius: 2px; vertical-align: middle;">
                          </a>
                        </div>
                      `;
                      } else {
                        return `<div class="action-item">${escapeHtml(action)}</div>`;
                      }
                    })
                    .join("")}
                </div>
              </div>
            `
                : `
              <div class="applied-actions">
                <small>Applied actions: 0</small>
              </div>
            `
            }
          </div>
        `,
          )
          .join("")}
      </div>
    `;
  }

  resultsDiv.style.display = "block";
}

// Show expression examples modal
function showExpressionExamples() {
  const modal = document.getElementById("expressionExamplesModal");
  if (modal) {
    modal.style.display = "flex";
    modal.style.alignItems = "center";
    modal.style.justifyContent = "center";
    modal.style.zIndex = "1060"; // Ensure it's above other modals
    modal.classList.add("show");
    document.body.classList.add("modal-open");
  }
}

// Close expression examples modal
function closeExpressionExamples() {
  const modal = document.getElementById("expressionExamplesModal");
  if (modal) {
    modal.classList.remove("show");
    modal.style.display = "none";
    modal.style.alignItems = "";
    modal.style.justifyContent = "";
  }
  document.body.classList.remove("modal-open");
}

// Preview stream rules
async function previewStreamRules() {
  const button = document.querySelector(
    'button[onclick="previewStreamRules()"]',
  );
  const originalText = button ? button.innerHTML : "";

  try {
    // Show loading state
    if (button) {
      button.disabled = true;
      button.innerHTML = "⏳ Loading...";
    }

    // Show loading modal
    showLoadingModal("Loading stream rules preview...");

    const response = await fetch("/api/data-mapping/preview", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        source_type: "stream",
      }),
    });

    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }

    const result = await response.json();

    // Hide loading modal
    hideLoadingModal();

    displayPreviewResults("Stream Rules Preview", result);
  } catch (error) {
    console.error("Preview failed:", error);
    hideLoadingModal();
    showAlert(
      "error",
      "Failed to preview stream rules. Please check your configuration and try again.",
    );
  } finally {
    // Restore button state
    if (button) {
      button.disabled = false;
      button.innerHTML = originalText;
    }
  }
}

// Preview EPG rules
async function previewEpgRules() {
  const button = document.querySelector('button[onclick="previewEpgRules()"]');
  const originalText = button ? button.innerHTML : "";

  try {
    // Show loading state
    if (button) {
      button.disabled = true;
      button.innerHTML = "⏳ Loading...";
    }

    // Show loading modal
    showLoadingModal("Loading EPG rules preview...");

    const response = await fetch("/api/data-mapping/preview", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        source_type: "epg",
      }),
    });

    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }

    const result = await response.json();

    // Hide loading modal
    hideLoadingModal();

    displayPreviewResults("EPG Rules Preview", result);
  } catch (error) {
    console.error("Preview failed:", error);
    hideLoadingModal();
    showAlert(
      "error",
      "Failed to preview EPG rules. Please check your configuration and try again.",
    );
  } finally {
    // Restore button state
    if (button) {
      button.disabled = false;
      button.innerHTML = originalText;
    }
  }
}

// Display preview results with mutations
function displayPreviewResults(title, result) {
  const totalChannels = result.total_channels || 0;
  const affectedChannels = result.final_channels?.length || 0;
  const channels = result.final_channels || [];
  const rules = result.rules || [];

  // Initialize filter state - all rules selected by default
  const selectedRules = new Set(rules.map((_, index) => index));

  // Create rule filter cards
  const ruleCardsHtml =
    rules.length > 0
      ? `
    <div class="rule-filter-section">
      <h6 class="rule-filter-title">Filter by Applied Rules:</h6>
      <div class="rule-filter-cards">
        ${rules
          .map(
            (rule, index) => `
          <div class="rule-filter-card active" data-rule-index="${index}" data-rule-id="${rule.rule_id}" onclick="toggleRuleFilter(${index})">
            <div class="rule-card-header">
              <span class="rule-card-name" title="${escapeHtml(rule.rule_name)}">${truncateText(rule.rule_name, 25)}</span>
              <span class="rule-card-badge">${rule.affected_channels_count}</span>
            </div>
            <div class="rule-card-stats">
              <span class="rule-stat">📊 ${rule.affected_channels_count} channels</span>
              <span class="rule-stat">📋 ${rule.condition_count || 0}C/${rule.action_count || 0}A</span>
              <span class="rule-stat" title="Total: ${rule.total_execution_time || 0}μs across ${rule.affected_channels_count} channels">⏱️ ${rule.avg_execution_time > 0 ? rule.avg_execution_time + "μs/channel" : "<1μs/channel"}</span>
            </div>
          </div>
        `,
          )
          .join("")}
        <div class="rule-filter-card select-all active" onclick="toggleAllRules()">
          <div class="rule-card-header">
            <span class="rule-card-name">Deselect All</span>
            <span class="rule-card-badge">${affectedChannels}</span>
          </div>
          <div class="rule-card-stats">
            <span class="rule-stat">Toggle all rules</span>
          </div>
        </div>
      </div>
    </div>
  `
      : "";

  // Create modal HTML using application's standard modal structure
  const modalHtml = `
    <div class="modal" id="globalPreviewModal" style="display: none">
      <div class="modal-content standard-modal preview-modal-large">
        <div class="modal-header">
          <h3 class="modal-title">${title}</h3>
        </div>
        <div class="modal-body">
          ${ruleCardsHtml}

          ${
            channels.length > 0
              ? `
            <div class="preview-channels-container">
              <div class="channels-header">
                <h6>Modified Channels:</h6>
                <div class="channels-count-badge">${affectedChannels} channels</div>
              </div>
              <div class="preview-channels-list">
                ${channels
                  .map((channel, channelIndex) => {
                    const mutations = [];

                    // Check for changes between original and mapped values
                    if (
                      channel.original_channel_name !==
                      channel.mapped_channel_name
                    ) {
                      mutations.push({
                        field: "channel_name",
                        before: channel.original_channel_name,
                        after: channel.mapped_channel_name,
                        type: "text",
                      });
                    }
                    if (channel.original_tvg_id !== channel.mapped_tvg_id) {
                      mutations.push({
                        field: "tvg_id",
                        before: channel.original_tvg_id || "null",
                        after: channel.mapped_tvg_id || "null",
                        type: "text",
                      });
                    }
                    if (
                      channel.original_tvg_shift !== channel.mapped_tvg_shift
                    ) {
                      mutations.push({
                        field: "tvg_shift",
                        before: channel.original_tvg_shift || "null",
                        after: channel.mapped_tvg_shift || "null",
                        type: "text",
                      });
                    }
                    if (
                      channel.original_group_title !==
                      channel.mapped_group_title
                    ) {
                      mutations.push({
                        field: "group_title",
                        before: channel.original_group_title || "null",
                        after: channel.mapped_group_title || "null",
                        type: "text",
                      });
                    }
                    if (channel.original_tvg_logo !== channel.mapped_tvg_logo) {
                      mutations.push({
                        field: "tvg_logo",
                        before: channel.original_tvg_logo || "null",
                        after: channel.mapped_tvg_logo || "null",
                        type: "logo",
                      });
                    }

                    const truncatedName = truncateText(
                      channel.channel_name,
                      40,
                    );
                    const needsTooltip = channel.channel_name.length > 40;
                    const ruleCount = channel.applied_rules
                      ? channel.applied_rules.length
                      : 0;

                    return `
                    <div class="channel-preview-row" data-channel-index="${channelIndex}" data-applied-rules='${JSON.stringify(channel.applied_rules || [])}'>
                      <div class="channel-header">
                        <span class="channel-name" ${needsTooltip ? `title="${escapeHtml(channel.channel_name)}"` : ""}>
                          ${escapeHtml(truncatedName)}
                        </span>
                        <span class="channel-stats">${ruleCount} rule${ruleCount !== 1 ? "s" : ""} applied</span>
                      </div>
                      <div class="channel-mutations">
                        ${
                          mutations.length > 0
                            ? `
                          <code class="mutations-code">
${mutations
  .map((mut) => {
    const beforeVal = mut.before;
    const afterVal = mut.after;
    const logoPreview =
      mut.type === "logo"
        ? `<div class="inline-logo-preview">
           ${getLogoPreviewElement(mut.before, "before")}
           <span class="logo-arrow">→</span>
           ${getLogoPreviewElement(mut.after, "after")}
         </div>`
        : "";
    return `${mut.field.padEnd(12)} ${beforeVal} → ${afterVal}${logoPreview}`;
  })
  .join("\n")}
                          </code>
                        `
                            : `
                          <span class="no-mutations">No visible changes detected</span>
                        `
                        }
                      </div>
                    </div>
                  `;
                  })
                  .join("")}
              </div>
            </div>
          `
              : `
            <div class="empty-state">
              <div class="empty-state-icon">📋</div>
              <h5 class="empty-state-title">No Channels Modified</h5>
              <p class="empty-state-message">
                None of your current rules have modified any channels. This could mean:
              </p>
              <ul class="empty-state-suggestions">
                <li>Your rules aren't matching any channel data</li>
                <li>All rules are inactive</li>
                <li>No stream sources are configured</li>
              </ul>
              <div class="empty-state-actions">
                <button class="btn btn-primary btn-sm" onclick="closePreviewModal(); editRule ? editRule() : createRule()">
                  📝 Create New Rule
                </button>
              </div>
            </div>
          `
          }
        </div>
        <div class="modal-footer">
          <button type="button" class="btn btn-secondary" onclick="closePreviewModal()">Close</button>
        </div>
      </div>
    </div>
  `;

  // Remove any existing modal
  const existingModal = document.getElementById("globalPreviewModal");
  if (existingModal) {
    existingModal.remove();
  }

  // Add modal to DOM
  document.body.insertAdjacentHTML("beforeend", modalHtml);

  // Show modal using application's standard approach
  const modal = document.getElementById("globalPreviewModal");
  if (modal) {
    modal.style.display = "flex";
    modal.style.alignItems = "center";
    modal.style.justifyContent = "center";
    modal.classList.add("show");
    document.body.classList.add("modal-open");
  }

  // Store filter state globally for the modal
  window.previewFilterState = {
    selectedRules: selectedRules,
    allChannels: channels,
    allRules: rules,
  };
}

// Close preview modal function
function closePreviewModal() {
  const modal = document.getElementById("globalPreviewModal");
  if (modal) {
    modal.classList.remove("show");
    modal.style.display = "none";
    modal.style.alignItems = "";
    modal.style.justifyContent = "";
    document.body.classList.remove("modal-open");
    modal.remove();
  }
}

// Helper functions for preview
function truncateText(text, maxLength) {
  if (!text || text.length <= maxLength) return text;
  return text.substring(0, maxLength - 3) + "...";
}

// Helper function to parse logo UUIDs and create preview elements
function getLogoPreviewElement(logoValue, type) {
  if (!logoValue || logoValue === "null") {
    return `<div class="mutation-logo-placeholder ${type}">∅</div>`;
  }

  let logoUrl = logoValue;

  // Parse @logo: format to extract UUID and construct proper URL
  if (logoValue.includes("@logo:")) {
    const uuidMatch = logoValue.match(/@logo:([a-f0-9-]{36})/);
    if (uuidMatch) {
      const logoUuid = uuidMatch[1];
      // Get base URL from window location
      const baseUrl = window.location.origin;
      logoUrl = `${baseUrl}/api/logos/${logoUuid}`;
    }
  }

  if (logoUrl.startsWith("http")) {
    return `<img src="${logoUrl}"
                 class="mutation-logo-preview ${type}"
                 alt="${type}"
                 onmouseover="showLogoPopup(event, '${logoUrl}')"
                 onmouseout="hideLogoPopup()"
                 onerror="this.style.display='none'">`;
  }

  return `<div class="mutation-logo-placeholder ${type}">∅</div>`;
}

// Logo popup functions
function showLogoPopup(event, logoUrl) {
  // Remove any existing popup
  hideLogoPopup();

  const popup = document.createElement("div");
  popup.id = "logo-popup";
  popup.className = "logo-popup";
  popup.innerHTML = `<img src="${logoUrl}" alt="Logo preview" class="logo-popup-image">`;

  // Position popup near mouse
  popup.style.left = event.pageX + 10 + "px";
  popup.style.top = event.pageY - 50 + "px";

  document.body.appendChild(popup);
}

function hideLogoPopup() {
  const popup = document.getElementById("logo-popup");
  if (popup) {
    popup.remove();
  }
}

function toggleRuleFilter(ruleIndex) {
  const filterState = window.previewFilterState;
  if (!filterState) return;

  const card = document.querySelector(`[data-rule-index="${ruleIndex}"]`);
  const selectAllCard = document.querySelector(".rule-filter-card.select-all");

  if (filterState.selectedRules.has(ruleIndex)) {
    filterState.selectedRules.delete(ruleIndex);
    card.classList.remove("active");
  } else {
    filterState.selectedRules.add(ruleIndex);
    card.classList.add("active");
  }

  // Update select all button state
  const allSelected =
    filterState.selectedRules.size === filterState.allRules.length;
  const noneSelected = filterState.selectedRules.size === 0;

  if (allSelected) {
    selectAllCard.classList.add("active");
    selectAllCard.querySelector(".rule-card-name").textContent = "Deselect All";
  } else if (noneSelected) {
    selectAllCard.classList.remove("active");
    selectAllCard.querySelector(".rule-card-name").textContent = "Select All";
  } else {
    selectAllCard.classList.remove("active");
    selectAllCard.querySelector(".rule-card-name").textContent = "Select All";
  }

  updateChannelVisibility();
}

function toggleAllRules() {
  const filterState = window.previewFilterState;
  if (!filterState) return;

  const selectAllCard = document.querySelector(".rule-filter-card.select-all");
  const ruleCards = document.querySelectorAll(
    ".rule-filter-card:not(.select-all)",
  );

  const allSelected =
    filterState.selectedRules.size === filterState.allRules.length;

  if (allSelected) {
    // Deselect all
    filterState.selectedRules.clear();
    ruleCards.forEach((card) => card.classList.remove("active"));
    selectAllCard.classList.remove("active");
    selectAllCard.querySelector(".rule-card-name").textContent = "Select All";
  } else {
    // Select all
    filterState.selectedRules.clear();
    filterState.allRules.forEach((_, index) =>
      filterState.selectedRules.add(index),
    );
    ruleCards.forEach((card) => card.classList.add("active"));
    selectAllCard.classList.add("active");
    selectAllCard.querySelector(".rule-card-name").textContent = "Deselect All";
  }

  updateChannelVisibility();
}

function updateChannelVisibility() {
  const filterState = window.previewFilterState;
  if (!filterState) return;

  const channelRows = document.querySelectorAll(".channel-preview-row");

  // Create a mapping of rule IDs to selected indices
  const selectedRuleIds = new Set();
  filterState.selectedRules.forEach((ruleIndex) => {
    const rule = filterState.allRules[ruleIndex];
    if (rule && rule.rule_id) {
      selectedRuleIds.add(rule.rule_id);
    }
  });

  channelRows.forEach((row) => {
    const appliedRules = JSON.parse(row.dataset.appliedRules || "[]");

    // Show channel if any of its applied rules are selected
    // Allow showing nothing when no rules are selected (deselect all)
    const shouldShow =
      filterState.selectedRules.size > 0 &&
      appliedRules.some((ruleId) => selectedRuleIds.has(ruleId));

    row.style.display = shouldShow ? "block" : "none";
  });

  // Update channel count badge
  const visibleChannels = document.querySelectorAll(
    '.channel-preview-row:not([style*="display: none"])',
  ).length;
  const countBadge = document.querySelector(".channels-count-badge");
  if (countBadge) {
    countBadge.textContent = `${visibleChannels} channels`;
  }
}

// Utility functions
// Loading modal functions
function showLoadingModal(message = "Loading...") {
  const loadingModalHtml = `
    <div class="modal" id="loadingModal" style="display: flex; align-items: center; justify-content: center;">
      <div class="modal-content" style="max-width: 400px; text-align: center; padding: 2rem; border-radius: 8px;">
        <div class="loading" style="margin: 0 auto 1rem auto;"></div>
        <p style="margin: 0; color: var(--text-muted); font-size: 0.9rem;">${escapeHtml(message)}</p>
      </div>
    </div>
  `;

  // Remove existing loading modal
  const existing = document.getElementById("loadingModal");
  if (existing) existing.remove();

  // Add to DOM
  document.body.insertAdjacentHTML("beforeend", loadingModalHtml);
  document.body.classList.add("modal-open");
}

function hideLoadingModal() {
  const modal = document.getElementById("loadingModal");
  if (modal) {
    modal.remove();
    document.body.classList.remove("modal-open");
  }
}

function escapeHtml(text) {
  if (!text) return "";
  const map = {
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#039;",
  };
  return text.replace(/[&<>"']/g, function (m) {
    return map[m];
  });
}

function showAlert(type, message) {
  const alertsContainer = document.getElementById("alertsContainer");
  if (!alertsContainer) return;

  const alert = document.createElement("div");
  alert.className = `alert alert-${type} alert-dismissible fade show`;
  alert.innerHTML = `
    ${message}
    <button type="button" class="close" onclick="this.parentElement.remove()">
      <span>&times;</span>
    </button>
  `;

  alertsContainer.appendChild(alert);

  // Auto-remove after 5 seconds
  setTimeout(() => {
    if (alert.parentElement) {
      alert.remove();
    }
  }, 5000);
}

// Autocomplete for expression textarea with @ prefixes
class ExpressionAutocomplete {
  constructor(textareaElement) {
    this.textarea = textareaElement;
    this.dropdown = null;
    this.currentQuery = "";
    this.selectedIndex = -1;
    this.items = [];
    this.debounceTimeout = null;
    this.currentPrefix = "";
    this.currentPosition = -1;
    this.prefixStart = -1;

    // Define available autocomplete prefixes
    this.prefixes = {
      "@logo:": {
        name: "Logo",
        description: "Insert logo reference",
        keepPrefix: true, // Keep @logo: prefix in result
        searchFunction: this.searchLogos.bind(this),
        renderItem: this.renderLogoItem.bind(this),
        selectItem: this.selectItem.bind(this),
      },
      "@": {
        name: "Helper",
        description: "Available helpers",
        keepPrefix: false, // Replace @ with the helper value
        searchFunction: this.searchHelpers.bind(this),
        renderItem: this.renderHelperItem.bind(this),
        selectItem: this.selectItem.bind(this),
      },
    };

    this.setupTextarea();
  }

  setupTextarea() {
    // Create dropdown container
    this.dropdown = document.createElement("div");
    this.dropdown.className = "expression-autocomplete-dropdown";
    this.dropdown.style.display = "none";
    this.dropdown.style.position = "absolute";
    this.dropdown.style.zIndex = "1070";

    this.textarea.parentNode.style.position = "relative";
    this.textarea.parentNode.appendChild(this.dropdown);

    // Add event listeners
    this.textarea.addEventListener("input", (e) => {
      this.handleInput(e);
    });

    this.textarea.addEventListener("keydown", (e) => {
      this.handleKeydown(e);
    });

    this.textarea.addEventListener("blur", () => {
      setTimeout(() => this.hideDropdown(), 200);
    });

    this.textarea.addEventListener("click", (e) => {
      this.handleCursorPosition();
    });

    this.textarea.addEventListener("keyup", (e) => {
      // Handle cursor movement keys that don't trigger input events
      if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) {
        this.handleCursorPosition();
      }
    });

    this.textarea.addEventListener("focus", (e) => {
      // Check cursor position when field gains focus (e.g., tabbing in)
      setTimeout(() => this.handleCursorPosition(), 50);
    });
  }

  handleInput(e) {
    const cursorPos = this.textarea.selectionStart;
    const text = this.textarea.value;

    // Find any @ pattern before cursor
    const beforeCursor = text.substring(0, cursorPos);

    // Check for specific prefixes first (longest to shortest)
    const sortedPrefixes = Object.keys(this.prefixes).sort(
      (a, b) => b.length - a.length,
    );
    let matchFound = false;

    for (const prefix of sortedPrefixes) {
      if (prefix === "@") continue; // Handle @ last as fallback

      const escapedPrefix = prefix.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      const regex = new RegExp(`${escapedPrefix}([^"\\s]*)$`);
      const match = beforeCursor.match(regex);

      if (match) {
        const query = match[1];
        this.currentQuery = query;
        this.currentPrefix = prefix;
        this.prefixStart = cursorPos - match[0].length;
        this.currentPosition = cursorPos;

        clearTimeout(this.debounceTimeout);
        this.debounceTimeout = setTimeout(() => {
          this.search(query);
        }, 300);
        matchFound = true;
        break;
      }
    }

    // Check for lone @ as fallback
    if (!matchFound) {
      const atMatch = beforeCursor.match(/@([^"\\s]*)$/);
      if (atMatch && atMatch[0] === "@") {
        // Just @ typed, show available helpers
        this.currentQuery = "";
        this.currentPrefix = "@";
        this.prefixStart = cursorPos - 1;
        this.currentPosition = cursorPos;

        clearTimeout(this.debounceTimeout);
        this.debounceTimeout = setTimeout(() => {
          this.search("");
        }, 300);
        matchFound = true;
      }
    }

    if (!matchFound) {
      this.hideDropdown();
    }
  }

  handleCursorPosition() {
    const cursorPos = this.textarea.selectionStart;
    const text = this.textarea.value;
    const beforeCursor = text.substring(0, cursorPos);
    const afterCursor = text.substring(cursorPos);

    // Check if cursor is positioned at the end of or within a complete @logo:uuid pattern
    const logoUuidMatch = beforeCursor.match(
      /@logo:([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/i,
    );

    // Also check if we're at the end of a UUID (not followed by more UUID characters)
    if (logoUuidMatch) {
      const nextChar = afterCursor.charAt(0);
      // Only trigger if we're truly at the end (space, quote, newline, or end of text)
      if (!nextChar || /[\s"'\n\r]/.test(nextChar)) {
        const uuid = logoUuidMatch[1];
        this.currentQuery = uuid;
        this.currentPrefix = "@logo:";
        this.prefixStart = cursorPos - logoUuidMatch[0].length;
        this.currentPosition = cursorPos;

        // Show autocomplete immediately without debounce for existing UUIDs
        this.search(uuid);
      }
    }
  }

  async search(query) {
    const prefixConfig = this.prefixes[this.currentPrefix];
    if (!prefixConfig) {
      this.hideDropdown();
      return;
    }

    try {
      this.items = await prefixConfig.searchFunction(query);
      this.selectedIndex = -1;
      this.renderDropdown();
    } catch (error) {
      console.error(`Search failed for ${this.currentPrefix}:`, error);
      this.hideDropdown();
    }
  }

  async searchLogos(query) {
    // Check if query is a UUID (36 characters with dashes)
    const uuidPattern =
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

    if (query && uuidPattern.test(query)) {
      // Query is a UUID - fetch specific logo and show alternatives
      try {
        const [specificResponse, allResponse] = await Promise.all([
          fetch(`/api/logos/${encodeURIComponent(query)}/formats`),
          fetch(`/api/logos/search?limit=9`), // Get 9 other logos
        ]);

        const results = [];

        // Add the current UUID logo first (if found)
        if (specificResponse.ok) {
          const currentLogo = await specificResponse.json();
          results.push({
            type: "logo",
            name: currentLogo.name,
            url: `http://localhost:8080${currentLogo.url}`,
            value: currentLogo.name,
            uuid: currentLogo.id,
            insertValue: currentLogo.id,
            isCurrentUuid: true, // Flag to indicate this is the current UUID
          });
        }

        // Add other available logos
        if (allResponse.ok) {
          const allLogos = await allResponse.json();
          const otherLogos = allLogos.assets
            .filter((logo) => logo && logo.name && logo.id && logo.id !== query)
            .map((logo) => ({
              type: "logo",
              name: logo.name,
              url: logo.url,
              value: logo.name,
              uuid: logo.id,
              insertValue: logo.id,
            }));
          results.push(...otherLogos);
        }

        return results;
      } catch (error) {
        console.warn("Failed to fetch logo by UUID:", error);
      }
      // If UUID lookup fails, fall through to regular search
    }

    // Regular name-based search
    let url = `/api/logos/search?limit=10`;
    if (query && query.length > 0) {
      url += `&query=${encodeURIComponent(query)}`;
    }
    const response = await fetch(url);
    if (!response.ok) throw new Error("Logo search failed");
    const result = await response.json();
    return result.assets
      .filter((logo) => logo && logo.name && logo.id)
      .map((logo) => ({
        type: "logo",
        name: logo.name,
        url: logo.url,
        value: logo.name,
        uuid: logo.id,
        insertValue: logo.id, // Store UUID, not name
      }));
  }

  async searchHelpers(query) {
    // Return available helper prefixes
    const helpers = [
      {
        type: "helper",
        name: "logo:",
        description: "Insert logo reference by UUID",
        value: "logo:",
        insertValue: "@logo:", // What gets inserted when selected
      },
      // Future helpers following the same UUID pattern:
      // {
      //   type: 'helper',
      //   name: 'source:',
      //   description: 'Insert source reference by UUID (e.g., @source:uuid)',
      //   value: 'source:',
      //   insertValue: '@source:'
      // },
      // {
      //   type: 'helper',
      //   name: 'filter:',
      //   description: 'Insert filter reference by UUID (e.g., @filter:uuid)',
      //   value: 'filter:',
      //   insertValue: 'filter:'
      // },
      // {
      //   type: 'helper',
      //   name: 'field:',
      //   description: 'Insert validated field name (e.g., @field:channel_name)',
      //   value: 'field:',
      //   insertValue: 'field:'
      // },
      // {
      //   type: 'helper',
      //   name: 'regex:',
      //   description: 'Insert predefined regex pattern (e.g., @regex:email)',
      //   value: 'regex:',
      //   insertValue: 'regex:'
      // }
    ];

    if (query.length === 0) {
      return helpers;
    } else {
      return helpers.filter(
        (helper) =>
          helper.name.toLowerCase().includes(query.toLowerCase()) ||
          helper.description.toLowerCase().includes(query.toLowerCase()),
      );
    }
  }

  renderDropdown() {
    const prefixConfig = this.prefixes[this.currentPrefix];
    if (!prefixConfig) return;

    if (this.items.length === 0) {
      this.dropdown.innerHTML = `
        <div class="autocomplete-item no-results" style="padding: 8px 12px; color: #666;">
          No ${prefixConfig.name.toLowerCase()} found for "${escapeHtml(this.currentQuery)}"
        </div>
      `;
    } else {
      this.dropdown.innerHTML = this.items
        .map((item, index) =>
          prefixConfig.renderItem(item, index, this.selectedIndex),
        )
        .join("");

      // Add click handlers
      this.dropdown
        .querySelectorAll(".expression-autocomplete-item")
        .forEach((item, index) => {
          item.addEventListener("click", () => {
            const value = item.dataset.value;
            const insertValue = item.dataset.insertValue || value;
            if (value) {
              prefixConfig.selectItem(insertValue, value);
            }
          });
        });
    }

    this.positionDropdown();
    this.showDropdown();
  }

  renderLogoItem(item, index, selectedIndex) {
    const isCurrentUuid = item.isCurrentUuid;
    const currentUuidBadge = isCurrentUuid
      ? '<span class="current-uuid-badge">CURRENT</span>'
      : "";

    return `
      <div class="expression-autocomplete-item${index === selectedIndex ? " selected" : ""}"
           data-index="${index}"
           data-value="${escapeHtml(item.value)}"
           data-insert-value="${escapeHtml(item.insertValue)}"
           data-uuid="${escapeHtml(item.uuid)}"
           data-current-uuid="${isCurrentUuid ? "true" : "false"}">
        <img src="${item.url}" alt="${escapeHtml(item.name)}" style="width: 24px; height: 24px; object-fit: contain; border-radius: 2px;">
        <div style="flex: 1;">
          <div style="font-weight: 500;">${escapeHtml(item.name)}${currentUuidBadge}</div>
          <div style="font-size: 0.75em; opacity: 0.7; font-family: monospace;">UUID: ${escapeHtml(item.uuid.substring(0, 8))}...</div>
        </div>
      </div>
    `;
  }

  renderHelperItem(item, index, selectedIndex) {
    return `
      <div class="expression-autocomplete-item${index === selectedIndex ? " selected" : ""}"
           data-index="${index}"
           data-value="${escapeHtml(item.value)}"
           data-insert-value="${escapeHtml(item.insertValue || item.value)}">
        <span style="font-family: monospace; background: rgba(0,0,0,0.1); padding: 2px 6px; border-radius: 3px; font-size: 0.9em;">@${escapeHtml(item.name)}</span>
        <div style="flex: 1;">
          <div style="font-weight: 500;">${escapeHtml(item.name)}</div>
          <div style="font-size: 0.85em; opacity: 0.8;">${escapeHtml(item.description)}</div>
        </div>
      </div>
    `;
  }

  positionDropdown() {
    // Position dropdown near the cursor position
    const rect = this.textarea.getBoundingClientRect();
    const parentRect = this.textarea.parentNode.getBoundingClientRect();

    // Calculate cursor position more accurately
    const cursorPos = this.textarea.selectionStart;
    const textBeforeCursor = this.textarea.value.substring(0, cursorPos);
    const lines = textBeforeCursor.split("\n");
    const currentLine = lines.length - 1;
    const currentColumn = lines[lines.length - 1].length;

    // Get computed styles for more accurate measurements
    const computedStyle = window.getComputedStyle(this.textarea);
    const fontSize = parseFloat(computedStyle.fontSize);
    const lineHeight = parseFloat(computedStyle.lineHeight) || fontSize * 1.2;
    const paddingLeft = parseFloat(computedStyle.paddingLeft) || 0;
    const paddingTop = parseFloat(computedStyle.paddingTop) || 0;

    // Approximate character width based on font size (monospace assumption)
    const charWidth = fontSize * 0.6;

    // Calculate position within textarea
    const cursorX = Math.min(
      currentColumn * charWidth + paddingLeft,
      rect.width - 200,
    );
    const cursorY = currentLine * lineHeight + paddingTop;

    // Position dropdown
    const dropdownTop = rect.top - parentRect.top + cursorY + lineHeight + 5;
    const dropdownLeft = rect.left - parentRect.left + cursorX;

    // Ensure dropdown doesn't go off-screen
    const maxLeft = window.innerWidth - 320; // 300px width + 20px margin
    const maxTop = window.innerHeight - 200; // Approximate dropdown height

    this.dropdown.style.top = Math.min(dropdownTop, maxTop) + "px";
    this.dropdown.style.left = Math.min(dropdownLeft, maxLeft) + "px";
    this.dropdown.style.maxWidth = "300px";
    this.dropdown.style.zIndex = "1000";
  }

  handleKeydown(e) {
    if (
      !this.dropdown.style.display ||
      this.dropdown.style.display === "none"
    ) {
      return;
    }

    const items = this.dropdown.querySelectorAll(
      ".expression-autocomplete-item:not(.no-results)",
    );

    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        this.selectedIndex = Math.min(this.selectedIndex + 1, items.length - 1);
        this.updateSelection();
        break;

      case "ArrowUp":
        e.preventDefault();
        this.selectedIndex = Math.max(this.selectedIndex - 1, -1);
        this.updateSelection();
        break;

      case "Enter":
      case "Tab":
        e.preventDefault();
        if (this.selectedIndex >= 0 && items[this.selectedIndex]) {
          const item = items[this.selectedIndex];
          const value = item.dataset.value;
          const insertValue = item.dataset.insertValue || value;
          if (value) {
            const prefixConfig = this.prefixes[this.currentPrefix];
            if (prefixConfig) {
              prefixConfig.selectItem(insertValue, value);
            }
          }
        }
        break;

      case "Escape":
        this.hideDropdown();
        break;
    }
  }

  updateSelection() {
    const items = this.dropdown.querySelectorAll(
      ".expression-autocomplete-item",
    );
    items.forEach((item, index) => {
      if (index === this.selectedIndex) {
        item.style.backgroundColor = "#007bff";
        item.style.color = "white";
        item.classList.add("selected");
      } else {
        item.style.backgroundColor = "transparent";
        item.style.color = "inherit";
        item.classList.remove("selected");
      }
    });
  }

  selectItem(insertValue, originalValue) {
    const text = this.textarea.value;
    const beforePrefix = text.substring(0, this.prefixStart);
    const afterCursor = text.substring(this.currentPosition);
    const prefixConfig = this.prefixes[this.currentPrefix];

    let newText, newCursorPos;

    if (prefixConfig.keepPrefix) {
      // Keep the prefix (e.g., @logo:uuid)
      if (this.currentPrefix === "@logo:") {
        newText = beforePrefix + "@logo:" + insertValue + afterCursor;
        newCursorPos = this.prefixStart + "@logo:".length + insertValue.length;
      } else {
        newText = beforePrefix + this.currentPrefix + insertValue + afterCursor;
        newCursorPos =
          this.prefixStart + this.currentPrefix.length + insertValue.length;
      }
    } else {
      // Replace the prefix entirely (e.g., @ becomes logo:)
      newText = beforePrefix + insertValue + afterCursor;
      newCursorPos = this.prefixStart + insertValue.length;
    }

    this.textarea.value = newText;
    this.textarea.setSelectionRange(newCursorPos, newCursorPos);

    this.hideDropdown();

    // Trigger change event for validation
    this.textarea.dispatchEvent(new Event("input"));
  }

  showDropdown() {
    this.dropdown.style.display = "block";
  }

  hideDropdown() {
    this.dropdown.style.display = "none";
    this.selectedIndex = -1;
  }
}

// Original Logo autocomplete functionality for dedicated inputs
class LogoAutocomplete {
  constructor(inputElement, hiddenInputElement) {
    this.input = inputElement;
    this.hiddenInput = hiddenInputElement;
    this.dropdown = null;
    this.currentQuery = "";
    this.selectedIndex = -1;
    this.logos = [];
    this.debounceTimeout = null;

    this.setupInput();
  }

  setupInput() {
    // Create dropdown container
    this.dropdown = document.createElement("div");
    this.dropdown.className = "logo-autocomplete-dropdown";
    this.dropdown.style.display = "none";
    this.input.parentNode.appendChild(this.dropdown);

    // Add event listeners
    this.input.addEventListener("input", (e) => {
      this.debouncedSearch(e.target.value);
    });

    this.input.addEventListener("keydown", (e) => {
      this.handleKeydown(e);
    });

    this.input.addEventListener("blur", () => {
      // Delay hiding to allow click selection
      setTimeout(() => this.hideDropdown(), 200);
    });

    this.input.addEventListener("focus", () => {
      if (this.input.value.trim()) {
        this.search(this.input.value.trim());
      }
    });
  }

  debouncedSearch(query) {
    clearTimeout(this.debounceTimeout);
    this.debounceTimeout = setTimeout(() => {
      this.search(query);
    }, 300);
  }

  async search(query) {
    if (query.length < 2) {
      this.hideDropdown();
      return;
    }

    this.currentQuery = query;

    try {
      const response = await fetch(
        `/api/logos/search?query=${encodeURIComponent(query)}&limit=10`,
      );
      if (!response.ok) throw new Error("Search failed");

      const result = await response.json();
      this.logos = result.assets;
      this.selectedIndex = -1;
      this.renderDropdown();
    } catch (error) {
      console.error("Logo search failed:", error);
      this.hideDropdown();
    }
  }

  renderDropdown() {
    if (this.logos.length === 0) {
      this.dropdown.innerHTML = `
        <div class="logo-autocomplete-item no-results">
          No logos found for "${escapeHtml(this.currentQuery)}"
        </div>
      `;
    } else {
      this.dropdown.innerHTML = this.logos
        .map(
          (logo, index) => `
        <div class="logo-autocomplete-item${index === this.selectedIndex ? " selected" : ""}"
             data-index="${index}"
             data-logo-id="${logo.asset.id}"
             data-logo-name="${escapeHtml(logo.asset.name)}">
          <img src="${logo.url}" alt="${escapeHtml(logo.asset.name)}" class="logo-preview-tiny">
          <span class="logo-name">${escapeHtml(logo.asset.name)}</span>
        </div>
      `,
        )
        .join("");

      // Add click handlers
      this.dropdown
        .querySelectorAll(".logo-autocomplete-item")
        .forEach((item) => {
          item.addEventListener("click", () => {
            const logoId = item.dataset.logoId;
            const logoName = item.dataset.logoName;
            if (logoId && logoName) {
              this.selectLogo(logoId, logoName);
            }
          });
        });
    }

    this.showDropdown();
  }

  handleKeydown(e) {
    if (
      !this.dropdown.style.display ||
      this.dropdown.style.display === "none"
    ) {
      return;
    }

    const items = this.dropdown.querySelectorAll(
      ".logo-autocomplete-item:not(.no-results)",
    );

    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        this.selectedIndex = Math.min(this.selectedIndex + 1, items.length - 1);
        this.updateSelection();
        break;

      case "ArrowUp":
        e.preventDefault();
        this.selectedIndex = Math.max(this.selectedIndex - 1, -1);
        this.updateSelection();
        break;

      case "Enter":
        e.preventDefault();
        if (this.selectedIndex >= 0 && items[this.selectedIndex]) {
          const item = items[this.selectedIndex];
          const logoId = item.dataset.logoId;
          const logoName = item.dataset.logoName;
          if (logoId && logoName) {
            this.selectLogo(logoId, logoName);
          }
        }
        break;

      case "Escape":
        this.hideDropdown();
        break;
    }
  }

  updateSelection() {
    const items = this.dropdown.querySelectorAll(".logo-autocomplete-item");
    items.forEach((item, index) => {
      item.classList.toggle("selected", index === this.selectedIndex);
    });
  }

  selectLogo(logoId, logoName) {
    this.input.value = logoName;
    this.hiddenInput.value = logoId;
    this.hideDropdown();

    // Trigger change event for validation
    this.input.dispatchEvent(new Event("change"));
  }

  showDropdown() {
    this.dropdown.style.display = "block";
  }

  hideDropdown() {
    this.dropdown.style.display = "none";
  }

  clear() {
    this.input.value = "";
    this.hiddenInput.value = "";
    this.hideDropdown();
  }
}

// Drag and drop functionality
let draggedElement = null;
let draggedElementId = null;

function enableDragAndDrop() {
  const dragHandles = document.querySelectorAll(".drag-handle");
  dragHandles.forEach((handle) => {
    handle.addEventListener("dragstart", handleDragStart);
    handle.addEventListener("dragend", handleDragEnd);
  });

  const ruleItems = document.querySelectorAll(".rule-card");
  ruleItems.forEach((item) => {
    item.addEventListener("dragover", handleDragOver);
    item.addEventListener("drop", handleDrop);
    item.addEventListener("dragenter", handleDragEnter);
    item.addEventListener("dragleave", handleDragLeave);
  });

  // Add global drag over handler for the rules container
  const rulesContainer = document.getElementById("rulesContainer");
  if (rulesContainer) {
    rulesContainer.addEventListener("dragover", handleContainerDragOver);
    rulesContainer.addEventListener("dragleave", handleContainerDragLeave);
  }
}

function handleDragStart(e) {
  draggedElement = this.closest(".rule-card");
  draggedElementId = draggedElement.dataset.ruleId;
  draggedElement.classList.add("dragging");
  e.dataTransfer.effectAllowed = "move";
  e.dataTransfer.setData("text/html", draggedElement.outerHTML);
}

function handleDragOver(e) {
  if (e.preventDefault) {
    e.preventDefault();
  }
  e.dataTransfer.dropEffect = "move";

  // Show insertion indicator based on mouse position
  if (this !== draggedElement) {
    showInsertionIndicator(this, e);
  }

  return false;
}

function handleDragEnter(e) {
  if (this !== draggedElement) {
    this.classList.add("drag-over");
  }
}

function handleDragLeave(e) {
  // Only remove drag-over class if we're actually leaving the element
  // (not just moving to a child element)
  const rect = this.getBoundingClientRect();
  const x = e.clientX;
  const y = e.clientY;

  if (x < rect.left || x > rect.right || y < rect.top || y > rect.bottom) {
    this.classList.remove("drag-over");
  }
}

function handleContainerDragOver(e) {
  if (e.preventDefault) {
    e.preventDefault();
  }
  e.dataTransfer.dropEffect = "move";

  // Find the closest rule card to show insertion indicator
  const mouseY = e.clientY;
  const ruleCards = Array.from(this.querySelectorAll(".rule-card")).filter(
    (card) => card !== draggedElement,
  );

  if (ruleCards.length === 0) return;

  let closestCard = null;
  let closestDistance = Infinity;
  let insertBefore = true;

  ruleCards.forEach((card) => {
    const rect = card.getBoundingClientRect();
    const cardTop = rect.top;
    const cardBottom = rect.bottom;
    const cardMiddle = rect.top + rect.height / 2;

    // Calculate distance to top edge (with extended zone)
    const distanceToTop = Math.abs(mouseY - cardTop);
    // Calculate distance to bottom edge (with extended zone)
    const distanceToBottom = Math.abs(mouseY - cardBottom);

    // Extended zones: 20px above and below each card
    const extendedTop = cardTop - 20;
    const extendedBottom = cardBottom + 20;

    // Check if mouse is in the extended zone of this card
    if (mouseY >= extendedTop && mouseY <= extendedBottom) {
      if (mouseY < cardMiddle) {
        // Closer to top - insert before
        if (distanceToTop < closestDistance) {
          closestDistance = distanceToTop;
          closestCard = card;
          insertBefore = true;
        }
      } else {
        // Closer to bottom - insert after
        if (distanceToBottom < closestDistance) {
          closestDistance = distanceToBottom;
          closestCard = card;
          insertBefore = false;
        }
      }
    }
  });

  if (closestCard) {
    showInsertionIndicatorAtCard(closestCard, insertBefore);
  }

  return false;
}

function handleContainerDragLeave(e) {
  // Check if we're leaving the container entirely
  const rect = this.getBoundingClientRect();
  const x = e.clientX;
  const y = e.clientY;

  if (x < rect.left || x > rect.right || y < rect.top || y > rect.bottom) {
    hideInsertionIndicator();
  }
}

function showInsertionIndicator(element, e) {
  const rect = element.getBoundingClientRect();
  const mouseY = e.clientY;
  const elementMiddle = rect.top + rect.height / 2;

  // Extended zones for better UX
  const extendedTopZone = rect.top - 15;
  const extendedBottomZone = rect.bottom + 15;

  let insertBefore = false;

  if (
    mouseY <= extendedTopZone ||
    (mouseY < elementMiddle && mouseY >= extendedTopZone)
  ) {
    insertBefore = true;
  } else if (
    mouseY >= extendedBottomZone ||
    (mouseY >= elementMiddle && mouseY <= extendedBottomZone)
  ) {
    insertBefore = false;
  } else {
    // We're in the middle area, don't show indicator to avoid confusion
    hideInsertionIndicator();
    return;
  }

  showInsertionIndicatorAtCard(element, insertBefore);
}

function showInsertionIndicatorAtCard(element, insertBefore) {
  // Remove existing indicator
  hideInsertionIndicator();

  const indicator = document.createElement("div");
  indicator.className = "drag-insertion-indicator";
  indicator.id = "drag-indicator";

  if (insertBefore) {
    // Insert before this element
    element.parentNode.insertBefore(indicator, element);
  } else {
    // Insert after this element
    element.parentNode.insertBefore(indicator, element.nextSibling);
  }
}

function hideInsertionIndicator() {
  const indicator = document.getElementById("drag-indicator");
  if (indicator) {
    indicator.remove();
  }
}

function handleDrop(e) {
  if (e.stopPropagation) {
    e.stopPropagation();
  }

  if (this !== draggedElement) {
    const dropTargetId = this.dataset.ruleId;
    const rect = this.getBoundingClientRect();
    const mouseY = e.clientY;
    const elementMiddle = rect.top + rect.height / 2;

    // Extended zones for drop detection
    const extendedTopZone = rect.top - 15;
    const extendedBottomZone = rect.bottom + 15;

    let insertBefore = false;

    if (
      mouseY <= extendedTopZone ||
      (mouseY < elementMiddle && mouseY >= extendedTopZone)
    ) {
      insertBefore = true;
    } else if (
      mouseY >= extendedBottomZone ||
      (mouseY >= elementMiddle && mouseY <= extendedBottomZone)
    ) {
      insertBefore = false;
    } else {
      // Default to inserting after if in middle zone
      insertBefore = false;
    }

    reorderRules(draggedElementId, dropTargetId, insertBefore);
  }

  return false;
}

function handleDragEnd(e) {
  const ruleItems = document.querySelectorAll(".rule-card");
  ruleItems.forEach((item) => {
    item.classList.remove("dragging", "drag-over");
  });

  hideInsertionIndicator();
  draggedElement = null;
  draggedElementId = null;
}

async function reorderRules(draggedRuleId, targetRuleId, insertBefore = false) {
  try {
    // Find the rules in the current array
    const draggedRule = currentRules.find((r) => r.id === draggedRuleId);
    const targetRule = currentRules.find((r) => r.id === targetRuleId);

    if (!draggedRule || !targetRule) return;

    // Create new order array
    const newOrder = [];
    const draggedIndex = currentRules.findIndex((r) => r.id === draggedRuleId);
    const targetIndex = currentRules.findIndex((r) => r.id === targetRuleId);

    // Create a copy without the dragged item
    const filteredRules = currentRules.filter((r) => r.id !== draggedRuleId);

    // Calculate insertion position based on insertBefore flag
    let insertPosition;
    if (insertBefore) {
      insertPosition =
        targetIndex > draggedIndex ? targetIndex - 1 : targetIndex;
    } else {
      insertPosition =
        targetIndex > draggedIndex ? targetIndex : targetIndex + 1;
    }

    // Insert dragged item at calculated position
    filteredRules.splice(insertPosition, 0, draggedRule);

    // Update sort_order for all rules
    filteredRules.forEach((rule, index) => {
      rule.sort_order = index + 1;
      newOrder.push([rule.id, rule.sort_order]);
    });

    // Send reorder request to server
    const response = await fetch("/api/data-mapping/reorder", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(newOrder),
    });

    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }

    // Update local state and re-render
    currentRules = filteredRules;
    renderRules();

    showAlert("success", "Rules reordered successfully");
  } catch (error) {
    console.error("Failed to reorder rules:", error);
    showAlert("error", "Failed to reorder rules");
    // Reload rules to reset to server state
    await loadRules();
  }
}

// Initialize when DOM is loaded
document.addEventListener("DOMContentLoaded", init);
