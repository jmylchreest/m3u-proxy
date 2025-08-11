import { ServiceEvent, ProgressEvent, EventHandler } from '@/types/api'
import { getBackendUrl } from '@/lib/config'

export class EventsClient {
  private eventSource: EventSource | null = null
  private handlers: EventHandler[] = []
  private reconnectAttempts = 0
  private maxReconnectAttempts = 5
  private reconnectDelay = 1000

  connect() {
    if (this.eventSource) {
      console.log('[Events] Disconnecting existing connection before reconnecting')
      this.disconnect()
    }

    try {
      console.log('[Events] Connecting to events stream')
      const backendUrl = getBackendUrl()
      this.eventSource = new EventSource(`${backendUrl}/api/v1/progress/events`)

      this.eventSource.onopen = () => {
        console.log('[Events] Connection opened successfully')
        this.reconnectAttempts = 0
      }

      // Handle ALL message events generically
      this.eventSource.onmessage = (event) => {
        console.log('[Events] Default message received:', event.data)
        this.parseAndHandleEvent(event.data)
      }

      // Specifically listen for 'progress' events
      this.eventSource.addEventListener('progress', (event: MessageEvent) => {
        console.log('[Events] Progress event received:', event.data)
        this.parseAndHandleEvent(event.data)
      })

      // Listen for other common SSE event types
      const otherEventTypes = ['message', 'data', 'update', 'log', 'status', 'event']
      otherEventTypes.forEach(eventType => {
        this.eventSource!.addEventListener(eventType, (event: MessageEvent) => {
          console.log(`[Events] ${eventType} event received:`, event.data)
          this.parseAndHandleEvent(event.data)
        })
      })

      this.eventSource.onerror = (error) => {
        console.error('[Events] Connection error:', error, 'ReadyState:', this.eventSource?.readyState)
        this.handleReconnect()
      }

    } catch (error) {
      console.error('[Events] Failed to create SSE connection:', error)
    }
  }

  private parseAndHandleEvent(data: string) {
    try {
      const eventData = JSON.parse(data)
      console.log('[Events] Parsed event data:', eventData)
      
      // Create a generic ServiceEvent from any JSON structure
      let serviceEvent: ServiceEvent
      
      if ('operation_type' in eventData) {
        // Handle progress events
        const progressEvent = eventData as ProgressEvent
        serviceEvent = {
          id: progressEvent.id || `event-${Date.now()}`,
          timestamp: progressEvent.last_update || new Date().toISOString(),
          level: progressEvent.state === 'error' ? 'error' : 
                 progressEvent.state === 'completed' ? 'info' : 'debug',
          message: progressEvent.operation_name ? 
                   `${progressEvent.operation_name}: ${progressEvent.current_stage}` :
                   JSON.stringify(eventData),
          source: progressEvent.operation_type || 'unknown',
          context: eventData
        }
      } else if ('level' in eventData && 'message' in eventData) {
        // Handle standard service events
        serviceEvent = eventData as ServiceEvent
      } else {
        // Handle completely generic JSON as an event
        serviceEvent = {
          id: eventData.id || `event-${Date.now()}`,
          timestamp: eventData.timestamp || eventData.created_at || eventData.updated_at || new Date().toISOString(),
          level: 'info',
          message: eventData.message || eventData.description || eventData.name || JSON.stringify(eventData).substring(0, 100),
          source: eventData.source || eventData.type || eventData.event_type || 'generic',
          context: eventData
        }
      }
      
      this.handleEvent(serviceEvent)
    } catch (error) {
      console.error('[Events] Failed to parse event:', error, 'Raw data:', data)
      // Create a fallback event for unparseable data
      const fallbackEvent: ServiceEvent = {
        id: `parse-error-${Date.now()}`,
        timestamp: new Date().toISOString(),
        level: 'error',
        message: `Failed to parse SSE data: ${data.substring(0, 100)}...`,
        source: 'sse-parser',
        context: { raw_data: data, error: error instanceof Error ? error.message : 'Unknown error' }
      }
      this.handleEvent(fallbackEvent)
    }
  }

  private handleEvent(event: ServiceEvent) {
    console.log(`[Events] Handling event: ${event.level} - ${event.message}`)
    this.handlers.forEach(handler => handler(event))
  }

  private handleReconnect() {
    if (this.reconnectAttempts < this.maxReconnectAttempts) {
      this.reconnectAttempts++
      console.log(`[Events] Attempting to reconnect (attempt ${this.reconnectAttempts})`)
      
      setTimeout(() => {
        if (this.eventSource?.readyState === EventSource.CLOSED) {
          this.connect()
        }
      }, this.reconnectDelay * this.reconnectAttempts)
    } else {
      console.error('[Events] Max reconnect attempts reached, giving up')
    }
  }

  subscribe(handler: EventHandler) {
    this.handlers.push(handler)
  }

  unsubscribe(handler: EventHandler) {
    const index = this.handlers.indexOf(handler)
    if (index > -1) {
      this.handlers.splice(index, 1)
    }
  }

  disconnect() {
    if (this.eventSource) {
      this.eventSource.close()
      this.eventSource = null
    }
    this.handlers = []
    this.reconnectAttempts = 0
  }

  isConnected(): boolean {
    return this.eventSource?.readyState === EventSource.OPEN
  }
}

// Export singleton instance
export const eventsClient = new EventsClient()