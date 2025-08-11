import { API_CONFIG } from './config'

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

export class SSEClient {
  private eventSource: EventSource | null = null
  private handlers: Map<string, ProgressEventHandler[]> = new Map()
  private reconnectAttempts = 0
  private maxReconnectAttempts = 5
  private reconnectDelay = 1000

  connect(operationType?: string) {
    if (this.eventSource) {
      console.log('[SSE] Disconnecting existing connection before reconnecting')
      this.disconnect()
    }

    try {
      const url = new URL(`${API_CONFIG.baseUrl}/api/v1/progress/events`)
      if (operationType) {
        url.searchParams.set('operation_type', operationType)
      }

      console.log(`[SSE] Connecting to: ${url.toString()}`)
      this.eventSource = new EventSource(url.toString())

      this.eventSource.onopen = () => {
        console.log('[SSE] Connection opened successfully')
        this.reconnectAttempts = 0
      }

      this.eventSource.onmessage = (event) => {
        console.log('[SSE] Raw message received:', event.data)
        try {
          const progressEvent: ProgressEvent = JSON.parse(event.data)
          console.log('[SSE] Parsed progress event:', progressEvent)
          this.handleEvent(progressEvent)
        } catch (error) {
          console.error('[SSE] Failed to parse event:', error, 'Raw data:', event.data)
        }
      }

      this.eventSource.onerror = (error) => {
        console.error('[SSE] Connection error:', error, 'ReadyState:', this.eventSource?.readyState)
        this.handleReconnect()
      }

    } catch (error) {
      console.error('[SSE] Failed to create SSE connection:', error)
    }
  }

  private handleEvent(event: ProgressEvent) {
    console.log(`[SSE] Handling event for operation_type: ${event.operation_type}`)
    
    // Call handlers for specific operation types
    const typeHandlers = this.handlers.get(event.operation_type) || []
    console.log(`[SSE] Found ${typeHandlers.length} handlers for ${event.operation_type}`)
    typeHandlers.forEach(handler => handler(event))

    // Call global handlers
    const globalHandlers = this.handlers.get('*') || []
    console.log(`[SSE] Found ${globalHandlers.length} global handlers`)
    globalHandlers.forEach(handler => handler(event))
  }

  private handleReconnect() {
    if (this.reconnectAttempts < this.maxReconnectAttempts) {
      this.reconnectAttempts++
      console.log(`Attempting to reconnect SSE (attempt ${this.reconnectAttempts})`)
      
      setTimeout(() => {
        if (this.eventSource?.readyState === EventSource.CLOSED) {
          this.connect()
        }
      }, this.reconnectDelay * this.reconnectAttempts)
    } else {
      console.error('Max reconnect attempts reached, giving up')
    }
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
      }
    }
  }

  disconnect() {
    if (this.eventSource) {
      this.eventSource.close()
      this.eventSource = null
    }
    this.handlers.clear()
    this.reconnectAttempts = 0
  }

  isConnected(): boolean {
    return this.eventSource?.readyState === EventSource.OPEN
  }
}

// Export singleton instance
export const sseClient = new SSEClient()