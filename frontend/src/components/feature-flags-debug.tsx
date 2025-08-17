"use client";

import React from 'react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { useFeatureFlags } from '@/hooks/useFeatureFlags';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { RefreshCw, Flag, Check, X } from 'lucide-react';

/**
 * Feature flags debug component that shows all current feature flags and their values
 */
export function FeatureFlagsDebug() {
  const { featureFlags, featureConfigs, isLoading, error } = useFeatureFlags();

  if (isLoading) {
    return (
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Flag className="w-5 h-5" />
            Feature Flags
          </CardTitle>
          <CardDescription>
            Current feature flag configuration
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex items-center justify-center py-8">
            <RefreshCw className="w-6 h-6 animate-spin mr-2" />
            <span>Loading feature flags...</span>
          </div>
        </CardContent>
      </Card>
    );
  }

  if (error) {
    return (
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Flag className="w-5 h-5" />
            Feature Flags
          </CardTitle>
          <CardDescription>
            Current feature flag configuration
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Alert variant="destructive">
            <AlertDescription>
              Failed to load feature flags: {error}
            </AlertDescription>
          </Alert>
        </CardContent>
      </Card>
    );
  }

  const flagEntries = Object.entries(featureFlags);
  const enabledFlags = flagEntries.filter(([_, enabled]) => enabled);
  const disabledFlags = flagEntries.filter(([_, enabled]) => !enabled);
  const configEntries = Object.entries(featureConfigs);

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Flag className="w-5 h-5" />
          Feature Flags & Configuration
        </CardTitle>
        <CardDescription>
          Current feature flags and configuration from backend
        </CardDescription>
      </CardHeader>
      <CardContent>
        {flagEntries.length === 0 && configEntries.length === 0 ? (
          <div className="text-center py-6 text-muted-foreground">
            <Flag className="w-12 h-12 mx-auto mb-3 opacity-50" />
            <p>No feature flags or configuration found</p>
          </div>
        ) : (
          <div className="space-y-6">
            {/* Feature Flags Section */}
            {flagEntries.length > 0 ? (
              <div>
                {/* Summary header */}
                <div className="flex items-center justify-between py-4 border-b">
                  <div className="space-y-1">
                    <div className="font-medium">Feature Flags</div>
                    <div className="text-sm text-muted-foreground">
                      Runtime feature toggles and their current status
                    </div>
                  </div>
                  <div className="flex items-center gap-4 text-sm text-muted-foreground">
                    <span>{enabledFlags.length} enabled</span>
                    <span>{disabledFlags.length} disabled</span>
                  </div>
                </div>

                {/* Feature flag rows */}
                {flagEntries.map(([key, enabled], index) => {
                  const hasConfig = configEntries.find(([configKey]) => configKey === key);
                  const isLast = index === flagEntries.length - 1;
                  return (
                    <div
                      key={key}
                      className={`flex items-center justify-between py-4 ${!isLast ? 'border-b' : ''}`}
                    >
                      <div className="space-y-1">
                        <div className="font-medium font-mono text-sm">{key}</div>
                        <div className="text-sm text-muted-foreground">
                          {hasConfig ? 'Feature flag with configuration options' : 'Simple feature toggle'}
                        </div>
                      </div>
                      <div className="flex items-center gap-2">
                        {hasConfig && (
                          <Badge variant="outline" className="text-xs">
                            has config
                          </Badge>
                        )}
                        <Badge variant={enabled ? "default" : "secondary"}>
                          {enabled ? (
                            <>
                              <Check className="w-3 h-3 mr-1" />
                              enabled
                            </>
                          ) : (
                            <>
                              <X className="w-3 h-3 mr-1" />
                              disabled
                            </>
                          )}
                        </Badge>
                      </div>
                    </div>
                  );
                })}
              </div>
            ) : (
              <div className="flex items-center justify-between py-4 border-b">
                <div className="space-y-1">
                  <div className="font-medium">Feature Flags</div>
                  <div className="text-sm text-muted-foreground">
                    No feature flags configured
                  </div>
                </div>
                <Badge variant="secondary">none</Badge>
              </div>
            )}

            {/* Configuration Section */}
            <div>
              {configEntries.length > 0 ? (
                <div>
                  {/* Configuration header */}
                  <div className="flex items-center justify-between py-4 border-b">
                    <div className="space-y-1">
                      <div className="font-medium">Feature Configuration</div>
                      <div className="text-sm text-muted-foreground">
                        Configuration options for features that have them
                      </div>
                    </div>
                    <div className="text-sm text-muted-foreground">
                      {configEntries.length} feature{configEntries.length !== 1 ? 's' : ''} configured
                    </div>
                  </div>

                  {/* Configuration rows */}
                  {configEntries.map(([featureName, config], index) => {
                    const configKeys = Object.keys(config);
                    const isLast = index === configEntries.length - 1;
                    return (
                      <div key={featureName} className={`py-4 ${!isLast ? 'border-b' : ''}`}>
                        <div className="flex items-center justify-between mb-3">
                          <div className="space-y-1">
                            <div className="font-medium font-mono text-sm">{featureName}</div>
                            <div className="text-sm text-muted-foreground">
                              {configKeys.length} configuration option{configKeys.length !== 1 ? 's' : ''}
                            </div>
                          </div>
                        </div>
                        {configKeys.length > 0 ? (
                          <div className="space-y-2 ml-4">
                            {Object.entries(config).map(([configKey, configValue]) => (
                              <div key={configKey} className="flex items-center justify-between text-sm">
                                <span className="font-mono text-muted-foreground">{configKey}</span>
                                <span className="font-mono">
                                  {typeof configValue === 'string' 
                                    ? `"${configValue}"` 
                                    : JSON.stringify(configValue)
                                  }
                                </span>
                              </div>
                            ))}
                          </div>
                        ) : (
                          <p className="text-sm text-muted-foreground italic ml-4">No configuration options</p>
                        )}
                      </div>
                    );
                  })}
                </div>
              ) : (
                <div className="flex items-center justify-between py-4">
                  <div className="space-y-1">
                    <div className="font-medium">Feature Configuration</div>
                    <div className="text-sm text-muted-foreground">
                      No feature configuration found
                    </div>
                  </div>
                  <Badge variant="secondary">none</Badge>
                </div>
              )}
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}