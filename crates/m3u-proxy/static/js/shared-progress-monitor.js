/**
 * Shared Progress Monitor utility for real-time progress updates
 * Supports both Server-Sent Events (SSE) and polling fallback
 */
console.log('Loading shared-progress-monitor.js');
class ProgressMonitor {
  constructor(operationType, onProgressUpdate, onError = null) {
    this.operationType = operationType;
    this.onProgressUpdate = onProgressUpdate;
    this.onError = onError;
    
    // Connection management
    this.eventSource = null;
    this.pollingInterval = null;
    this.useSSE = true; // Try SSE first, fallback to polling if needed
    this.isRunning = false;
    
    // Configuration
    this.SSE_RETRY_DELAY = 5000; // 5 seconds before retrying SSE
    this.POLLING_INTERVAL = 2000; // 2 seconds for polling fallback
    
    // Debug logging
    this.debug = false;
  }
  
  /**
   * Start monitoring progress updates
   */
  start() {
    if (this.isRunning) {
      this.stop();
    }
    
    this.isRunning = true;
    
    if (this.useSSE) {
      this._startSSE();
    } else {
      this._startPolling();
    }
  }
  
  /**
   * Stop monitoring progress updates
   */
  stop() {
    this.isRunning = false;
    
    // Stop SSE connection if active
    if (this.eventSource) {
      this.eventSource.close();
      this.eventSource = null;
    }
    
    // Stop polling if active
    if (this.pollingInterval) {
      clearInterval(this.pollingInterval);
      this.pollingInterval = null;
    }
  }
  
  /**
   * Enable debug logging
   */
  enableDebug() {
    this.debug = true;
  }
  
  /**
   * Force fallback to polling (useful for testing)
   */
  forcePolling() {
    this.useSSE = false;
    if (this.isRunning) {
      this.start(); // Restart with polling
    }
  }
  
  /**
   * Start Server-Sent Events monitoring
   */
  _startSSE() {
    try {
      // Build URL with operation type filter
      const url = new URL('/api/v1/progress/events', window.location.origin);
      if (this.operationType) {
        url.searchParams.set('operation_type', this.operationType);
      }
      
      console.log(`Attempting to connect to SSE endpoint: ${url.toString()}`);
      this.eventSource = new EventSource(url.toString());
      
      this.eventSource.onopen = (event) => {
        if (this.debug) {
          console.log(`SSE connection opened for ${this.operationType} progress updates`);
        }
      };
      
      this.eventSource.addEventListener('heartbeat', (event) => {
        if (this.debug) {
          console.debug('SSE heartbeat:', event.data);
        }
      });
      
      this.eventSource.addEventListener('progress', (event) => {
        try {
          const operation = JSON.parse(event.data);
          this.onProgressUpdate(operation);
        } catch (error) {
          console.error('Error parsing SSE progress data:', error);
          if (this.onError) {
            this.onError(error);
          }
        }
      });
      
      this.eventSource.addEventListener('disconnect', (event) => {
        if (this.debug) {
          console.log('SSE disconnected:', event.data);
        }
        this.eventSource.close();
        this.eventSource = null;
        
        // Retry SSE connection after delay if still running
        if (this.isRunning && this.useSSE) {
          setTimeout(() => {
            if (this.isRunning && this.useSSE) {
              if (this.debug) {
                console.log('Retrying SSE connection...');
              }
              this._startSSE();
            }
          }, this.SSE_RETRY_DELAY);
        }
      });
      
      this.eventSource.onerror = (event) => {
        console.warn(`SSE connection error for ${this.operationType}:`, event);
        console.warn('EventSource readyState:', this.eventSource.readyState);
        console.warn('Falling back to polling');
        this.eventSource.close();
        this.eventSource = null;
        this.useSSE = false; // Disable SSE for this session
        
        if (this.isRunning) {
          this._startPolling();
        }
      };
      
    } catch (error) {
      console.error(`Failed to initialize SSE for ${this.operationType}, falling back to polling:`, error);
      this.useSSE = false;
      if (this.isRunning) {
        this._startPolling();
      }
    }
  }
  
  /**
   * Start traditional polling monitoring
   */
  _startPolling() {
    if (this.pollingInterval) {
      clearInterval(this.pollingInterval);
    }
    
    this.pollingInterval = setInterval(async () => {
      if (!this.isRunning) return;
      
      try {
        // Build API URL with operation type filter
        const url = new URL('/api/v1/progress', window.location.origin);
        if (this.operationType) {
          url.searchParams.set('operation_type', this.operationType);
        }
        
        const response = await fetch(url.toString());
        if (!response.ok) return;
        
        const data = await response.json();
        const operations = data.operations || [];
        
        // Send each operation as individual progress update
        operations.forEach(operation => {
          this.onProgressUpdate(operation);
        });
        
      } catch (error) {
        if (this.debug) {
          console.debug(`Polling progress error for ${this.operationType}:`, error);
        }
        if (this.onError) {
          this.onError(error);
        }
      }
    }, this.POLLING_INTERVAL);
  }
}

// Export for use in other scripts
window.ProgressMonitor = ProgressMonitor;
console.log('ProgressMonitor class registered on window object');