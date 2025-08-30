"use client"

import { useState, useEffect } from "react"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Badge } from "@/components/ui/badge"
import { Alert, AlertDescription } from "@/components/ui/alert"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { 
  RefreshCw, 
  Save, 
  Settings as SettingsIcon,
  CheckCircle,
  AlertCircle,
  XCircle
} from "lucide-react"
import { RuntimeSettings, UpdateSettingsRequest, SettingsResponse } from "@/types/api"
import { apiClient } from "@/lib/api-client"
import { FeatureFlagsEditor } from "@/components/feature-flags-editor"
import { useFeatureFlags, invalidateFeatureFlagsCache } from "@/hooks/useFeatureFlags"

// Feature flag interface (should match the one in FeatureFlagsEditor)
interface FeatureFlag {
  key: string
  enabled: boolean
  config: Record<string, any>
}

// Standard Rust tracing log levels
const LOG_LEVELS = [
  { value: 'TRACE', label: 'TRACE', description: 'Most verbose, includes all details' },
  { value: 'DEBUG', label: 'DEBUG', description: 'Debugging information' },
  { value: 'INFO', label: 'INFO', description: 'General information (default)' },
  { value: 'WARN', label: 'WARN', description: 'Warning messages' },
  { value: 'ERROR', label: 'ERROR', description: 'Error messages only' },
] as const

function getStatusIcon(success: boolean) {
  return success ? (
    <CheckCircle className="h-4 w-4 text-green-500" />
  ) : (
    <XCircle className="h-4 w-4 text-destructive" />
  )
}

export function Settings() {
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [saveSuccess, setSaveSuccess] = useState<string | null>(null)
  const [settings, setSettings] = useState<RuntimeSettings | null>(null)
  const [editedSettings, setEditedSettings] = useState<Partial<RuntimeSettings>>({})
  
  // Feature flags state
  const [flags, setFlags] = useState<FeatureFlag[]>([])
  const [flagsLoaded, setFlagsLoaded] = useState(false)
  const { refetch } = useFeatureFlags()

  const fetchSettings = async () => {
    setLoading(true)
    setError(null)
    setSaveSuccess(null)
    
    try {
      const response: SettingsResponse = await apiClient.getSettings()
      if (response.success && response.settings) {
        setSettings(response.settings)
        setEditedSettings({})
      } else {
        setError('No settings data received')
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to fetch settings')
    } finally {
      setLoading(false)
    }
  }

  const fetchFeatureFlags = async () => {
    try {
      const response = await apiClient.getFeatures()
      const featureFlags: FeatureFlag[] = []
      
      // Convert flags and config into unified structure
      Object.entries(response.flags || {}).forEach(([key, enabled]) => {
        featureFlags.push({
          key,
          enabled: Boolean(enabled),
          config: response.config?.[key] || {}
        })
      })
      
      // Add any config-only features (features with config but no flag)
      Object.keys(response.config || {}).forEach(key => {
        if (!featureFlags.find(f => f.key === key)) {
          featureFlags.push({
            key,
            enabled: false,
            config: response.config[key] || {}
          })
        }
      })
      
      setFlags(featureFlags.sort((a, b) => a.key.localeCompare(b.key)))
      setFlagsLoaded(true)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to fetch feature flags')
    }
  }

  const fetchAll = async () => {
    setLoading(true)
    setError(null)
    setSaveSuccess(null)
    
    try {
      await Promise.all([fetchSettings(), fetchFeatureFlags()])
    } catch (err) {
      // Error handling is done in individual functions
    } finally {
      setLoading(false)
    }
  }

  const saveSettings = async (): Promise<string> => {
    if (!settings || Object.keys(editedSettings).length === 0) {
      return 'No settings changes to save'
    }

    try {
      const updateRequest: UpdateSettingsRequest = editedSettings
      const response: SettingsResponse = await apiClient.updateSettings(updateRequest)
      
      if (response.success) {
        setSettings(response.settings)
        setEditedSettings({})
        return response.message + (response.applied_changes.length > 0 ? 
          ` Applied changes: ${response.applied_changes.join(', ')}` : '')
      } else {
        throw new Error('Failed to save settings')
      }
    } catch (err) {
      throw new Error(err instanceof Error ? err.message : 'Failed to save settings')
    }
  }

  const saveFeatureFlags = async (): Promise<string> => {
    try {
      const flagsData = flags.reduce((acc, flag) => {
        acc[flag.key] = flag.enabled
        return acc
      }, {} as Record<string, boolean>)
      
      const configData = flags.reduce((acc, flag) => {
        if (Object.keys(flag.config).length > 0) {
          acc[flag.key] = flag.config
        }
        return acc
      }, {} as Record<string, Record<string, any>>)
      
      await apiClient.updateFeatures({
        flags: flagsData,
        config: configData
      })
      
      // Invalidate cache and refresh the feature flags context
      invalidateFeatureFlagsCache()
      await refetch()
      
      return 'Feature flags updated successfully'
    } catch (err) {
      throw new Error(err instanceof Error ? err.message : 'Failed to save feature flags')
    }
  }

  const saveAll = async () => {
    setSaving(true)
    setError(null)
    setSaveSuccess(null)
    
    const settingsHasChanges = settings && Object.keys(editedSettings).length > 0
    const featureFlagsChanged = flagsLoaded // Assume flags might have changed if loaded
    
    if (!settingsHasChanges && !featureFlagsChanged) {
      return
    }
    
    try {
      const results: string[] = []
      
      if (settingsHasChanges) {
        const settingsResult = await saveSettings()
        results.push(settingsResult)
      }
      
      if (featureFlagsChanged) {
        const flagsResult = await saveFeatureFlags()
        results.push(flagsResult)
      }
      
      setSaveSuccess(results.join('. '))
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save changes')
    } finally {
      setSaving(false)
    }
  }

  const handleInputChange = (key: keyof RuntimeSettings, value: any) => {
    if (settings && value === settings[key]) {
      // Value is back to original, remove from edited settings
      setEditedSettings(prev => {
        const newSettings = { ...prev }
        delete newSettings[key]
        return newSettings
      })
    } else {
      // Value is different from original, add to edited settings
      setEditedSettings(prev => ({
        ...prev,
        [key]: value
      }))
    }
  }

  const getCurrentValue = (key: keyof RuntimeSettings) => {
    return editedSettings.hasOwnProperty(key) ? editedSettings[key] : settings?.[key]
  }

  const isModified = (key: keyof RuntimeSettings) => {
    return editedSettings.hasOwnProperty(key) && settings && editedSettings[key] !== settings[key]
  }

  const hasSettingsChanges = Object.keys(editedSettings).length > 0
  const hasAnyChanges = hasSettingsChanges || flagsLoaded // Simplified - assume flags might have changes if loaded

  useEffect(() => {
    fetchAll()
  }, [])

  return (
    <div className="space-y-6">
      {/* Header with controls */}
      <div className="flex justify-between items-center">
        <div>
          <p className="text-muted-foreground">
            Runtime application settings that can be changed without restart
          </p>
        </div>
        <div className="flex gap-2">
          <Button onClick={fetchAll} disabled={loading} size="sm" variant="outline">
            <RefreshCw className={`h-4 w-4 mr-2 ${loading ? 'animate-spin' : ''}`} />
            Refresh All
          </Button>
          <Button 
            onClick={saveAll} 
            disabled={saving || !hasAnyChanges} 
            size="sm"
          >
            <Save className={`h-4 w-4 mr-2 ${saving ? 'animate-spin' : ''}`} />
            Save All Changes
          </Button>
        </div>
      </div>

      {/* Status Messages */}
      {error && (
        <Card className="border-destructive">
          <CardContent className="pt-6">
            <div className="flex items-center gap-2 text-destructive">
              <XCircle className="h-4 w-4" />
              <span className="font-medium">Error:</span>
              <span>{error}</span>
            </div>
          </CardContent>
        </Card>
      )}

      {saveSuccess && (
        <Alert variant="success">
          <CheckCircle className="h-4 w-4" />
          <AlertDescription>
            <span className="font-medium">Success:</span> {saveSuccess}
          </AlertDescription>
        </Alert>
      )}

      {/* Feature Flags Management */}
      <FeatureFlagsEditor 
        flags={flags}
        setFlags={setFlags}
        loading={loading}
        error={error}
        setError={setError}
        onRefresh={fetchFeatureFlags}
      />

      {/* Settings Table */}
      {settings && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <SettingsIcon className="h-5 w-5" />
              Runtime Settings
            </CardTitle>
            <CardDescription>
              Modify application settings that take effect immediately
            </CardDescription>
          </CardHeader>
          <CardContent>
            <div className="space-y-6">
              {/* Log Level */}
              <div className="flex items-center justify-between py-4 border-b">
                <div className="space-y-1">
                  <div className="font-medium">Log Level</div>
                  <div className="text-sm text-muted-foreground">
                    Minimum log level to output. Lower levels include all higher levels.
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  {isModified('log_level') && (
                    <Badge variant="secondary">Modified</Badge>
                  )}
                  <Select
                    value={String(getCurrentValue('log_level') || 'INFO')}
                    onValueChange={(value) => handleInputChange('log_level', value)}
                  >
                    <SelectTrigger className="w-[240px] justify-between">
                      <SelectValue placeholder="Select level" className="text-left" />
                    </SelectTrigger>
                    <SelectContent className="w-[240px]">
                      {LOG_LEVELS.map((level) => (
                        <SelectItem key={level.value} value={level.value} className="cursor-pointer">
                          <div className="flex flex-col py-1 w-full text-left">
                            <span className="font-medium text-sm">{level.label}</span>
                            <span className="text-xs text-muted-foreground whitespace-nowrap">{level.description}</span>
                          </div>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              </div>

              {/* Request Logging */}
              <div className="flex items-center justify-between py-4 border-b">
                <div className="space-y-1">
                  <div className="font-medium">Request Logging</div>
                  <div className="text-sm text-muted-foreground">
                    Enable detailed logging of HTTP requests
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  {isModified('enable_request_logging') && (
                    <Badge variant="secondary">Modified</Badge>
                  )}
                  <input
                    id="enable_request_logging"
                    type="checkbox"
                    checked={Boolean(getCurrentValue('enable_request_logging'))}
                    onChange={(e) => handleInputChange('enable_request_logging', e.target.checked)}
                    className="rounded border-gray-300"
                  />
                </div>
              </div>

              {/* Metrics Collection */}
              <div className="flex items-center justify-between py-4 border-b">
                <div className="space-y-1">
                  <div className="font-medium">Metrics Collection</div>
                  <div className="text-sm text-muted-foreground">
                    Enable collection of application metrics
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  {isModified('enable_metrics') && (
                    <Badge variant="secondary">Modified</Badge>
                  )}
                  <input
                    id="enable_metrics"
                    type="checkbox"
                    checked={Boolean(getCurrentValue('enable_metrics')) || false}
                    onChange={(e) => handleInputChange('enable_metrics', e.target.checked)}
                    className="rounded border-gray-300"
                  />
                </div>
              </div>

              {/* Max Connections */}
              <div className="flex items-center justify-between py-4 border-b">
                <div className="space-y-1">
                  <div className="font-medium">Max Connections</div>
                  <div className="text-sm text-muted-foreground">
                    Maximum number of concurrent connections (optional)
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  {isModified('max_connections') && (
                    <Badge variant="secondary">Modified</Badge>
                  )}
                  <Input
                    type="number"
                    value={String(getCurrentValue('max_connections') || '')}
                    onChange={(e) => handleInputChange('max_connections', e.target.value ? parseInt(e.target.value) : undefined)}
                    className="w-32"
                    placeholder="1000"
                    min="0"
                  />
                </div>
              </div>

              {/* Request Timeout */}
              <div className="flex items-center justify-between py-4">
                <div className="space-y-1">
                  <div className="font-medium">Request Timeout</div>
                  <div className="text-sm text-muted-foreground">
                    Request timeout in seconds (optional)
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  {isModified('request_timeout_seconds') && (
                    <Badge variant="secondary">Modified</Badge>
                  )}
                  <Input
                    type="number"
                    value={String(getCurrentValue('request_timeout_seconds') || '')}
                    onChange={(e) => handleInputChange('request_timeout_seconds', e.target.value ? parseInt(e.target.value) : undefined)}
                    className="w-32"
                    placeholder="30"
                    min="0"
                  />
                </div>
              </div>
            </div>
          </CardContent>
        </Card>
      )}

      {/* Raw Settings Data (for debugging) */}
      {settings && (
        <Card>
          <CardHeader>
            <CardTitle>Raw Settings Data</CardTitle>
            <CardDescription>
              Current settings as returned by the API
            </CardDescription>
          </CardHeader>
          <CardContent>
            <pre className="bg-muted p-3 rounded text-xs overflow-auto">
              {JSON.stringify(settings, null, 2)}
            </pre>
          </CardContent>
        </Card>
      )}
    </div>
  )
}