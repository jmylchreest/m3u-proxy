"use client"

import { useState, useEffect, useMemo, useRef, useCallback } from 'react'
import { Bell, X, GripHorizontal } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Progress } from '@/components/ui/progress'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { useProgressContext, NotificationEvent } from '@/providers/ProgressProvider'
import { formatProgress } from '@/hooks/useProgressState'
import { cn } from '@/lib/utils'

interface NotificationBellProps {
  operationType?: string // Filter by operation type (undefined = all)
  showPopup?: boolean // Whether to show popup on click (false = just mark as read)
  className?: string
}

export function NotificationBell({ 
  operationType, 
  showPopup = true,
  className 
}: NotificationBellProps) {
  const context = useProgressContext()
  const [events, setEvents] = useState<NotificationEvent[]>([])
  const [isOpen, setIsOpen] = useState(false)
  const [height, setHeight] = useState(384) // Default h-96 = 384px
  const [isResizing, setIsResizing] = useState(false)
  const resizeRef = useRef<HTMLDivElement>(null)
  const startYRef = useRef(0)
  const startHeightRef = useRef(0)

  // Subscribe to all events
  useEffect(() => {
    const unsubscribe = context.subscribeToAll((event) => {
      setEvents(prev => {
        const newEvents = new Map<string, NotificationEvent>()
        
        // Add existing events
        prev.forEach(e => newEvents.set(e.id, e))
        
        // Add/update new event
        newEvents.set(event.id, event)
        
        // Convert back to array and sort by update time
        return Array.from(newEvents.values()).sort((a, b) => 
          new Date(b.last_update).getTime() - new Date(a.last_update).getTime()
        )
      })
    })

    // Load initial events
    setEvents(context.getAllEvents())

    return unsubscribe
  }, [])

  // Filter events by operation type and get unread count
  const { filteredEvents, unreadCount } = useMemo(() => {
    const filtered = operationType 
      ? events.filter(e => e.operation_type === operationType)
      : events

    const unread = filtered.filter(e => !e.hasBeenVisible).length

    return {
      filteredEvents: filtered.slice(0, 20), // Show last 20 events
      unreadCount: unread
    }
  }, [events, operationType])

  // Handle bell click
  const handleBellClick = () => {
    if (showPopup) {
      setIsOpen(!isOpen)
      
      // Mark visible events as read when popup opens
      if (!isOpen && filteredEvents.length > 0) {
        const visibleEventIds = filteredEvents.map(e => e.id)
        context.markAsVisible(visibleEventIds)
      }
    } else {
      // Just mark all unread events as read
      const unreadEventIds = filteredEvents
        .filter(e => !e.hasBeenVisible)
        .map(e => e.id)
      
      if (unreadEventIds.length > 0) {
        context.markAsVisible(unreadEventIds)
      }
    }
  }

  const formatEventTime = (event: NotificationEvent) => {
    const date = new Date(event.last_update)
    const now = new Date()
    const diffMs = now.getTime() - date.getTime()
    const diffMinutes = Math.floor(diffMs / 60000)
    
    if (diffMinutes < 1) return 'Just now'
    if (diffMinutes < 60) return `${diffMinutes}m ago`
    
    const diffHours = Math.floor(diffMinutes / 60)
    if (diffHours < 24) return `${diffHours}h ago`
    
    const diffDays = Math.floor(diffHours / 24)
    return `${diffDays}d ago`
  }

  const getEventStatusColor = (state: string) => {
    switch (state) {
      case 'completed': return 'text-green-600 dark:text-green-400'
      case 'error': return 'text-destructive'
      case 'processing': return 'text-blue-600 dark:text-blue-400'
      case 'idle': return 'text-amber-600 dark:text-amber-400'
      default: return 'text-muted-foreground'
    }
  }

  // Resize handlers
  const handleMouseMove = useCallback((e: MouseEvent) => {
    const deltaY = e.clientY - startYRef.current
    const newHeight = Math.max(200, Math.min(600, startHeightRef.current + deltaY))
    setHeight(newHeight)
  }, [])

  const handleMouseUp = useCallback(() => {
    setIsResizing(false)
    document.removeEventListener('mousemove', handleMouseMove)
    document.removeEventListener('mouseup', handleMouseUp)
  }, [handleMouseMove])

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setIsResizing(true)
    startYRef.current = e.clientY
    startHeightRef.current = height
    
    document.addEventListener('mousemove', handleMouseMove)
    document.addEventListener('mouseup', handleMouseUp)
  }, [height, handleMouseMove, handleMouseUp])

  // Cleanup listeners on unmount
  useEffect(() => {
    return () => {
      document.removeEventListener('mousemove', handleMouseMove)
      document.removeEventListener('mouseup', handleMouseUp)
    }
  }, [handleMouseMove, handleMouseUp])

  return (
    <Popover open={isOpen} onOpenChange={setIsOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          className={cn("relative", className)}
          onClick={handleBellClick}
        >
          <Bell className="h-4 w-4" />
          {unreadCount > 0 && (
            <Badge 
              variant="destructive" 
              className="absolute -top-1 -right-1 h-5 w-5 rounded-full p-0 text-xs flex items-center justify-center"
            >
              {unreadCount > 99 ? '99+' : unreadCount}
            </Badge>
          )}
        </Button>
      </PopoverTrigger>
      
      {showPopup && (
        <PopoverContent className="w-96 p-0" align="end">
          <div className="flex items-center justify-between p-4 border-b">
            <h3 className="font-semibold">
              {operationType ? `${operationType.replace('_', ' ')} Events` : 'Recent Activity'}
            </h3>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setIsOpen(false)}
              className="h-6 w-6 p-0"
            >
              <X className="h-4 w-4" />
            </Button>
          </div>
          
          <div 
            className="overflow-y-auto notification-scrollbar"
            style={{ height: `${height}px` }}
          >
            {filteredEvents.length === 0 ? (
              <div className="p-8 text-center text-muted-foreground">
                <Bell className="h-8 w-8 mx-auto mb-2" />
                <p>No recent events</p>
              </div>
            ) : (
              <div className="p-2">
                {filteredEvents.map((event) => (
                  <div
                    key={event.id}
                    className={cn(
                      "p-3 rounded-lg border mb-2 transition-colors",
                      !event.hasBeenVisible && "bg-accent/50 border-accent"
                    )}
                  >
                    <div className="flex items-start justify-between mb-2">
                      <div className="font-medium text-sm">
                        {event.operation_name}
                        {event.stages && event.stages.length > 1 && (() => {
                          const currentStageIndex = event.stages.findIndex(s => s.id === event.current_stage)
                          const stageNumber = currentStageIndex >= 0 ? currentStageIndex + 1 : 1
                          return (
                            <Badge variant="outline" className="ml-2 text-xs">
                              {stageNumber}/{event.stages.length} stages
                            </Badge>
                          )
                        })()}
                      </div>
                      <div className="text-xs text-muted-foreground">
                        {formatEventTime(event)}
                      </div>
                    </div>
                    
                    <div className="flex items-center justify-between text-sm">
                      <span className={cn("font-medium", getEventStatusColor(event.state))}>
                        {event.state.charAt(0).toUpperCase() + event.state.slice(1)}
                      </span>
                      <span className="text-muted-foreground">
                        {(() => {
                          const startTime = new Date(event.started_at).getTime()
                          const updateTime = new Date(event.last_update).getTime()
                          const durationMs = updateTime - startTime
                          return durationMs > 0 ? `${Math.floor(durationMs / 1000)}s` : ''
                        })()
                        }
                      </span>
                    </div>
                    
                    {/* Stage information */}
                    {(() => {
                      const currentStage = event.stages?.find(s => s.id === event.current_stage)
                      return currentStage && currentStage.stage_step && (
                        <div className="text-xs text-muted-foreground mt-1">
                          <span className="font-medium">{currentStage.name}:</span> {currentStage.stage_step}
                        </div>
                      )
                    })()}
                    
                    {/* Enhanced progress display with stage information */}
                    {event.overall_percentage !== undefined && (
                      <div className="space-y-2 mt-2">
                        {/* Overall progress */}
                        <div className="space-y-1">
                          <div className="flex items-center justify-between text-xs">
                            <span className="text-muted-foreground">
                              {event.stages && event.stages.length > 1 ? 'Overall Progress' : 'Progress'}
                            </span>
                            <span className="text-muted-foreground font-medium">
                              {event.overall_percentage.toFixed(1)}%
                            </span>
                          </div>
                          <Progress value={event.overall_percentage} className="h-2" />
                        </div>
                        
                        {/* Current stage progress - only show if we have multiple stages */}
                        {(() => {
                          const currentStage = event.stages?.find(s => s.id === event.current_stage)
                          if (!currentStage || !event.stages || event.stages.length <= 1) return null
                          
                          const currentStageIndex = event.stages.findIndex(s => s.id === event.current_stage)
                          const stageNumber = currentStageIndex >= 0 ? currentStageIndex + 1 : 1
                          
                          return (
                            <div className="space-y-1">
                              <div className="flex items-center justify-between text-xs">
                                <span className="text-muted-foreground">
                                  Stage: {currentStage.name}
                                  <span className="ml-1">
                                    ({stageNumber}/{event.stages.length})
                                  </span>
                                </span>
                              <span className="text-muted-foreground font-medium">
                                {currentStage.percentage.toFixed(1)}%
                              </span>
                            </div>
                              <Progress value={currentStage.percentage} className="h-2" />
                            </div>
                          )
                        })()}
                      </div>
                    )}
                    
                    {event.error && (
                      <div className="text-xs text-destructive mt-2 p-2 bg-red-50 rounded">
                        {event.error}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
          
          {/* Resize handle */}
          <div 
            ref={resizeRef}
            className="flex items-center justify-center h-4 bg-muted/50 hover:bg-muted border-t cursor-ns-resize group"
            onMouseDown={handleMouseDown}
          >
            <GripHorizontal className="h-3 w-3 text-muted-foreground group-hover:text-foreground transition-colors" />
          </div>
        </PopoverContent>
      )}
    </Popover>
  )
}