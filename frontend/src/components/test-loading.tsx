"use client"

import { useState, useEffect } from 'react'
import { useManualLoading } from '@/hooks/usePageLoading'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'

export function TestLoading() {
  const { startLoading, stopLoading } = useManualLoading()
  const [data, setData] = useState<string | null>(null)

  const simulateDataFetch = async () => {
    startLoading()
    setData(null)
    
    try {
      // Simulate API call
      await new Promise(resolve => setTimeout(resolve, 2000))
      setData("Data loaded successfully!")
    } catch (error) {
      setData("Error loading data")
    } finally {
      stopLoading()
    }
  }

  const simulateFastLoad = async () => {
    startLoading()
    
    try {
      // Simulate fast API call
      await new Promise(resolve => setTimeout(resolve, 500))
      setData("Fast data loaded!")
    } finally {
      stopLoading()
    }
  }

  useEffect(() => {
    // Initial data load with loading spinner
    simulateDataFetch()
  }, [])

  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <CardTitle>Loading Spinner Test</CardTitle>
          <CardDescription>
            Test the global page loading spinner functionality
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div>
            <p className="text-sm text-muted-foreground mb-2">
              Data Status: {data || "Loading..."}
            </p>
          </div>
          
          <div className="flex gap-2">
            <Button onClick={simulateDataFetch}>
              Simulate Slow Load (2s)
            </Button>
            <Button variant="outline" onClick={simulateFastLoad}>
              Simulate Fast Load (0.5s)
            </Button>
          </div>

          <div className="text-xs text-muted-foreground">
            <p>• Click buttons to test manual loading control</p>
            <p>• Navigate between pages to see automatic route loading</p>
            <p>• Loading spinner appears globally over the entire page</p>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}