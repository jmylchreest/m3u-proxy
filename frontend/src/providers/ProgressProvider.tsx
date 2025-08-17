"use client"

import React, { createContext, useContext, useRef, useEffect, useCallback, useState, useMemo, ReactNode } from 'react'
import { usePathname } from 'next/navigation'
import { SSEClient } from '@/lib/sse-client'
import { getBackendUrl } from '@/lib/config'
import { ProgressEvent as APIProgressEvent, ProgressStage } from '@/types/api'
import { Debug } from '@/utils/debug'

// Extend the API type for UI-specific functionality
export interface ProgressEvent extends APIProgressEvent {
  hasBeenVisible?: boolean
}

export interface NotificationEvent extends ProgressEvent {
  hasBeenVisible: boolean
  // Composite key for grouping: owner_id + operation_type  
  groupKey?: string
}

interface ProgressEventContext {
  // Subscribe to events for specific resource IDs
  subscribe: (resourceId: string, callback: (event: ProgressEvent) => void) => () => void
  // Subscribe to all events of a specific operation type
  subscribeToType: (operationType: string, callback: (event: ProgressEvent) => void) => () => void
  // Subscribe to all events (for notifications)
  subscribeToAll: (callback: (event: NotificationEvent) => void) => () => void
  // Get current state for a resource
  getResourceState: (resourceId: string) => ProgressEvent | null
  // Get all events with visibility tracking
  getAllEvents: () => NotificationEvent[]
  // Mark events as visible
  markAsVisible: (eventIds: string[]) => void
  // Get unread count for operation type
  getUnreadCount: (operationType?: string) => number
  // Connection status
  isConnected: boolean
}

const ProgressContext = createContext<ProgressEventContext | null>(null)

export function ProgressProvider({ children }: { children: ReactNode }) {
  const pathname = usePathname()
  const [events, setEvents] = useState<Map<string, NotificationEvent>>(new Map())
  const [connected, setConnected] = useState(false)
  const subscribersRef = useRef<Map<string, Set<(event: ProgressEvent) => void>>>(new Map())
  const allSubscribersRef = useRef<Set<(event: NotificationEvent) => void>>(new Set())
  const sseClientRef = useRef<SSEClient | null>(null)
  const debug = Debug.createLogger('ProgressProvider')

  // Determine if we should include completed events based on current page
  const includeCompleted = pathname === '/events/'
  
  // Determine operation type filter based on current page
  const getOperationTypeForPath = (path: string): string | null => {
    switch (path) {
      case '/sources/stream/':
        return 'stream_ingestion'
      case '/sources/epg/':
        return 'epg_ingestion'
      case '/proxies/':
        return 'proxy_regeneration'
      default:
        return null // No filter for events page and other pages
    }
  }
  
  const operationType = getOperationTypeForPath(pathname)

  // Handle progress events (now with single operation ID per process)
  const handleProgressEvent = useCallback((event: ProgressEvent) => {
      debug.log('Received event:', {
        id: event.id,
        owner_id: event.owner_id,
        owner_type: event.owner_type,
        operation_type: event.operation_type,
        operation_name: event.operation_name,
        state: event.state,
        current_stage: event.current_stage,
        overall_percentage: event.overall_percentage,
        stages: event.stages
      })
      
      // Create notification event with visibility tracking
      const notificationEvent: NotificationEvent = {
        ...event,
        hasBeenVisible: false
      }

      // Update events map using event.id
      setEvents(prev => {
        const newEvents = new Map(prev)
        const existingEvent = newEvents.get(event.id)
        
        // Preserve hasBeenVisible flag if event already exists
        if (existingEvent) {
          notificationEvent.hasBeenVisible = existingEvent.hasBeenVisible
        }
        
        newEvents.set(event.id, notificationEvent)
        return newEvents
      })

      // Route to resource-specific subscribers using owner_id
      if (event.owner_id) {
        const subscribers = subscribersRef.current.get(event.owner_id)
        if (subscribers && subscribers.size > 0) {
          debug.log(`Found ${subscribers.size} subscribers for owner: ${event.owner_id}`)
          subscribers.forEach(callback => {
            try {
              callback(event)
            } catch (error) {
              debug.error('Error in resource subscriber:', error)
            }
          })
        } else {
          debug.log(`No subscribers found for owner: ${event.owner_id}`)
        }
      }

      // Route to operation-type subscribers
      const typeSubscribers = subscribersRef.current.get(event.operation_type)
      typeSubscribers?.forEach(callback => {
        try {
          callback(event)
        } catch (error) {
          debug.error('Error in type subscriber:', error)
        }
      })

      // Route to all subscribers (for notifications)
      allSubscribersRef.current.forEach(callback => {
        try {
          callback(notificationEvent)
        } catch (error) {
          debug.error('Error in all subscriber:', error)
        }
      })
  }, [])

  // Initialize SSE connection
  useEffect(() => {
    debug.log('Initializing SSE connection')
    
    // Create a simple EventSource directly since we need to control connection status
    const backendUrl = getBackendUrl()
    
    // Build query parameters
    const params = new URLSearchParams({
      include_completed: includeCompleted.toString()
    })
    
    // Add operation_type filter if specified (not for events page)
    if (operationType) {
      params.append('operation_type', operationType)
    }
    
    const sseUrl = `${backendUrl}/api/v1/progress/events?${params}`
    debug.log('SSE URL with filters:', sseUrl)
    const eventSource = new EventSource(sseUrl)
    
    eventSource.onopen = () => {
      debug.log('SSE connection opened')
      setConnected(true)
    }
    
    // Add listener for all event types (including heartbeat)
    eventSource.addEventListener('heartbeat', (event) => {
      debug.log('Heartbeat received:', event.data)
    })
    
    eventSource.addEventListener('progress', (event) => {
      debug.log('Progress event received:', event.data)
      try {
        const progressEvent: ProgressEvent = JSON.parse(event.data)
        debug.log('Parsed progress event:', progressEvent)
        handleProgressEvent(progressEvent)
      } catch (error) {
        debug.error('Failed to parse progress event:', error, 'Raw data:', event.data)
      }
    })
    
    eventSource.onerror = (error) => {
      debug.log('SSE connection error:', error)
      setConnected(false)
    }
    
    eventSource.onmessage = (event) => {
      debug.log('Raw SSE message received:', event)
      debug.log('Event data:', event.data)
      try {
        const progressEvent: ProgressEvent = JSON.parse(event.data)
        debug.log('Parsed progress event:', progressEvent)
        handleProgressEvent(progressEvent)
      } catch (error) {
        debug.error('Failed to parse event:', error, 'Raw data:', event.data)
      }
    }

    return () => {
      debug.log('Cleaning up SSE connection')
      eventSource.close()
      setConnected(false)
    }
  }, [includeCompleted, operationType])

  // Subscribe to events for specific resource ID
  const subscribe = useCallback((resourceId: string, callback: (event: ProgressEvent) => void) => {
    debug.log(`Subscribing to resource: ${resourceId}`)
    
    if (!subscribersRef.current.has(resourceId)) {
      subscribersRef.current.set(resourceId, new Set())
    }
    subscribersRef.current.get(resourceId)!.add(callback)

    // Return unsubscribe function
    return () => {
      debug.log(`Unsubscribing from resource: ${resourceId}`)
      const subscribers = subscribersRef.current.get(resourceId)
      if (subscribers) {
        subscribers.delete(callback)
        if (subscribers.size === 0) {
          subscribersRef.current.delete(resourceId)
        }
      }
    }
  }, [])

  // Subscribe to events by operation type
  const subscribeToType = useCallback((operationType: string, callback: (event: ProgressEvent) => void) => {
    debug.log(`Subscribing to operation type: ${operationType}`)
    
    if (!subscribersRef.current.has(operationType)) {
      subscribersRef.current.set(operationType, new Set())
    }
    subscribersRef.current.get(operationType)!.add(callback)

    return () => {
      debug.log(`Unsubscribing from operation type: ${operationType}`)
      const subscribers = subscribersRef.current.get(operationType)
      if (subscribers) {
        subscribers.delete(callback)
        if (subscribers.size === 0) {
          subscribersRef.current.delete(operationType)
        }
      }
    }
  }, [])

  // Subscribe to all events
  const subscribeToAll = useCallback((callback: (event: NotificationEvent) => void) => {
    debug.log('Subscribing to all events')
    allSubscribersRef.current.add(callback)

    return () => {
      debug.log('Unsubscribing from all events')
      allSubscribersRef.current.delete(callback)
    }
  }, [])

  // Get current state for a resource
  const getResourceState = useCallback((resourceId: string): ProgressEvent | null => {
    // Check if any event is for this resource ID using owner_id
    for (const [eventId, event] of events) {
      if (event.owner_id === resourceId) {
        return event
      }
    }
    return null
  }, [events])

  // Get all events with visibility tracking
  const getAllEvents = useCallback(() => {
    return Array.from(events.values()).sort((a, b) => 
      new Date(b.last_update).getTime() - new Date(a.last_update).getTime()
    )
  }, [events])

  // Mark events as visible
  const markAsVisible = useCallback((eventIds: string[]) => {
    setEvents(prev => {
      const newEvents = new Map(prev)
      let hasChanges = false
      
      eventIds.forEach(eventId => {
        const event = newEvents.get(eventId)
        if (event && !event.hasBeenVisible) {
          newEvents.set(eventId, { ...event, hasBeenVisible: true })
          hasChanges = true
        }
      })
      
      return hasChanges ? newEvents : prev
    })
  }, [])

  // Get unread count
  const getUnreadCount = useCallback((operationType?: string) => {
    let count = 0
    for (const event of events.values()) {
      if (!event.hasBeenVisible) {
        if (!operationType || event.operation_type === operationType) {
          count++
        }
      }
    }
    return count
  }, [events])

  const contextValue: ProgressEventContext = useMemo(() => ({
    subscribe,
    subscribeToType,
    subscribeToAll,
    getResourceState,
    getAllEvents,
    markAsVisible,
    getUnreadCount,
    isConnected: connected
  }), [subscribe, subscribeToType, subscribeToAll, getResourceState, markAsVisible, connected])

  return (
    <ProgressContext.Provider value={contextValue}>
      {children}
    </ProgressContext.Provider>
  )
}

export function useProgressContext() {
  const context = useContext(ProgressContext)
  if (!context) {
    throw new Error('useProgressContext must be used within a ProgressProvider')
  }
  return context
}