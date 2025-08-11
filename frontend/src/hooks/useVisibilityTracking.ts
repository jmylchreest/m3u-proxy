"use client"

import { useEffect, useRef } from 'react'
import { useProgressContext } from '@/providers/ProgressProvider'

/**
 * Hook to automatically mark events as visible when they appear in the UI
 * @param eventIds - Array of event IDs that are currently visible
 * @param enabled - Whether to track visibility (default: true)
 */
export function useVisibilityTracking(eventIds: string[], enabled: boolean = true) {
  const context = useProgressContext()
  const previousEventIds = useRef<Set<string>>(new Set())

  useEffect(() => {
    if (!enabled || eventIds.length === 0) return

    const currentEventIds = new Set(eventIds)
    const newEventIds: string[] = []

    // Find events that are newly visible
    for (const eventId of currentEventIds) {
      if (!previousEventIds.current.has(eventId)) {
        newEventIds.push(eventId)
      }
    }

    // Mark new events as visible
    if (newEventIds.length > 0) {
      console.log('[useVisibilityTracking] Marking events as visible:', newEventIds)
      context.markAsVisible(newEventIds)
    }

    // Update the previous set
    previousEventIds.current = currentEventIds
  }, [eventIds, enabled, context])
}

/**
 * Hook to mark events as visible when a component mounts/renders
 * Useful for one-time marking when events are displayed
 * @param eventIds - Array of event IDs to mark as visible
 */
export function useMarkAsVisible(eventIds: string[]) {
  const context = useProgressContext()
  
  useEffect(() => {
    if (eventIds.length > 0) {
      console.log('[useMarkAsVisible] Marking events as visible:', eventIds)
      context.markAsVisible(eventIds)
    }
  }, [eventIds, context])
}