// Logo Management JavaScript

let currentLogos = [];
let currentPage = 1;
let totalPages = 1;
let editingLogo = null;
let searchQuery = "";
let typeFilter = "";

// Initialize page
function initializeLogosPage() {
  console.log("Initializing logos page..."); // Debug log
  loadLogos();
  loadStats();

  // Setup file input change handler
  const fileInput = document.getElementById("logoFile");
  if (fileInput) {
    fileInput.addEventListener("change", handleFilePreview);
  }

  // Setup page-level drag and drop
  setupPageLevelDragAndDrop();

  // Setup standard modal close handlers
  SharedUtils.setupStandardModalCloseHandlers("uploadModal");
  SharedUtils.setupStandardModalCloseHandlers("editModal");
}

// Check if DOM is already loaded, if so initialize immediately
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", initializeLogosPage);
} else {
  // DOM is already loaded, initialize immediately
  initializeLogosPage();
}

// Load logo assets
async function loadLogos(page = 1) {
  console.log("Loading logos, page:", page); // Debug log
  console.log("Current searchQuery:", searchQuery, "typeFilter:", typeFilter); // Debug log
  currentPage = page;

  try {
    const params = new URLSearchParams({
      page: page.toString(),
      limit: "20",
    });

    if (searchQuery) {
      params.append("search", searchQuery);
    }

    if (typeFilter === "uploaded") {
      params.append("include_cached", "false");
    }

    const url = `/api/v1/logos?${params}&_t=${new Date().getTime()}`;
    console.log("Fetching URL:", url); // Debug log

    const response = await fetch(url);
    console.log("Logo API response status:", response.status); // Debug log

    if (!response.ok) {
      const errorText = await response.text();
      console.error("API Error Response:", errorText);
      throw new Error(`Failed to load logos: ${response.status} ${errorText}`);
    }

    const data = await response.json();
    console.log("Logo API response data:", data); // Debug log
    console.log(
      "Logo data assets count:",
      data.assets ? data.assets.length : "NO ASSETS PROPERTY",
    ); // Debug log

    if (!data.assets) {
      console.error("No assets property in response data:", data);
      currentLogos = [];
    } else {
      currentLogos = data.assets;
    }

    totalPages = data.total_pages || 1;
    console.log("Set currentLogos count:", currentLogos.length); // Debug log
    console.log("currentLogos array:", currentLogos); // Debug log

    // Initialize linked assets for each logo (no additional API calls needed)
    console.log("Initializing logo data..."); // Debug log
    currentLogos.forEach((logo) => {
      // Initialize empty arrays if not present
      logo.linked_assets = logo.linked_assets || [];
      logo.available_formats = logo.available_formats || [];
    });
    console.log("Logo data initialized successfully"); // Debug log

    console.log("About to render logos..."); // Debug log
    renderLogos();
    renderPagination();
    console.log("Logos rendered"); // Debug log
  } catch (error) {
    console.error("Error loading logos:", error);
    console.error("Error stack:", error.stack);
    // Ensure currentLogos is still a valid array even if API fails
    currentLogos = [];
    totalPages = 1;
    renderLogos(); // Render empty state
    showError(`Failed to load logos: ${error.message}`);
  }
}

async function loadLinkedAssetsForLogos() {
  console.log(
    "loadLinkedAssetsForLogos called with",
    currentLogos.length,
    "logos",
  );

  if (!currentLogos || currentLogos.length === 0) {
    console.log("No logos to load linked assets for");
    return;
  }

  // Initialize linked assets data from main API response
  currentLogos.forEach((logo, index) => {
    console.log(
      `Initializing linked assets for logo ${index}: ${logo.id} (${logo.name})`,
    );

    // Initialize empty arrays if not present - data will come from main API or separate calls when needed
    logo.linked_assets = logo.linked_assets || [];
    logo.available_formats = logo.available_formats || [];

    console.log(
      `Linked assets initialized for ${logo.id}:`,
      logo.linked_assets.length,
      "linked,",
      logo.available_formats.length,
      "formats",
    );
  });

  console.log("All linked assets initialized");
}

// Load cache statistics
async function loadStats() {
  try {
    const response = await fetch("/api/v1/logos/stats");
    if (!response.ok) throw new Error("Failed to load stats");

    const stats = await response.json();
    renderStats(stats);
  } catch (error) {
    console.error("Error loading stats:", error);
    // Show fallback stats instead of hiding
    const container = document.getElementById("statsContainer");
    container.innerHTML = `
        <strong>Logos:</strong> Loading... •
        <strong>Storage:</strong> Loading... •
        <strong>Linked:</strong> +0
    `;
  }
}

// Render statistics
function renderStats(stats) {
  const container = document.getElementById("statsContainer");
  const totalLogos = stats.total_uploaded_logos + stats.total_cached_logos;
  const totalLinkedAssets = stats.total_linked_assets || 0;

  container.innerHTML = `
        <strong>Logos:</strong> ${totalLogos}
        (${stats.total_uploaded_logos} uploaded, ${stats.total_cached_logos} cached) •
        <strong>Storage:</strong> ${formatFileSize(stats.total_storage_used)} •
        <strong>Linked:</strong> +${totalLinkedAssets}
    `;
}

// Render logos grid
function renderLogos() {
  console.log("renderLogos called"); // Debug log
  const container = document.getElementById("logosContainer");

  if (!container) {
    console.error("logosContainer element not found!");
    return;
  }

  console.log("renderLogos: currentLogos:", currentLogos); // Debug log
  console.log(
    "renderLogos: currentLogos type:",
    typeof currentLogos,
    "isArray:",
    Array.isArray(currentLogos),
  ); // Debug log

  // Ensure currentLogos is an array
  if (!Array.isArray(currentLogos)) {
    console.error(
      "currentLogos is not an array:",
      currentLogos,
      "type:",
      typeof currentLogos,
    );
    currentLogos = [];
  }

  console.log(
    "renderLogos: After validation, currentLogos.length:",
    currentLogos.length,
  ); // Debug log

  if (currentLogos.length === 0) {
    console.log("Rendering empty state"); // Debug log
    container.innerHTML = `
            <div class="empty-state">
                <h3>No Logos Found</h3>
                <p>${searchQuery || typeFilter ? "No logos match your search criteria" : "Upload your first logo to get started"}</p>
            </div>
        `;
    return;
  }

  console.log("Rendering", currentLogos.length, "logos"); // Debug log

  let html = '<div class="logos-grid">';

  currentLogos.forEach((logo) => {
    if (!logo || typeof logo !== "object" || !logo.id) return; // Skip if logo is null/undefined, not an object, or has no id

    // Provide defaults for missing properties
    const assetType = logo.asset_type || "uploaded";
    const typeLabel = assetType === "uploaded" ? "uploaded" : "cached";
    const logoName = logo.name || "Unnamed Logo";
    const logoUrl = logo.url || "#";
    const fileSize = logo.file_size || 0;

    // Extract format from mime_type or file extension
    let format = "unknown";
    if (logo.mime_type) {
      if (logo.mime_type.includes("svg")) format = "svg";
      else if (logo.mime_type.includes("png")) format = "png";
      else if (
        logo.mime_type.includes("jpeg") ||
        logo.mime_type.includes("jpg")
      )
        format = "jpg";
      else if (logo.mime_type.includes("gif")) format = "gif";
      else if (logo.mime_type.includes("webp")) format = "webp";
    } else if (logo.file_name) {
      const ext = logo.file_name.split(".").pop()?.toLowerCase();
      if (ext) format = ext;
    }

    // Get available formats from linked assets
    const availableFormats = [format];
    if (logo.linked_assets && logo.linked_assets.length > 0) {
      logo.linked_assets.forEach((linked) => {
        let linkedFormat = "unknown";
        if (linked.mime_type) {
          if (linked.mime_type.includes("svg")) linkedFormat = "svg";
          else if (linked.mime_type.includes("png")) linkedFormat = "png";
          else if (
            linked.mime_type.includes("jpeg") ||
            linked.mime_type.includes("jpg")
          )
            linkedFormat = "jpg";
          else if (linked.mime_type.includes("gif")) linkedFormat = "gif";
          else if (linked.mime_type.includes("webp")) linkedFormat = "webp";
        } else if (linked.file_name) {
          const ext = linked.file_name.split(".").pop()?.toLowerCase();
          if (ext) linkedFormat = ext;
        }
        if (!availableFormats.includes(linkedFormat)) {
          availableFormats.push(linkedFormat);
        }
      });
    }

    // Color coding for badges
    const sizeColor = "#6c757d"; // grey
    const typeColor = assetType === "uploaded" ? "#007bff" : "#28a745"; // blue for uploaded, green for cached

    html += `
            <div class="logo-card" onclick="editLogoAsset('${logo.id}')">
                <div class="logo-actions">
                    <button class="btn btn-sm btn-outline-danger" onclick="event.stopPropagation(); deleteLogo('${logo.id}')" title="Delete">
                        🗑️
                    </button>
                </div>

                <img class="logo-preview" src="${logoUrl}" alt="${escapeHtml(logoName)}"
                     onerror="this.src='data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMTAwIiBoZWlnaHQ9IjEwMCIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj48cmVjdCB3aWR0aD0iMTAwIiBoZWlnaHQ9IjEwMCIgZmlsbD0iI2YwZjBmMCIvPjx0ZXh0IHg9IjUwIiB5PSI1NSIgZm9udC1mYW1pbHk9IkFyaWFsIiBmb250LXNpemU9IjE0IiBmaWxsPSIjOTk5IiB0ZXh0LWFuY2hvcj0ibWlkZGxlIj5ObyBJbWFnZTwvdGV4dD48L3N2Zz4='">

                <div class="logo-info">
                    <div class="logo-name" style="font-weight: 500; margin-bottom: 2px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">${escapeHtml(logoName)}</div>
                    <div class="logo-meta" style="font-size: 0.75rem; display: flex; gap: 4px; align-items: center; overflow: hidden; flex-wrap: wrap;">
                        <span class="badge" style="background-color: ${sizeColor}; color: white; font-size: 0.65rem; padding: 2px 6px; border-radius: 10px;">${formatFileSize(fileSize)}</span>
                        ${availableFormats
                          .map((fmt) => {
                            const formatColor =
                              fmt === "png" ? "#fd7e14" : "#ffc107"; // orange for png, yellow for other formats
                            return `<span class="badge" style="background-color: ${formatColor}; color: white; font-size: 0.65rem; padding: 2px 6px; border-radius: 10px;">${fmt}</span>`;
                          })
                          .join("")}
                        <span class="badge" style="background-color: ${typeColor}; color: white; font-size: 0.65rem; padding: 2px 6px; border-radius: 10px;">${typeLabel}</span>
                    </div>
                </div>
            </div>
        `;
  });

  html += "</div>";
  container.innerHTML = html;
}

// Render pagination
function renderPagination() {
  const container = document.getElementById("paginationContainer");

  if (totalPages <= 1) {
    container.innerHTML = "";
    return;
  }

  let html = '<nav><ul class="pagination justify-content-center">';

  // Previous button
  html += `
        <li class="page-item ${currentPage === 1 ? "disabled" : ""}">
            <button class="page-link" onclick="loadLogos(${currentPage - 1})" ${currentPage === 1 ? "disabled" : ""}>
                Previous
            </button>
        </li>
    `;

  // Page numbers
  const startPage = Math.max(1, currentPage - 2);
  const endPage = Math.min(totalPages, currentPage + 2);

  if (startPage > 1) {
    html += `<li class="page-item"><button class="page-link" onclick="loadLogos(1)">1</button></li>`;
    if (startPage > 2) {
      html += `<li class="page-item disabled"><span class="page-link">...</span></li>`;
    }
  }

  for (let i = startPage; i <= endPage; i++) {
    html += `
            <li class="page-item ${i === currentPage ? "active" : ""}">
                <button class="page-link" onclick="loadLogos(${i})">${i}</button>
            </li>
        `;
  }

  if (endPage < totalPages) {
    if (endPage < totalPages - 1) {
      html += `<li class="page-item disabled"><span class="page-link">...</span></li>`;
    }
    html += `<li class="page-item"><button class="page-link" onclick="loadLogos(${totalPages})">${totalPages}</button></li>`;
  }

  // Next button
  html += `
        <li class="page-item ${currentPage === totalPages ? "disabled" : ""}">
            <button class="page-link" onclick="loadLogos(${currentPage + 1})" ${currentPage === totalPages ? "disabled" : ""}>
                Next
            </button>
        </li>
    `;

  html += "</ul></nav>";
  container.innerHTML = html;
}

// Search logos
function searchLogos() {
  searchQuery = document.getElementById("logoSearchInput").value.trim();
  loadLogos(1);
}

// Filter logos by type
function filterLogos() {
  const includeCached = document.getElementById("includeCachedLogos").checked;
  typeFilter = includeCached ? "" : "uploaded";
  loadLogos(1);
}

// Refresh stats (removed unnecessary popup)
function refreshStats() {
  loadStats();
}

// Upload logo (placeholder)
function uploadLogo() {
  // Check if this is being called from the button click to submit, or to open modal
  const uploadBtn = document.querySelector(
    "#uploadModal .modal-footer .btn-primary",
  );
  const modal = document.getElementById("uploadModal");

  // If modal is visible, this is a submit action
  if (
    modal &&
    modal.style.display !== "none" &&
    modal.classList.contains("show")
  ) {
    submitUpload();
    return;
  }

  // Otherwise, open the modal
  clearSelectedFile();
  document.getElementById("logoName").value = "";
  document.getElementById("logoDescription").value = "";

  // Setup drag and drop when modal opens
  setupDragAndDrop();

  // Disable upload button initially
  disableUploadButton();

  // Show modal using standard utilities
  SharedUtils.showStandardModal("uploadModal");
}

// Close upload modal
function closeUploadModal() {
  SharedUtils.hideStandardModal("uploadModal");
  document.getElementById("uploadForm").reset();
  document.getElementById("uploadPreview").style.display = "none";
  disableUploadButton();
}

// Enable upload button
function enableUploadButton() {
  const uploadBtn = document.querySelector(
    "#uploadModal .modal-footer .btn-primary",
  );
  if (uploadBtn) {
    uploadBtn.disabled = false;
    uploadBtn.style.cursor = "pointer";
  }
}

// Disable upload button
function disableUploadButton() {
  const uploadBtn = document.querySelector(
    "#uploadModal .modal-footer .btn-primary",
  );
  if (uploadBtn) {
    uploadBtn.disabled = true;
    uploadBtn.style.cursor = "not-allowed";
  }
}

// Submit upload form
async function submitUpload() {
  const form = document.getElementById("uploadForm");
  const fileInput = document.getElementById("logoFile");
  const nameInput = document.getElementById("logoName");

  // Get file from input or dropped file
  const file = fileInput.files[0] || fileInput._droppedFile;
  if (!file) {
    showError("Please select a file to upload");
    return;
  }

  if (!nameInput.value.trim()) {
    showError("Please enter a logo name");
    nameInput.focus();
    return;
  }

  const formData = new FormData();
  formData.append("file", file);
  formData.append("name", nameInput.value.trim());
  formData.append(
    "description",
    document.getElementById("logoDescription").value.trim(),
  );

  const uploadBtn = document.querySelector(
    "#uploadModal .modal-footer .btn-primary",
  );
  const originalText = uploadBtn.textContent;
  uploadBtn.textContent = "Uploading...";
  uploadBtn.disabled = true;

  try {
    const response = await fetch("/api/v1/logos/upload", {
      method: "POST",
      body: formData,
    });

    if (!response.ok) {
      const error = await response.text();
      throw new Error(error || `HTTP error! status: ${response.status}`);
    }

    const result = await response.json();
    showSuccess(`Logo "${result.name}" uploaded successfully`);
    closeUploadModal();
    loadLogos(); // Refresh the logos list
  } catch (error) {
    console.error("Upload failed:", error);
    showError(`Upload failed: ${error.message}`);
  } finally {
    uploadBtn.textContent = originalText;
    enableUploadButton();
  }
}

// Handle file preview
function handleFilePreview(event) {
  const file = event.target.files[0];
  const uploadArea = document.getElementById("fileUploadArea");

  if (!file) {
    clearSelectedFile();
    return;
  }

  // Validate file type
  if (!file.type.startsWith("image/")) {
    showError("Please select an image file");
    event.target.value = "";
    clearSelectedFile();
    return;
  }

  // Validate file size (10MB max)
  if (file.size > 10 * 1024 * 1024) {
    showError("File size must be less than 10MB");
    event.target.value = "";
    clearSelectedFile();
    return;
  }

  // Update upload area to show selected file
  uploadArea.classList.add("has-file");
  document.getElementById("selectedFileName").textContent = file.name;
  document.getElementById("selectedFileSize").textContent = formatFileSize(
    file.size,
  );

  // Auto-populate logo name from filename
  autoPopulateLogoName(file.name);

  // Show preview
  const reader = new FileReader();
  reader.onload = function (e) {
    document.getElementById("previewImage").src = e.target.result;
    document.getElementById("uploadPreview").style.display = "block";
  };
  reader.readAsDataURL(file);

  // Enable the upload button since we have a valid file
  enableUploadButton();
}

// Clear selected file
function clearSelectedFile() {
  const fileInput = document.getElementById("logoFile");
  const uploadArea = document.getElementById("fileUploadArea");

  fileInput.value = "";
  fileInput._droppedFile = null; // Clear dropped file reference
  uploadArea.classList.remove("has-file");
  document.getElementById("uploadPreview").style.display = "none";
  document.getElementById("selectedFileName").textContent = "";
  document.getElementById("selectedFileSize").textContent = "";

  // Disable upload button
  disableUploadButton();
}

// Setup drag and drop functionality
function setupDragAndDrop() {
  const uploadArea = document.getElementById("fileUploadArea");
  const fileInput = document.getElementById("logoFile");

  console.log("Setting up drag and drop", uploadArea, fileInput);

  if (!uploadArea || !fileInput) {
    console.error("Upload area or file input not found");
    return;
  }

  // Don't disable pointer events - we need clicks to work
  // fileInput.style.pointerEvents = "none";

  let dragCounter = 0;

  // Only prevent defaults on the upload area, not the entire document
  // This allows normal file input behavior elsewhere

  // Handle drag enter/leave with counter to prevent flickering
  uploadArea.addEventListener(
    "dragenter",
    (e) => {
      // Only prevent default if it's actually a file being dragged
      if (e.dataTransfer && e.dataTransfer.types.includes("Files")) {
        preventDefaults(e);
        dragCounter++;
        highlight();
      }
    },
    false,
  );

  uploadArea.addEventListener(
    "dragleave",
    (e) => {
      if (e.dataTransfer && e.dataTransfer.types.includes("Files")) {
        preventDefaults(e);
        dragCounter--;
        if (dragCounter === 0) {
          unhighlight();
        }
      }
    },
    false,
  );

  uploadArea.addEventListener(
    "dragover",
    (e) => {
      // Only prevent default if it's actually a file being dragged
      if (e.dataTransfer && e.dataTransfer.types.includes("Files")) {
        preventDefaults(e);
        e.dataTransfer.dropEffect = "copy";
      }
    },
    false,
  );

  uploadArea.addEventListener(
    "drop",
    (e) => {
      // Only handle file drops
      if (
        e.dataTransfer &&
        e.dataTransfer.files &&
        e.dataTransfer.files.length > 0
      ) {
        preventDefaults(e);
        dragCounter = 0;
        unhighlight();
        handleDrop(e);
      }
    },
    false,
  );

  console.log("Drag and drop setup complete");
}

function preventDefaults(e) {
  e.preventDefault();
  e.stopPropagation();
}

function highlight() {
  console.log("Drag highlight");
  document.getElementById("fileUploadArea").classList.add("drag-over");
}

function unhighlight() {
  console.log("Drag unhighlight");
  document.getElementById("fileUploadArea").classList.remove("drag-over");
}

function handleDrop(e) {
  console.log("File dropped!", e.dataTransfer.files);
  const files = e.dataTransfer.files;

  if (files && files.length > 0) {
    const file = files[0];
    console.log("Processing dropped file:", file.name, file.type, file.size);

    // Validate file type
    if (!file.type.startsWith("image/")) {
      console.error("Invalid file type:", file.type);
      showError("Please select an image file");
      return;
    }

    // Validate file size (10MB max)
    if (file.size > 10 * 1024 * 1024) {
      console.error("File too large:", file.size);
      showError("File size must be less than 10MB");
      return;
    }

    const fileInput = document.getElementById("logoFile");
    const uploadArea = document.getElementById("fileUploadArea");

    // Update UI to show file is selected
    uploadArea.classList.add("has-file");
    document.getElementById("selectedFileName").textContent = file.name;
    document.getElementById("selectedFileSize").textContent = formatFileSize(
      file.size,
    );

    // Auto-populate logo name from filename
    autoPopulateLogoName(file.name);

    // Show preview
    const reader = new FileReader();
    reader.onload = function (e) {
      document.getElementById("previewImage").src = e.target.result;
      document.getElementById("uploadPreview").style.display = "block";
    };
    reader.readAsDataURL(file);

    // Store the file reference for upload
    fileInput._droppedFile = file;
    console.log("File stored for upload:", file.name);

    // Enable the upload button since we have a valid file
    enableUploadButton();
  } else {
    console.error("No files in drop event");
  }
}

// Perform upload (now functional)
async function performUpload() {
  const fileInput = document.getElementById("logoFile");
  const nameInput = document.getElementById("logoName");
  const descriptionInput = document.getElementById("logoDescription");

  // Check for either regular file input or dropped file
  const file =
    (fileInput.files && fileInput.files[0]) || fileInput._droppedFile;

  if (!file) {
    showError("Please select a file to upload");
    return;
  }

  if (!nameInput.value.trim()) {
    showError("Please enter a logo name");
    return;
  }

  const formData = new FormData();
  formData.append("file", file);
  formData.append("name", nameInput.value.trim());
  if (descriptionInput.value.trim()) {
    formData.append("description", descriptionInput.value.trim());
  }

  try {
    const response = await fetch("/api/v1/logos/upload", {
      method: "POST",
      body: formData,
    });

    if (!response.ok) {
      const errorText = await response.text();
      throw new Error(`Upload failed: ${errorText}`);
    }

    showSuccess("Logo uploaded successfully");
    closeUploadModal();
    loadLogos(currentPage);
    loadStats();
  } catch (error) {
    console.error("Upload error:", error);
    showError(`Failed to upload logo: ${error.message}`);
  }
}

// Edit logo asset
async function editLogoAsset(logoId) {
  try {
    // Find logo in current logos array first
    let logoData = currentLogos.find(
      (logo) => logo.id === logoId || logo.asset.id === logoId,
    );

    if (logoData) {
      // Use data from current logos list
      editingLogo = logoData;
      populateEditForm(logoData);
      SharedUtils.showStandardModal("editModal");
      return;
    }

    // If not found in current list, try to fetch from API
    const response = await fetch(`/api/v1/logos/${logoId}/info`);
    if (!response.ok) throw new Error("Failed to fetch logo details");

    logoData = await response.json();
    editingLogo = logoData;
    populateEditForm(logoData);

    // Show modal using standard utilities
    SharedUtils.showStandardModal("editModal");
  } catch (error) {
    console.error("Error loading logo for editing:", error);
    showError("Failed to load logo details");
  }
}

// Populate edit form
function populateEditForm(logo) {
  // Handle both direct logo objects and LogoAssetWithUrl format
  const logoAsset = logo.asset || logo;
  const logoUrl = logo.url || "";

  document.getElementById("editLogoId").value = logoAsset.id;
  document.getElementById("editLogoName").value = logoAsset.name;
  document.getElementById("editLogoDescription").value =
    logoAsset.description || "";
  document.getElementById("editPreviewImage").src = logoUrl;

  // File info
  const fileInfo = `
        <strong>File:</strong> ${escapeHtml(logoAsset.file_name)}<br>
        <strong>Size:</strong> ${formatFileSize(logoAsset.file_size)}<br>
        <strong>Type:</strong> ${escapeHtml(logoAsset.mime_type)}<br>
        ${logoAsset.width && logoAsset.height ? `<strong>Dimensions:</strong> ${logoAsset.width}×${logoAsset.height}px<br>` : ""}
        <strong>Asset Type:</strong> ${logoAsset.asset_type === "uploaded" ? "Uploaded" : "Cached"}<br>
        <strong>Created:</strong> ${new Date(logoAsset.created_at).toLocaleDateString()}
    `;

  document.getElementById("editFileInfo").innerHTML = fileInfo;
}

// Save logo edit
async function saveLogoEdit() {
  const logoId = document.getElementById("editLogoId").value;
  const name = document.getElementById("editLogoName").value.trim();
  const description = document
    .getElementById("editLogoDescription")
    .value.trim();

  if (!name) {
    showError("Logo name is required");
    return;
  }

  try {
    const response = await fetch(`/api/v1/logos/${logoId}`, {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        name,
        description: description || null,
      }),
    });

    if (!response.ok) throw new Error("Failed to update logo");

    showSuccess("Logo updated successfully");
    closeEditModal();
    loadLogos(currentPage);
  } catch (error) {
    console.error("Error updating logo:", error);
    showError("Failed to update logo");
  }
}

// Close edit modal
function closeEditModal() {
  SharedUtils.hideStandardModal("editModal");
  editingLogo = null;
}

// Delete logo
async function deleteLogo(logoId) {
  if (!logoId) {
    logoId = document.getElementById("editLogoId").value;
  }

  if (
    !confirm(
      "Are you sure you want to delete this logo? This action cannot be undone.",
    )
  ) {
    return;
  }

  try {
    const response = await fetch(`/api/v1/logos/${logoId}`, {
      method: "DELETE",
    });

    if (!response.ok) throw new Error("Failed to delete logo");

    showSuccess("Logo deleted successfully");

    // Close edit modal if open
    const editModal = document.getElementById("editModal");
    if (editModal && editModal.classList.contains("show")) {
      closeEditModal();
    }

    loadLogos(currentPage);
    loadStats(); // Refresh stats
  } catch (error) {
    console.error("Error deleting logo:", error);
    showError("Failed to delete logo");
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
    // Check which modal is open and use appropriate container
    const uploadModal = document.getElementById("uploadModal");
    const editModal = document.getElementById("editModal");

    if (uploadModal && uploadModal.classList.contains("show")) {
      SharedUtils.showError(message, "alertsContainer");
    } else if (editModal && editModal.classList.contains("show")) {
      SharedUtils.showError(message, "alertsContainerEdit");
    } else {
      SharedUtils.showError(message);
    }
  } else {
    console.error(message);
    // Fallback: show a simple alert
    alert("Error: " + message);
  }
}

function showSuccess(message) {
  if (window.SharedUtils) {
    // Check which modal is open and use appropriate container
    const uploadModal = document.getElementById("uploadModal");
    const editModal = document.getElementById("editModal");

    if (uploadModal && uploadModal.classList.contains("show")) {
      SharedUtils.showSuccess(message, "alertsContainer");
    } else if (editModal && editModal.classList.contains("show")) {
      SharedUtils.showSuccess(message, "alertsContainerEdit");
    } else {
      SharedUtils.showSuccess(message);
    }
  } else {
    console.log(message);
    // Fallback: show a simple alert
    alert("Success: " + message);
  }
}

// Additional CSS for badges
// Extract filename without extension for auto-populating logo name
function extractFileNameWithoutExtension(filename) {
  if (!filename) return "";

  // Remove path if present (in case of full path)
  const baseName = filename.split(/[/\\]/).pop();

  // Remove extension
  const lastDotIndex = baseName.lastIndexOf(".");
  if (lastDotIndex === -1) return baseName; // No extension

  return baseName.substring(0, lastDotIndex);
}

// Auto-populate logo name field from filename
function autoPopulateLogoName(filename) {
  const nameField = document.getElementById("logoName");
  if (nameField && filename) {
    const nameFromFile = extractFileNameWithoutExtension(filename);
    // Only set if the field is empty or contains default text
    if (!nameField.value || nameField.value.trim() === "") {
      nameField.value = nameFromFile;
    }
  }
}

// Setup page-level drag and drop
function setupPageLevelDragAndDrop() {
  console.log("Setting up page-level drag and drop");

  let pageDragCounter = 0;

  // Add visual feedback styles to document head if not already present
  if (!document.getElementById("pageDragStyles")) {
    const style = document.createElement("style");
    style.id = "pageDragStyles";
    style.textContent = `
      .page-drag-overlay {
        position: fixed;
        top: 0;
        left: 0;
        width: 100%;
        height: 100%;
        background: rgba(0, 123, 255, 0.08);
        border: 4px dashed #007bff;
        z-index: 9999;
        display: none;
        align-items: center;
        justify-content: center;
        font-size: 28px;
        color: #007bff;
        font-weight: bold;
        pointer-events: none;
        animation: pulse 2s infinite;
      }
      .page-drag-active .page-drag-overlay {
        display: flex;
      }
      .page-drag-overlay-content {
        background: rgba(255, 255, 255, 0.95);
        padding: 2rem 3rem;
        border-radius: 12px;
        box-shadow: 0 8px 32px rgba(0, 0, 0, 0.1);
        text-align: center;
        border: 2px solid #007bff;
      }
      .page-drag-overlay-icon {
        font-size: 48px;
        margin-bottom: 1rem;
        display: block;
      }
      .page-drag-overlay-text {
        font-size: 24px;
        font-weight: 600;
        margin-bottom: 0.5rem;
      }
      .page-drag-overlay-subtext {
        font-size: 16px;
        color: #6c757d;
        font-weight: normal;
      }
      @keyframes pulse {
        0% { border-color: #007bff; }
        50% { border-color: #0056b3; }
        100% { border-color: #007bff; }
      }
    `;
    document.head.appendChild(style);
  }

  // Create overlay element if not present
  if (!document.getElementById("pageDragOverlay")) {
    const overlay = document.createElement("div");
    overlay.id = "pageDragOverlay";
    overlay.className = "page-drag-overlay";
    overlay.innerHTML = `
      <div class="page-drag-overlay-content">
        <span class="page-drag-overlay-icon">📁</span>
        <div class="page-drag-overlay-text">Drop image files here to upload</div>
        <div class="page-drag-overlay-subtext">Supports PNG, JPG, GIF, WebP, SVG (max 10MB)</div>
      </div>
    `;
    document.body.appendChild(overlay);
  }

  // Page-level drag enter
  document.addEventListener("dragenter", (e) => {
    // Only handle file drops, ignore other drag operations
    if (e.dataTransfer && e.dataTransfer.types.includes("Files")) {
      e.preventDefault();
      pageDragCounter++;
      document.body.classList.add("page-drag-active");
    }
  });

  // Page-level drag leave
  document.addEventListener("dragleave", (e) => {
    if (e.dataTransfer && e.dataTransfer.types.includes("Files")) {
      e.preventDefault();
      pageDragCounter--;
      if (pageDragCounter === 0) {
        document.body.classList.remove("page-drag-active");
      }
    }
  });

  // Page-level drag over
  document.addEventListener("dragover", (e) => {
    // Only handle file drops
    if (e.dataTransfer && e.dataTransfer.types.includes("Files")) {
      e.preventDefault();
      e.dataTransfer.dropEffect = "copy";
    }
  });

  // Page-level drop
  document.addEventListener("drop", (e) => {
    // Only handle file drops
    if (
      e.dataTransfer &&
      e.dataTransfer.files &&
      e.dataTransfer.files.length > 0
    ) {
      e.preventDefault();
      pageDragCounter = 0;
      document.body.classList.remove("page-drag-active");

      // Don't handle if the drop is within the upload modal file area
      const uploadArea = document.getElementById("fileUploadArea");
      if (uploadArea && uploadArea.contains(e.target)) {
        return; // Let the existing handler deal with it
      }

      handlePageLevelDrop(e);
    }
  });
}

// Handle page-level file drop
function handlePageLevelDrop(e) {
  console.log("Page-level file dropped!", e.dataTransfer.files);
  const files = e.dataTransfer.files;

  if (files && files.length > 0) {
    const file = files[0];
    console.log(
      "Processing page-level dropped file:",
      file.name,
      file.type,
      file.size,
    );

    // Validate file type
    if (!file.type.startsWith("image/")) {
      console.error("Invalid file type:", file.type);
      showError("Please select an image file");
      return;
    }

    // Validate file size (10MB max)
    if (file.size > 10 * 1024 * 1024) {
      console.error("File too large:", file.size);
      showError("File size must be less than 10MB");
      return;
    }

    // Open upload modal
    uploadLogo();

    // Wait a brief moment for modal to open, then populate it
    setTimeout(() => {
      const fileInput = document.getElementById("logoFile");
      const uploadArea = document.getElementById("fileUploadArea");

      if (fileInput && uploadArea) {
        // Update UI to show file is selected
        uploadArea.classList.add("has-file");
        document.getElementById("selectedFileName").textContent = file.name;
        document.getElementById("selectedFileSize").textContent =
          formatFileSize(file.size);

        // Auto-populate logo name from filename
        autoPopulateLogoName(file.name);

        // Show preview
        const reader = new FileReader();
        reader.onload = function (e) {
          document.getElementById("previewImage").src = e.target.result;
          document.getElementById("uploadPreview").style.display = "block";
        };
        reader.readAsDataURL(file);

        // Store the file reference for upload
        fileInput._droppedFile = file;
        console.log("Page-level file stored for upload:", file.name);

        // Enable the upload button since we have a valid file
        enableUploadButton();
      }
    }, 100);
  }
}

function initializeBadgeStyles() {
  if (!document.getElementById("badge-styles")) {
    const style = document.createElement("style");
    style.id = "badge-styles";
    style.textContent = `
            .badge {
                display: inline-block;
                padding: 0.25em 0.4em;
                font-size: 0.75em;
                font-weight: 700;
                line-height: 1;
                text-align: center;
                white-space: nowrap;
                vertical-align: baseline;
                border-radius: 0.25rem;
            }
            .badge-primary {
                color: #fff;
                background-color: var(--primary-color);
            }
            .badge-secondary {
                color: #fff;
                background-color: var(--secondary-color);
            }
            .pagination {
                display: flex;
                padding-left: 0;
                list-style: none;
                border-radius: var(--border-radius);
                margin: 0;
            }
            .page-item {
                margin: 0 2px;
            }
            .page-link {
                position: relative;
                display: block;
                padding: 0.5rem 0.75rem;
                margin-left: -1px;
                line-height: 1.25;
                color: var(--primary-color);
                text-decoration: none;
                background-color: #fff;
                border: 1px solid var(--border-color);
                border-radius: var(--border-radius);
                cursor: pointer;
            }
            .page-link:hover {
                z-index: 2;
                color: var(--primary-hover);
                text-decoration: none;
                background-color: #e9ecef;
                border-color: var(--border-color);
            }
            .page-item.active .page-link {
                z-index: 1;
                color: #fff;
                background-color: var(--primary-color);
                border-color: var(--primary-color);
            }
            .page-item.disabled .page-link {
                color: var(--text-muted);
                pointer-events: none;
                cursor: auto;
                background-color: #fff;
                border-color: var(--border-color);
            }
            .justify-content-center {
                justify-content: center !important;
            }
        `;
    document.head.appendChild(style);
  }
}

// Initialize badge styles
initializeBadgeStyles();
