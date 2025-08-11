"use client"

import React, { createContext, useContext, useEffect, useState, useCallback } from 'react'
import { getBackendUrl } from '@/lib/config'

export interface BackendConnectivityState {
  isConnected: boolean
  isChecking: boolean
  lastChecked: Date | null
  error: string | null
  backendUrl: string
  checkConnection: () => Promise<void>
}

const BackendConnectivityContext = createContext<BackendConnectivityState | undefined>(undefined)

export function useBackendConnectivity() {
  const context = useContext(BackendConnectivityContext)
  if (context === undefined) {
    throw new Error('useBackendConnectivity must be used within a BackendConnectivityProvider')
  }
  return context
}

interface BackendConnectivityProviderProps {
  children: React.ReactNode
}

export function BackendConnectivityProvider({
  children,
}: BackendConnectivityProviderProps) {
  const [isConnected, setIsConnected] = useState(false)
  const [isChecking, setIsChecking] = useState(true)
  const [lastChecked, setLastChecked] = useState<Date | null>(null)
  const [error, setError] = useState<string | null>(null)
  
  const backendUrl = getBackendUrl()

  const checkConnection = useCallback(async () => {
    setIsChecking(true)
    setError(null)
    
    try {
      console.log('[Backend] Checking connectivity to:', backendUrl)
      
      // Use the /live endpoint as it's a simple health check
      const controller = new AbortController()
      const timeoutId = setTimeout(() => controller.abort(), 10000) // 10 second timeout
      
      const response = await fetch(`${backendUrl}/live`, {
        method: 'GET',
        headers: {
          'Content-Type': 'application/json',
        },
        signal: controller.signal,
      })
      
      clearTimeout(timeoutId)
      
      if (response.ok) {
        console.log('[Backend] Connection successful')
        setIsConnected(true)
        setError(null)
      } else {
        throw new Error(`Backend returned ${response.status}: ${response.statusText}`)
      }
    } catch (err) {
      console.error('[Backend] Connection failed:', err)
      setIsConnected(false)
      
      if (err instanceof Error) {
        if (err.name === 'AbortError') {
          setError('Connection timeout - backend did not respond within 10 seconds')
        } else if (err.message.includes('fetch')) {
          setError('Network error - unable to reach backend service')
        } else {
          setError(err.message)
        }
      } else {
        setError('Unknown connection error')
      }
    } finally {
      setIsChecking(false)
      setLastChecked(new Date())
    }
  }, [backendUrl])

  // Initial connection check
  useEffect(() => {
    checkConnection()
  }, [checkConnection])

  // Periodic health checks every 60 seconds when connected
  useEffect(() => {
    if (!isConnected) return

    const interval = setInterval(() => {
      console.log('[Backend] Performing periodic health check')
      checkConnection()
    }, 60000) // 60 seconds

    return () => clearInterval(interval)
  }, [isConnected, checkConnection])

  // Retry logic when disconnected - check every 30 seconds
  useEffect(() => {
    if (isConnected || isChecking) return

    const retryInterval = setInterval(() => {
      console.log('[Backend] Retrying connection...')
      checkConnection()
    }, 30000) // 30 seconds

    return () => clearInterval(retryInterval)
  }, [isConnected, isChecking, checkConnection])

  const contextValue: BackendConnectivityState = {
    isConnected,
    isChecking,
    lastChecked,
    error,
    backendUrl,
    checkConnection,
  }

  return (
    <BackendConnectivityContext.Provider value={contextValue}>
      {children}
    </BackendConnectivityContext.Provider>
  )
}