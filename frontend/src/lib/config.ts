// Centralized function to get backend URL
export function getBackendUrl(): string {
  return process.env.NEXT_PUBLIC_API_BASE_URL || process.env.NEXT_PUBLIC_BACKEND_URL || 'http://localhost:8080'
}

// API Configuration
export const API_CONFIG = {
  baseUrl: getBackendUrl(),
  endpoints: {
    streamSources: '/api/v1/sources/stream',
    epgSources: '/api/v1/sources/epg',
    proxies: '/api/v1/proxies',
    filters: '/api/v1/filters',
    dataMapping: '/api/v1/data-mapping',
    logos: '/api/v1/logos',
    relays: '/api/v1/relay',
    dashboard: '/api/v1/dashboard/metrics',
    health: '/health'
  }
} as const

// Request timeout in milliseconds
export const REQUEST_TIMEOUT = 30000

// Default pagination settings
export const DEFAULT_PAGE_SIZE = 20
export const MAX_PAGE_SIZE = 100