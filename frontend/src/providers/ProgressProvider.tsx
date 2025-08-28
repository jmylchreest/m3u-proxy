"use client"

import React, { createContext, useContext, useRef, useEffect, useCallback, useState, useMemo, ReactNode } from 'react'
import { usePathname } from 'next/navigation'
import { sseManager, ProgressEvent as SSEProgressEvent } from '@/lib/sse-singleton'
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
  const debug = Debug.createLogger('ProgressProvider')
  const notificationSubscribersRef = useRef<Set<{ current: (event: NotificationEvent) => void }>>(new Set())

  // Local filtering logic based on current page
  const getOperationTypeFilter = useCallback((path: string): string | null => {
    // Normalize path to handle both with and without trailing slashes
    const normalizedPath = path.endsWith('/') ? path : path + '/'
    
    switch (normalizedPath) {
      case '/sources/stream/':
        return 'stream_ingestion'
      case '/sources/epg/':
        return 'epg_ingestion'
      case '/proxies/':
        return 'proxy_regeneration'
      default:
        return null // No filter for events page and other pages
    }
  }, [])

  const shouldIncludeCompleted = useCallback((path: string): boolean => {
    return path === '/events' || path === '/events/'
  }, [])

  // Local event filtering based on current page context
  const filterEventForCurrentPage = useCallback((event: ProgressEvent, currentPath: string): boolean => {
    const operationTypeFilter = getOperationTypeFilter(currentPath)
    const includeCompleted = shouldIncludeCompleted(currentPath)

    // Operation type filtering
    if (operationTypeFilter && event.operation_type !== operationTypeFilter) {
      return false
    }

    // Completion filtering - on most pages we don't want to show completed events cluttering the UI
    if (!includeCompleted && (event.state === 'completed' || event.state === 'error')) {
      return false
    }

    return true
  }, [getOperationTypeFilter, shouldIncludeCompleted])

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

      // Notify all notification subscribers
      for (const subscriberRef of notificationSubscribersRef.current) {
        subscriberRef.current(notificationEvent)
      }

      debug.log('Event processed, stored in local events map, and sent to notification subscribers')
  }, []) // NO DEPENDENCIES - stable callback to prevent SSE reconnections

  // Use global SSE singleton - no more per-component connections
  useEffect(() => {
    debug.log('ProgressProvider: Setting up connection to global SSE singleton')
    
    // Subscribe to all events from the singleton
    const unsubscribeFromAll = sseManager.subscribeToAll((event) => {
      handleProgressEvent(event)
    })
    
    // Monitor connection status
    const checkConnectionStatus = () => {
      setConnected(sseManager.isConnected())
    }
    
    // Initial connection status check
    checkConnectionStatus()
    
    // Poll connection status periodically
    const statusInterval = setInterval(checkConnectionStatus, 1000)

    // Cleanup on unmount
    return () => {
      debug.log('ProgressProvider: Cleaning up SSE singleton subscriptions')
      unsubscribeFromAll()
      clearInterval(statusInterval)
    }
  }, [handleProgressEvent])

  // Subscribe to events for specific resource ID using global singleton
  const subscribe = useCallback((resourceId: string, callback: (event: ProgressEvent) => void) => {
    debug.log(`Subscribing to resource via singleton: ${resourceId}`)
    
    // Use the global singleton manager for subscriptions
    return sseManager.subscribe(resourceId, callback)
  }, [])

  // Subscribe to events by operation type using global singleton
  const subscribeToType = useCallback((operationType: string, callback: (event: ProgressEvent) => void) => {
    debug.log(`Subscribing to operation type via singleton: ${operationType}`)
    
    // Use the global singleton manager for subscriptions
    return sseManager.subscribe(operationType, callback)
  }, [])

  // Subscribe to all events (for notifications)
  const subscribeToAll = useCallback((callback: (event: NotificationEvent) => void) => {
    debug.log('Subscribing to all events (for notifications)')
    
    // Store the callback in a ref so we can call it from handleProgressEvent
    const callbackRef = { current: callback }
    
    // Add callback to the subscribers set
    notificationSubscribersRef.current.add(callbackRef)
    
    // Send existing events to new subscriber
    for (const event of events.values()) {
      callback(event)
    }
    
    return () => {
      debug.log('Unsubscribing from all events (notifications)')
      notificationSubscribersRef.current.delete(callbackRef)
    }
  }, [events])

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

  // Get all events with visibility tracking (optionally filtered by current page)
  const getAllEvents = useCallback((filterByCurrentPage: boolean = false) => {
    const allEvents = Array.from(events.values())
    
    const filteredEvents = filterByCurrentPage 
      ? allEvents.filter(event => filterEventForCurrentPage(event, pathname))
      : allEvents
      
    return filteredEvents.sort((a, b) => 
      new Date(b.last_update).getTime() - new Date(a.last_update).getTime()
    )
  }, [events, pathname, filterEventForCurrentPage])

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