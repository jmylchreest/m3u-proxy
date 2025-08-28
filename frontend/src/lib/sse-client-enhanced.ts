import { API_CONFIG } from './config'
import { Debug } from '@/utils/debug'

export interface ProgressEvent {
  id: string
  operation_type: string
  operation_name: string
  state: 'idle' | 'connecting' | 'downloading' | 'processing' | 'completed' | 'failed'
  current_step: string
  progress: {
    percentage: number | null
    items?: {
      processed: number
      total: number
    } | null
    bytes?: number | null
  }
  timing: {
    started_at: string
    updated_at: string
    completed_at: string | null
    duration_ms: number
  }
  metadata: Record<string, any>
  error: string | null
}

export type ProgressEventHandler = (event: ProgressEvent) => void

export class EnhancedSSEClient {
  private eventSource: EventSource | null = null
  private handlers: Map<string, ProgressEventHandler[]> = new Map()
  private reconnectAttempts = 0
  private maxReconnectAttempts = 3 // Reduced from 5
  private reconnectDelay = 1000
  private debug = Debug.createLogger('EnhancedSSEClient')
  private abortController: AbortController | null = null
  private connectionTimeout: NodeJS.Timeout | null = null

  connect(operationType?: string, includeCompleted: boolean = false) {
    // Immediately disconnect any existing connection
    this.disconnect()

    // Create new abort controller for this connection
    this.abortController = new AbortController()

    try {
      const url = new URL(`${API_CONFIG.baseUrl}/api/v1/progress/events`)
      url.searchParams.set('include_completed', includeCompleted.toString())
      
      if (operationType) {
        url.searchParams.set('operation_type', operationType)
      }

      this.debug.log(`Connecting to: ${url.toString()}`)
      
      // Create connection with timeout
      this.connectionTimeout = setTimeout(() => {
        this.debug.error('Connection timeout after 10 seconds')
        this.handleConnectionError('timeout')
      }, 10000) // 10 second timeout

      this.eventSource = new EventSource(url.toString())

      this.eventSource.onopen = () => {
        if (this.abortController?.signal.aborted) return
        
        this.debug.log('Connection opened successfully')
        this.reconnectAttempts = 0
        
        if (this.connectionTimeout) {
          clearTimeout(this.connectionTimeout)
          this.connectionTimeout = null
        }
      }

      this.eventSource.addEventListener('heartbeat', (event) => {
        if (this.abortController?.signal.aborted) return
        this.debug.log('Heartbeat received')
      })

      this.eventSource.addEventListener('progress', (event) => {
        if (this.abortController?.signal.aborted) return
        
        try {
          const progressEvent: ProgressEvent = JSON.parse(event.data)
          this.handleEvent(progressEvent)
        } catch (error) {
          this.debug.error('Failed to parse progress event:', error)
        }
      })

      this.eventSource.onmessage = (event) => {
        if (this.abortController?.signal.aborted) return
        
        try {
          const progressEvent: ProgressEvent = JSON.parse(event.data)
          this.handleEvent(progressEvent)
        } catch (error) {
          this.debug.error('Failed to parse event:', error)
        }
      }

      this.eventSource.onerror = (error) => {
        if (this.abortController?.signal.aborted) return
        
        this.debug.error('Connection error:', error, 'ReadyState:', this.eventSource?.readyState)
        this.handleConnectionError('error')
      }

    } catch (error) {
      this.debug.error('Failed to create SSE connection:', error)
      this.handleConnectionError('creation_failed')
    }
  }

  private handleConnectionError(reason: string) {
    if (this.connectionTimeout) {
      clearTimeout(this.connectionTimeout)
      this.connectionTimeout = null
    }

    // Don't reconnect if connection was aborted (navigation happening)
    if (this.abortController?.signal.aborted) {
      this.debug.log('Connection aborted, not reconnecting')
      return
    }

    if (this.reconnectAttempts < this.maxReconnectAttempts) {
      this.reconnectAttempts++
      const delay = this.reconnectDelay * Math.pow(2, this.reconnectAttempts - 1) // Exponential backoff
      
      this.debug.log(`Attempting to reconnect in ${delay}ms (attempt ${this.reconnectAttempts}/${this.maxReconnectAttempts})`)
      
      setTimeout(() => {
        if (!this.abortController?.signal.aborted && this.eventSource?.readyState === EventSource.CLOSED) {
          this.connect()
        }
      }, delay)
    } else {
      this.debug.error(`Max reconnect attempts reached after ${reason}, giving up`)
    }
  }

  private handleEvent(event: ProgressEvent) {
    this.debug.log(`Handling event for operation_type: ${event.operation_type}`)
    
    // Call handlers for specific operation types
    const typeHandlers = this.handlers.get(event.operation_type) || []
    typeHandlers.forEach(handler => {
      try {
        handler(event)
      } catch (error) {
        this.debug.error('Error in type handler:', error)
      }
    })

    // Call global handlers
    const globalHandlers = this.handlers.get('*') || []
    globalHandlers.forEach(handler => {
      try {
        handler(event)
      } catch (error) {
        this.debug.error('Error in global handler:', error)
      }
    })
  }

  subscribe(operationType: string, handler: ProgressEventHandler) {
    if (!this.handlers.has(operationType)) {
      this.handlers.set(operationType, [])
    }
    this.handlers.get(operationType)!.push(handler)
  }

  unsubscribe(operationType: string, handler: ProgressEventHandler) {
    const handlers = this.handlers.get(operationType)
    if (handlers) {
      const index = handlers.indexOf(handler)
      if (index > -1) {
        handlers.splice(index, 1)
        
        // Clean up empty handler arrays
        if (handlers.length === 0) {
          this.handlers.delete(operationType)
        }
      }
    }
  }

  disconnect() {
    this.debug.log('Disconnecting SSE client')
    
    // Clear connection timeout
    if (this.connectionTimeout) {
      clearTimeout(this.connectionTimeout)
      this.connectionTimeout = null
    }

    // Abort any pending operations
    if (this.abortController) {
      this.abortController.abort()
      this.abortController = null
    }

    // Close event source
    if (this.eventSource) {
      this.eventSource.close()
      this.eventSource = null
    }

    // Reset state
    this.reconnectAttempts = 0
  }

  isConnected(): boolean {
    return this.eventSource?.readyState === EventSource.OPEN
  }
}

// Export singleton instance
export const enhancedSSEClient = new EnhancedSSEClient()