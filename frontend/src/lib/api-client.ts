import { API_CONFIG, REQUEST_TIMEOUT } from './config'
import { Debug } from '@/utils/debug'
import { 
  ApiResponse, 
  PaginatedResponse, 
  StreamSourceResponse, 
  CreateStreamSourceRequest,
  UpdateStreamSourceRequest,
  EpgSourceResponse,
  CreateEpgSourceRequest,
  StreamProxy,
  CreateStreamProxyRequest,
  UpdateStreamProxyRequest,
  Filter,
  FilterWithMeta,
  FilterTestRequest,
  DataMappingRule,
  RelayProfile,
  RelayHealthApiResponse,
  RuntimeSettings,
  UpdateSettingsRequest,
  SettingsResponse,
  LogoAsset,
  LogoAssetsResponse,
  LogoStats,
  LogoAssetUpdateRequest,
  LogoUploadRequest
} from '@/types/api'

class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
    public response?: any
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

class ApiClient {
  private baseUrl: string
  private debug = Debug.createLogger('ApiClient')

  constructor(baseUrl: string = API_CONFIG.baseUrl) {
    this.baseUrl = baseUrl
  }

  private async request<T>(
    endpoint: string,
    options: RequestInit = {}
  ): Promise<T> {
    const url = `${this.baseUrl}${endpoint}`
    
    // Don't set Content-Type for FormData uploads - let browser set multipart boundary
    const isFormData = options.body instanceof FormData
    const defaultHeaders: Record<string, string> = {
      'Accept': 'application/json'
    }
    
    if (!isFormData) {
      defaultHeaders['Content-Type'] = 'application/json'
    }

    const config: RequestInit = {
      ...options,
      headers: {
        ...defaultHeaders,
        ...options.headers,
      },
    }

    // Add timeout
    const controller = new AbortController()
    const timeoutId = setTimeout(() => controller.abort(), REQUEST_TIMEOUT)
    config.signal = controller.signal

    try {
      const response = await fetch(url, config)
      clearTimeout(timeoutId)

      if (!response.ok) {
        let errorMessage = `HTTP ${response.status}: ${response.statusText}`
        let errorData
        
        try {
          errorData = await response.json()
          if (errorData.error) {
            errorMessage = errorData.error
          }
        } catch {
          // Response is not JSON, use status text
        }

        throw new ApiError(errorMessage, response.status, errorData)
      }

      // Handle empty responses
      if (response.status === 204) {
        return {} as T
      }

      const data = await response.json()
      
      // Handle wrapped responses with success/data structure
      if (data.success && data.data) {
        return data.data
      }
      
      return data
    } catch (error) {
      clearTimeout(timeoutId)
      
      if (error instanceof ApiError) {
        throw error
      }
      
      if (error instanceof Error && error.name === 'AbortError') {
        throw new ApiError('Request timeout', 408)
      }
      
      throw new ApiError(
        error instanceof Error ? error.message : 'Network error occurred',
        0
      )
    }
  }

  // Stream Sources API
  async getStreamSources(params?: {
    page?: number
    limit?: number
    search?: string
    source_type?: string
  }): Promise<PaginatedResponse<StreamSourceResponse>> {
    const searchParams = new URLSearchParams()
    
    if (params?.page) searchParams.set('page', params.page.toString())
    if (params?.limit) searchParams.set('limit', params.limit.toString())
    if (params?.search) searchParams.set('search', params.search)
    if (params?.source_type) searchParams.set('source_type', params.source_type)
    
    const queryString = searchParams.toString()
    const endpoint = `${API_CONFIG.endpoints.streamSources}${queryString ? `?${queryString}` : ''}`
    
    return this.request<PaginatedResponse<StreamSourceResponse>>(endpoint)
  }

  async getStreamSource(id: string): Promise<ApiResponse<StreamSourceResponse>> {
    return this.request<ApiResponse<StreamSourceResponse>>(
      `${API_CONFIG.endpoints.streamSources}/${id}`
    )
  }

  async createStreamSource(source: CreateStreamSourceRequest): Promise<ApiResponse<StreamSourceResponse>> {
    return this.request<ApiResponse<StreamSourceResponse>>(
      API_CONFIG.endpoints.streamSources,
      {
        method: 'POST',
        body: JSON.stringify(source),
      }
    )
  }

  async updateStreamSource(
    id: string, 
    source: UpdateStreamSourceRequest
  ): Promise<ApiResponse<StreamSourceResponse>> {
    return this.request<ApiResponse<StreamSourceResponse>>(
      `${API_CONFIG.endpoints.streamSources}/${id}`,
      {
        method: 'PUT',
        body: JSON.stringify(source),
      }
    )
  }

  async deleteStreamSource(id: string): Promise<void> {
    await this.request<void>(
      `${API_CONFIG.endpoints.streamSources}/${id}`,
      {
        method: 'DELETE',
      }
    )
  }

  async refreshStreamSource(id: string): Promise<void> {
    await this.request<void>(
      `${API_CONFIG.endpoints.streamSources}/${id}/refresh`,
      {
        method: 'POST',
      }
    )
  }

  async validateStreamSource(source: CreateStreamSourceRequest): Promise<any> {
    return this.request<any>(
      `${API_CONFIG.endpoints.streamSources}/validate`,
      {
        method: 'POST',
        body: JSON.stringify(source),
      }
    )
  }

  // EPG Sources API
  async getEpgSources(params?: {
    page?: number
    limit?: number
    search?: string
    source_type?: string
  }): Promise<PaginatedResponse<EpgSourceResponse>> {
    const searchParams = new URLSearchParams()
    
    if (params?.page) searchParams.set('page', params.page.toString())
    if (params?.limit) searchParams.set('limit', params.limit.toString())
    if (params?.search) searchParams.set('search', params.search)
    if (params?.source_type) searchParams.set('source_type', params.source_type)
    
    const queryString = searchParams.toString()
    const endpoint = `${API_CONFIG.endpoints.epgSources}${queryString ? `?${queryString}` : ''}`
    
    return this.request<PaginatedResponse<EpgSourceResponse>>(endpoint)
  }

  async getEpgSource(id: string): Promise<ApiResponse<EpgSourceResponse>> {
    return this.request<ApiResponse<EpgSourceResponse>>(
      `${API_CONFIG.endpoints.epgSources}/${id}`
    )
  }

  async createEpgSource(source: CreateEpgSourceRequest): Promise<ApiResponse<EpgSourceResponse>> {
    return this.request<ApiResponse<EpgSourceResponse>>(
      API_CONFIG.endpoints.epgSources,
      {
        method: 'POST',
        body: JSON.stringify(source),
      }
    )
  }

  async updateEpgSource(
    id: string, 
    source: CreateEpgSourceRequest
  ): Promise<ApiResponse<EpgSourceResponse>> {
    return this.request<ApiResponse<EpgSourceResponse>>(
      `${API_CONFIG.endpoints.epgSources}/${id}`,
      {
        method: 'PUT',
        body: JSON.stringify(source),
      }
    )
  }

  async deleteEpgSource(id: string): Promise<void> {
    await this.request<void>(
      `${API_CONFIG.endpoints.epgSources}/${id}`,
      {
        method: 'DELETE',
      }
    )
  }

  async refreshEpgSource(id: string): Promise<void> {
    await this.request<void>(
      `${API_CONFIG.endpoints.epgSources}/${id}/refresh`,
      {
        method: 'POST',
      }
    )
  }

  // Proxy API
  async getProxies(params?: {
    page?: number
    limit?: number
    search?: string
  }): Promise<PaginatedResponse<StreamProxy>> {
    const searchParams = new URLSearchParams()
    
    if (params?.page) searchParams.set('page', params.page.toString())
    if (params?.limit) searchParams.set('limit', params.limit.toString())
    if (params?.search) searchParams.set('search', params.search)
    
    const queryString = searchParams.toString()
    const endpoint = `${API_CONFIG.endpoints.proxies}${queryString ? `?${queryString}` : ''}`
    
    return this.request<PaginatedResponse<StreamProxy>>(endpoint)
  }

  async getProxy(id: string): Promise<ApiResponse<StreamProxy>> {
    return this.request<ApiResponse<StreamProxy>>(
      `${API_CONFIG.endpoints.proxies}/${id}`
    )
  }

  async createProxy(proxy: CreateStreamProxyRequest): Promise<ApiResponse<StreamProxy>> {
    return this.request<ApiResponse<StreamProxy>>(
      API_CONFIG.endpoints.proxies,
      {
        method: 'POST',
        body: JSON.stringify(proxy),
      }
    )
  }

  async updateProxy(
    id: string, 
    proxy: UpdateStreamProxyRequest
  ): Promise<ApiResponse<StreamProxy>> {
    return this.request<ApiResponse<StreamProxy>>(
      `${API_CONFIG.endpoints.proxies}/${id}`,
      {
        method: 'PUT',
        body: JSON.stringify(proxy),
      }
    )
  }

  async deleteProxy(id: string): Promise<void> {
    await this.request<void>(
      `${API_CONFIG.endpoints.proxies}/${id}`,
      {
        method: 'DELETE',
      }
    )
  }

  async regenerateProxy(id: string): Promise<void> {
    await this.request<void>(
      `${API_CONFIG.endpoints.proxies}/${id}/regenerate`,
      {
        method: 'POST',
      }
    )
  }

  // Proxy association methods - these may or may not exist in the API
  async getProxyStreamSources(proxyId: string): Promise<any[]> {
    try {
      return await this.request<any[]>(
        `${API_CONFIG.endpoints.proxies}/${proxyId}/sources`
      )
    } catch (error) {
      this.debug.warn(`Proxy stream sources endpoint not available for ${proxyId}:`, error)
      return []
    }
  }

  async getProxyEpgSources(proxyId: string): Promise<any[]> {
    try {
      return await this.request<any[]>(
        `${API_CONFIG.endpoints.proxies}/${proxyId}/epg-sources`
      )
    } catch (error) {
      this.debug.warn(`Proxy EPG sources endpoint not available for ${proxyId}:`, error)
      return []
    }
  }

  async getProxyFilters(proxyId: string): Promise<any[]> {
    try {
      return await this.request<any[]>(
        `${API_CONFIG.endpoints.proxies}/${proxyId}/filters`
      )
    } catch (error) {
      this.debug.warn(`Proxy filters endpoint not available for ${proxyId}:`, error)
      return []
    }
  }

  // Filters API
  async getFilters(params?: {
    page?: number
    limit?: number
    search?: string
    source_type?: string
  }): Promise<FilterWithMeta[]> {
    const searchParams = new URLSearchParams()
    
    if (params?.page) searchParams.set('page', params.page.toString())
    if (params?.limit) searchParams.set('limit', params.limit.toString())
    if (params?.search) searchParams.set('search', params.search)
    if (params?.source_type) searchParams.set('source_type', params.source_type)
    
    const queryString = searchParams.toString()
    const endpoint = `${API_CONFIG.endpoints.filters}${queryString ? `?${queryString}` : ''}`
    
    return this.request<FilterWithMeta[]>(endpoint)
  }

  async getFilter(id: string): Promise<ApiResponse<Filter>> {
    return this.request<ApiResponse<Filter>>(`${API_CONFIG.endpoints.filters}/${id}`)
  }

  async createFilter(filter: Omit<Filter, 'id' | 'created_at' | 'updated_at'>): Promise<ApiResponse<Filter>> {
    return this.request<ApiResponse<Filter>>(
      API_CONFIG.endpoints.filters,
      {
        method: 'POST',
        body: JSON.stringify(filter),
      }
    )
  }

  async updateFilter(
    id: string, 
    filter: Omit<Filter, 'id' | 'created_at' | 'updated_at'>
  ): Promise<ApiResponse<Filter>> {
    return this.request<ApiResponse<Filter>>(
      `${API_CONFIG.endpoints.filters}/${id}`,
      {
        method: 'PUT',
        body: JSON.stringify(filter),
      }
    )
  }

  async deleteFilter(id: string): Promise<void> {
    await this.request<void>(
      `${API_CONFIG.endpoints.filters}/${id}`,
      {
        method: 'DELETE',
      }
    )
  }

  async testFilter(testRequest: FilterTestRequest): Promise<any> {
    return this.request<any>(
      `${API_CONFIG.endpoints.filters}/test`,
      {
        method: 'POST',
        body: JSON.stringify(testRequest),
      }
    )
  }

  async validateFilter(filterExpression: string): Promise<{ valid: boolean; error?: string; match_count?: number }> {
    return this.request<{ valid: boolean; error?: string; match_count?: number }>(
      `${API_CONFIG.endpoints.filters}/validate`,
      {
        method: 'POST',
        body: JSON.stringify({ filter_expression: filterExpression }),
      }
    )
  }

  async getFilterFields(): Promise<string[]> {
    return this.request<string[]>(`${API_CONFIG.endpoints.filters}/fields`)
  }

  // Data Mapping API
  async getDataMappingRules(params?: {
    page?: number
    limit?: number
    search?: string
    source_type?: string
  }): Promise<DataMappingRule[]> {
    const searchParams = new URLSearchParams()
    
    if (params?.page) searchParams.set('page', params.page.toString())
    if (params?.limit) searchParams.set('limit', params.limit.toString())
    if (params?.search) searchParams.set('search', params.search)
    if (params?.source_type) searchParams.set('source_type', params.source_type)
    
    const queryString = searchParams.toString()
    const endpoint = `${API_CONFIG.endpoints.dataMapping}${queryString ? `?${queryString}` : ''}`
    
    return this.request<DataMappingRule[]>(endpoint)
  }

  async getDataMappingRule(id: string): Promise<ApiResponse<DataMappingRule>> {
    return this.request<ApiResponse<DataMappingRule>>(`${API_CONFIG.endpoints.dataMapping}/${id}`)
  }

  async createDataMappingRule(rule: Omit<DataMappingRule, 'id' | 'created_at' | 'updated_at'>): Promise<ApiResponse<DataMappingRule>> {
    return this.request<ApiResponse<DataMappingRule>>(
      API_CONFIG.endpoints.dataMapping,
      {
        method: 'POST',
        body: JSON.stringify(rule),
      }
    )
  }

  async updateDataMappingRule(
    id: string, 
    rule: Omit<DataMappingRule, 'id' | 'created_at' | 'updated_at'>
  ): Promise<ApiResponse<DataMappingRule>> {
    return this.request<ApiResponse<DataMappingRule>>(
      `${API_CONFIG.endpoints.dataMapping}/${id}`,
      {
        method: 'PUT',
        body: JSON.stringify(rule),
      }
    )
  }

  async deleteDataMappingRule(id: string): Promise<void> {
    await this.request<void>(
      `${API_CONFIG.endpoints.dataMapping}/${id}`,
      {
        method: 'DELETE',
      }
    )
  }

  async reorderDataMappingRules(rules: { id: string; sort_order: number }[]): Promise<void> {
    await this.request<void>(
      `${API_CONFIG.endpoints.dataMapping}/reorder`,
      {
        method: 'PUT',
        body: JSON.stringify({ rules }),
      }
    )
  }

  async validateDataMappingExpression(expression: string, sourceType: string): Promise<{ valid: boolean; error?: string }> {
    return this.request<{ valid: boolean; error?: string }>(
      `/api/v1/expressions/validate/data-mapping`,
      {
        method: 'POST',
        body: JSON.stringify({ 
          expression,
          source_type: sourceType
        }),
      }
    )
  }

  async getDataMappingFields(sourceType: string): Promise<string[]> {
    return this.request<string[]>(`${API_CONFIG.endpoints.dataMapping}/fields/${sourceType}`)
  }

  async testDataMappingRule(testRequest: {
    source_id: string;
    source_type: string;
    expression: string;
  }): Promise<any> {
    return this.request<any>(
      `${API_CONFIG.endpoints.dataMapping}/test`,
      {
        method: 'POST',
        body: JSON.stringify(testRequest),
      }
    )
  }

  async previewDataMappingRule(previewRequest: {
    source_id?: string;
    source_type: string;
    expression: string;
    sample_data?: any;
  }): Promise<any> {
    const method = previewRequest.sample_data ? 'POST' : 'GET'
    const endpoint = `${API_CONFIG.endpoints.dataMapping}/preview`
    
    if (method === 'GET') {
      const searchParams = new URLSearchParams({
        source_type: previewRequest.source_type,
        expression: previewRequest.expression
      })
      if (previewRequest.source_id) {
        searchParams.set('source_id', previewRequest.source_id)
      }
      return this.request<any>(`${endpoint}?${searchParams.toString()}`)
    } else {
      return this.request<any>(endpoint, {
        method: 'POST',
        body: JSON.stringify(previewRequest),
      })
    }
  }

  // Relay Profiles API
  async getRelayProfiles(): Promise<RelayProfile[]> {
    return this.request<RelayProfile[]>(`${API_CONFIG.endpoints.relays}/profiles`)
  }

  // Settings API
  async getSettings(): Promise<SettingsResponse> {
    return this.request<SettingsResponse>('/api/v1/settings')
  }

  async updateSettings(settings: UpdateSettingsRequest): Promise<SettingsResponse> {
    return this.request<SettingsResponse>(
      '/api/v1/settings',
      {
        method: 'PUT',
        body: JSON.stringify(settings),
      }
    )
  }

  async getSettingsInfo(): Promise<any> {
    return this.request<any>('/api/v1/settings/info')
  }

  // Logo endpoints
  async getLogos(params?: {
    page?: number;
    limit?: number;
    include_cached?: boolean;
    search?: string;
  }): Promise<LogoAssetsResponse> {
    const queryParams = new URLSearchParams()
    if (params?.page) queryParams.set('page', params.page.toString())
    if (params?.limit) queryParams.set('limit', params.limit.toString())
    if (params?.include_cached !== undefined) queryParams.set('include_cached', params.include_cached.toString())
    if (params?.search) queryParams.set('search', params.search)
    
    const query = queryParams.toString()
    return this.request(`${API_CONFIG.endpoints.logos}${query ? `?${query}` : ''}`)
  }

  async getLogoStats(): Promise<LogoStats> {
    return this.request(`${API_CONFIG.endpoints.logos}/stats`)
  }

  async deleteLogo(id: string): Promise<void> {
    return this.request(`${API_CONFIG.endpoints.logos}/${id}`, {
      method: 'DELETE'
    })
  }

  async updateLogo(id: string, data: LogoAssetUpdateRequest): Promise<LogoAsset> {
    return this.request(`${API_CONFIG.endpoints.logos}/${id}`, {
      method: 'PUT',
      body: JSON.stringify(data)
    })
  }

  async replaceLogoImage(id: string, file: File, name?: string, description?: string): Promise<LogoAsset> {
    const formData = new FormData()
    formData.append('file', file)
    if (name) formData.append('name', name)
    if (description) formData.append('description', description)
    
    return this.request(`${API_CONFIG.endpoints.logos}/${id}/image`, {
      method: 'PUT',
      body: formData
    })
  }

  async uploadLogo(data: LogoUploadRequest): Promise<LogoAsset> {
    const formData = new FormData()
    formData.append('file', data.file)
    formData.append('name', data.name)
    if (data.description) {
      formData.append('description', data.description)
    }

    return this.request(`${API_CONFIG.endpoints.logos}/upload`, {
      method: 'POST',
      body: formData,
      // Don't set Content-Type header, let the browser set it with boundary
      headers: {}
    })
  }

  // Health check
  async healthCheck(): Promise<any> {
    return this.request<any>(API_CONFIG.endpoints.health)
  }

  // Feature flags API
  async getFeatures(): Promise<{ flags: Record<string, boolean>; config: Record<string, Record<string, any>>; timestamp: string }> {
    return this.request<{ flags: Record<string, boolean>; config: Record<string, Record<string, any>>; timestamp: string }>('/api/v1/features')
  }

  async updateFeatures(data: { flags: Record<string, boolean>; config: Record<string, Record<string, any>> }): Promise<any> {
    return this.request<any>('/api/v1/features', {
      method: 'PUT',
      body: JSON.stringify(data)
    })
  }

  // Relay health check
  async getRelayHealth(): Promise<RelayHealthApiResponse> {
    return this.request<RelayHealthApiResponse>('/api/v1/relay/health')
  }
}

// Export singleton instance
export const apiClient = new ApiClient()
export { ApiError }
export type { ApiClient }