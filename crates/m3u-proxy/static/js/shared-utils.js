// Shared Utilities for M3U Proxy
class SharedUtils {
  // DOM Utilities
  static escapeHtml(text) {
    if (!text) return "";
    const div = document.createElement("div");
    div.textContent = text;
    return div.innerHTML;
  }

  // File System Utilities
  static formatFileSize(bytes) {
    if (bytes === 0) return "0 Bytes";
    if (!bytes) return "";
    const k = 1024;
    const sizes = ["Bytes", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
  }

  // Date/Time Utilities
  static formatTimeCompact(date) {
    if (!date || isNaN(date.getTime())) {
      return "Invalid date";
    }

    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffDays = Math.floor(Math.abs(diffMs) / (1000 * 60 * 60 * 24));
    const diffHours = Math.floor(Math.abs(diffMs) / (1000 * 60 * 60));
    const diffMins = Math.floor(Math.abs(diffMs) / (1000 * 60));

    // For past dates
    if (diffMs > 0) {
      if (diffDays > 0) return `${diffDays}d ago`;
      if (diffHours > 0) return `${diffHours}h ago`;
      if (diffMins > 5) return `${diffMins}m ago`;
      return "Just now";
    }

    // For future dates
    if (diffDays > 0) return `in ${diffDays}d`;
    if (diffHours > 0) return `in ${diffHours}h`;
    if (diffMins > 5) return `in ${diffMins}m`;
    return "Soon";
  }

  // Modal Utilities
  static showStandardModal(modalId) {
    const modal = document.getElementById(modalId);
    if (modal) {
      modal.style.display = "flex";
      modal.classList.add("show");
      document.body.style.overflow = "hidden"; // Prevent background scrolling
    }
  }

  static hideStandardModal(modalId) {
    const modal = document.getElementById(modalId);
    if (modal) {
      modal.classList.remove("show");
      // Use timeout to allow fade animation
      setTimeout(() => {
        modal.style.display = "none";
        document.body.style.overflow = ""; // Restore background scrolling
      }, 300);
    }
  }

  static setupStandardModalCloseHandlers(modalId) {
    const modal = document.getElementById(modalId);
    if (!modal) return;

    // Close on backdrop click
    modal.addEventListener("click", (e) => {
      if (e.target === modal) {
        SharedUtils.hideStandardModal(modalId);
      }
    });

    // Close on Escape key
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && modal.classList.contains("show")) {
        SharedUtils.hideStandardModal(modalId);
      }
    });

    // Setup close button handlers
    const closeButtons = modal.querySelectorAll(".modal-close");
    closeButtons.forEach((button) => {
      button.addEventListener("click", () => {
        SharedUtils.hideStandardModal(modalId);
      });
    });
  }

  static parseDateTime(dateStr) {
    if (dateStr) {
      const normalizedDateStr = dateStr.replace(/\+00:00$/, "Z");
      const date = new Date(normalizedDateStr);

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

  // Alert/Notification System
  static showAlert(message, type = "info", containerId = "alertsContainer") {
    const alertsContainer = document.getElementById(containerId);
    if (!alertsContainer) {
      console.warn(`Alert container '${containerId}' not found`);
      return;
    }

    const alert = document.createElement("div");
    alert.className = `alert alert-${type}`;
    alert.textContent = message;

    alertsContainer.appendChild(alert);

    setTimeout(() => {
      if (alert.parentNode) {
        alert.remove();
      }
    }, 5000);
  }

  static showError(message, containerId = "alertsContainer") {
    this.showAlert(message, "danger", containerId);
  }

  static showSuccess(message, containerId = "alertsContainer") {
    this.showAlert(message, "success", containerId);
  }

  static showWarning(message, containerId = "alertsContainer") {
    this.showAlert(message, "warning", containerId);
  }

  static showInfo(message, containerId = "alertsContainer") {
    this.showAlert(message, "info", containerId);
  }

  // Loading State Management
  static showLoading(elementId = "loadingIndicator", tableId = null) {
    const loadingElement = document.getElementById(elementId);
    if (loadingElement) {
      loadingElement.style.display = "block";
    }

    if (tableId) {
      const tableElement = document.getElementById(tableId);
      if (tableElement) {
        tableElement.style.opacity = "0.5";
      }
    }
  }

  static hideLoading(elementId = "loadingIndicator", tableId = null) {
    const loadingElement = document.getElementById(elementId);
    if (loadingElement) {
      loadingElement.style.display = "none";
    }

    if (tableId) {
      const tableElement = document.getElementById(tableId);
      if (tableElement) {
        tableElement.style.opacity = "1";
      }
    }
  }

  // Modal Management
  static showModal(modalId) {
    const modal = document.getElementById(modalId);
    if (modal) {
      modal.classList.add("show");
    }
  }

  static hideModal(modalId) {
    const modal = document.getElementById(modalId);
    if (modal) {
      modal.classList.remove("show");
    }
  }

  // API Utilities
  static async handleApiCall(apiCall, errorMessage = "Operation failed") {
    try {
      const result = await apiCall();
      return { success: true, data: result };
    } catch (error) {
      console.error(`${errorMessage}:`, error);
      this.showError(`${errorMessage}: ${error.message}`);
      return { success: false, error };
    }
  }

  static createCacheBustingUrl(url) {
    const separator = url.includes("?") ? "&" : "?";
    return `${url}${separator}_t=${new Date().getTime()}`;
  }

  // Form Utilities
  static collectFormData(formElement) {
    const formData = new FormData(formElement);
    const data = {};
    for (let [key, value] of formData.entries()) {
      data[key] = value;
    }
    return data;
  }

  static resetForm(formElement) {
    if (formElement) {
      formElement.reset();
    }
  }

  // DOM Initialization Helper
  static onDomReady(callback) {
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", callback);
    } else {
      callback();
    }
  }

  // Confirmation Dialog (only for destructive actions)
  static confirm(message) {
    return window.confirm(message);
  }

  // Array Safety
  static ensureArray(data) {
    return Array.isArray(data) ? data : [];
  }

  // Debounce utility for search/input handlers
  static debounce(func, wait) {
    let timeout;
    return function executedFunction(...args) {
      const later = () => {
        clearTimeout(timeout);
        func(...args);
      };
      clearTimeout(timeout);
      timeout = setTimeout(later, wait);
    };
  }
}

// Make globally available
window.SharedUtils = SharedUtils;
