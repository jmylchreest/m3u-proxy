"use client";

import React, { useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Search, Calendar, Clock, Filter, Play, Grid, List } from 'lucide-react';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { VideoPlayerModal } from '@/components/video-player-modal';

interface EpgProgram {
  id: string;
  channel_id: string;
  channel_name: string;
  channel_logo?: string;
  title: string;
  description?: string;
  start_time: string;
  end_time: string;
  category?: string;
  rating?: string;
  source_id?: string;
  metadata?: Record<string, string>;
}

interface EpgSource {
  id: string;
  name: string;
  url?: string;
  last_updated?: string;
  channel_count: number;
  program_count: number;
}

interface SourceOption {
  id: string;
  name: string;
  type: 'epg_source' | 'stream_source';
  display_name: string;
}

interface EpgProgramsResponse {
  programs: EpgProgram[];
  total: number;
  page: number;
  limit: number;
  has_more: boolean;
}

interface EpgGuideResponse {
  channels: Record<string, { id: string; name: string; logo?: string }>;
  programs: Record<string, EpgProgram[]>;
  time_slots: string[];
  start_time: string;
  end_time: string;
}

export default function EpgPage() {
  const [programs, setPrograms] = useState<EpgProgram[]>([]);
  const [sources, setSources] = useState<EpgSource[]>([]);
  const [sourceOptions, setSourceOptions] = useState<SourceOption[]>([]);
  const [guideData, setGuideData] = useState<EpgGuideResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [debouncedSearch, setDebouncedSearch] = useState('');
  const [selectedSource, setSelectedSource] = useState<string>('');
  const [selectedCategory, setSelectedCategory] = useState<string>('');
  const [activeTab, setActiveTab] = useState<'programs' | 'guide'>('programs');
  const [currentPage, setCurrentPage] = useState(1);
  const [total, setTotal] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [categories, setCategories] = useState<string[]>([]);
  const [timeRange, setTimeRange] = useState<'today' | 'tomorrow' | 'week'>('today');
  const [selectedProgram, setSelectedProgram] = useState<EpgProgram | null>(null);
  const [isPlayerOpen, setIsPlayerOpen] = useState(false);
  const loadMoreRef = useRef<HTMLDivElement>(null);

  const getTimeRangeParams = useCallback(() => {
    const now = new Date();
    let startTime: Date;
    let endTime: Date;

    switch (timeRange) {
      case 'today':
        startTime = new Date(now.getFullYear(), now.getMonth(), now.getDate());
        endTime = new Date(startTime.getTime() + 24 * 60 * 60 * 1000);
        break;
      case 'tomorrow':
        startTime = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
        endTime = new Date(startTime.getTime() + 24 * 60 * 60 * 1000);
        break;
      case 'week':
        startTime = new Date(now.getFullYear(), now.getMonth(), now.getDate());
        endTime = new Date(startTime.getTime() + 7 * 24 * 60 * 60 * 1000);
        break;
    }

    return {
      start_time: startTime.toISOString(),
      end_time: endTime.toISOString(),
    };
  }, [timeRange]);

  // Debounce search input to prevent excessive API calls and focus loss
  useEffect(() => {
    const timer = setTimeout(() => {
      setDebouncedSearch(search);
    }, 300); // 300ms debounce

    return () => clearTimeout(timer);
  }, [search]);

  const fetchPrograms = useCallback(async (searchTerm: string = '', sourceId: string = '', category: string = '', pageNum: number = 1, append: boolean = false) => {
    try {
      setLoading(true);
      
      const params = new URLSearchParams({
        page: pageNum.toString(),
        limit: '50',
        ...getTimeRangeParams(),
      });
      
      if (searchTerm) params.append('search', searchTerm);
      if (sourceId) params.append('source_id', sourceId);
      if (category) params.append('category', category);

      const response = await fetch(`/api/v1/epg/programs?${params}`);
      
      if (!response.ok) {
        throw new Error(`Failed to fetch programs: ${response.statusText}`);
      }
      
      const data: { success: boolean; data: EpgProgramsResponse } = await response.json();
      
      if (!data.success) {
        throw new Error('API returned unsuccessful response');
      }

      if (append) {
        setPrograms(prev => {
          // Deduplicate by ID
          const existing = new Set(prev.map(program => program.id));
          const newPrograms = data.data.programs.filter(program => !existing.has(program.id));
          return [...prev, ...newPrograms];
        });
      } else {
        setPrograms(data.data.programs);
      }

      setCurrentPage(pageNum);
      setTotal(data.data.total);
      setHasMore(data.data.has_more);
      
      // Extract unique categories for filtering - only update on fresh fetch
      if (!append) {
        const uniqueCategories = Array.from(
          new Set(data.data.programs.map(p => p.category).filter(Boolean))
        ) as string[];
        setCategories(uniqueCategories);
      }
      
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'An error occurred');
      if (!append) {
        setPrograms([]);
      }
    } finally {
      setLoading(false);
    }
  }, [getTimeRangeParams]);

  const fetchSources = async () => {
    try {
      const options: SourceOption[] = [];

      // Fetch EPG Sources
      try {
        const epgSourcesResponse = await fetch('/api/v1/sources/epg');
        if (epgSourcesResponse.ok) {
          const epgSourcesData: { success: boolean; data: { items: any[] } } = await epgSourcesResponse.json();
          if (epgSourcesData.success && epgSourcesData.data.items) {
            // Store EPG sources for backward compatibility
            setSources(epgSourcesData.data.items);
            
            epgSourcesData.data.items
              .filter(source => source.is_active)
              .forEach(source => {
                options.push({
                  id: source.id,
                  name: source.name,
                  type: 'epg_source',
                  display_name: `${source.name} (${source.source_type.toUpperCase()})`,
                });
              });
          }
        }
      } catch (err) {
        console.warn('Failed to fetch EPG sources:', err);
      }

      // Fetch Stream Sources that may have EPG data
      try {
        const streamSourcesResponse = await fetch('/api/v1/sources/stream');
        if (streamSourcesResponse.ok) {
          const streamSourcesData: { success: boolean; data: { items: any[] } } = await streamSourcesResponse.json();
          if (streamSourcesData.success && streamSourcesData.data.items) {
            streamSourcesData.data.items
              .filter(source => source.is_active && source.source_type === 'xtream') // Only Xtream has EPG
              .forEach(source => {
                options.push({
                  id: source.id,
                  name: source.name,
                  type: 'stream_source',
                  display_name: `${source.name} (Xtream EPG)`,
                });
              });
          }
        }
      } catch (err) {
        console.warn('Failed to fetch stream sources:', err);
      }

      setSourceOptions(options);
    } catch (err) {
      console.error('Failed to fetch sources:', err);
    }
  };

  const fetchGuideData = async () => {
    try {
      setLoading(true);
      
      const params = new URLSearchParams(getTimeRangeParams());
      if (selectedSource) params.append('source_id', selectedSource);

      const response = await fetch(`/api/v1/epg/guide?${params}`);
      
      if (!response.ok) {
        throw new Error(`Failed to fetch guide data: ${response.statusText}`);
      }
      
      const data: { success: boolean; data: EpgGuideResponse } = await response.json();
      
      if (!data.success) {
        throw new Error('API returned unsuccessful response');
      }

      setGuideData(data.data);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'An error occurred');
      setGuideData(null);
    } finally {
      setLoading(false);
    }
  };

  const handleLoadMore = useCallback(() => {
    if (hasMore && !loading && activeTab === 'programs') {
      fetchPrograms(debouncedSearch, selectedSource, selectedCategory, currentPage + 1, true);
    }
  }, [hasMore, loading, activeTab, debouncedSearch, selectedSource, selectedCategory, currentPage, fetchPrograms]);

  useEffect(() => {
    fetchSources();
  }, []);

  // Handle search changes without losing focus
  const performProgramSearch = useCallback(() => {
    if (activeTab === 'programs') {
      setPrograms([]);
      setCurrentPage(1);
      fetchPrograms(debouncedSearch, selectedSource, selectedCategory, 1, false);
    }
  }, [activeTab, debouncedSearch, selectedSource, selectedCategory, fetchPrograms]);

  useEffect(() => {
    performProgramSearch();
  }, [debouncedSearch]); // Only trigger on debounced search changes

  // Handle non-search filter changes
  useEffect(() => {
    if (activeTab === 'programs') {
      setPrograms([]);
      setCurrentPage(1);
      fetchPrograms(debouncedSearch, selectedSource, selectedCategory, 1, false);
    } else {
      fetchGuideData();
    }
  }, [selectedSource, selectedCategory, timeRange, activeTab]); // Only depend on non-search filters

  // Intersection observer for infinite scroll - only for programs tab
  useEffect(() => {
    const loadMoreElement = loadMoreRef.current;
    if (!loadMoreElement || activeTab !== 'programs') return;

    const observer = new IntersectionObserver(
      (entries) => {
        const [entry] = entries;
        // Trigger load more when the element comes into view and we have more data
        // Only trigger on intersection, not when search changes to prevent focus loss
        if (entry.isIntersecting && hasMore && !loading && !debouncedSearch && activeTab === 'programs') {
          console.log('[EPG] Loading more programs via infinite scroll');
          handleLoadMore();
        }
      },
      {
        // Trigger when the element is 200px away from being visible
        rootMargin: '200px',
        threshold: 0.1,
      }
    );

    observer.observe(loadMoreElement);

    return () => {
      observer.unobserve(loadMoreElement);
    };
  }, [hasMore, loading, debouncedSearch, activeTab, handleLoadMore]);

  const handleSearch = (value: string) => {
    setSearch(value);
  };

  const handleSourceFilter = (value: string) => {
    setSelectedSource(value === 'all' ? '' : value);
  };

  const handleCategoryFilter = (value: string) => {
    setSelectedCategory(value === 'all' ? '' : value);
  };

  const handlePlayProgram = async (program: EpgProgram) => {
    try {
      // Try to get channel stream URL
      const response = await fetch(`/api/v1/channels/${program.channel_id}/stream`);
      if (!response.ok) {
        throw new Error('Failed to get stream URL');
      }
      
      const data = await response.json();
      if (data.success && data.data.stream_url) {
        // Convert relative URLs to absolute URLs
        let streamUrl = data.data.stream_url;
        if (streamUrl.startsWith('/')) {
          streamUrl = `${window.location.origin}${streamUrl}`;
        }
        
        // Update the program with the resolved stream URL for the video player
        const updatedProgram = {
          ...program,
          stream_url: streamUrl
        };
        
        setSelectedProgram(updatedProgram);
        setIsPlayerOpen(true);
      } else {
        throw new Error('No stream URL available for this channel');
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load stream');
    }
  };

  const formatTime = (timeString: string) => {
    return new Date(timeString).toLocaleTimeString([], { 
      hour: '2-digit', 
      minute: '2-digit' 
    });
  };

  const formatDate = (timeString: string) => {
    return new Date(timeString).toLocaleDateString([], {
      weekday: 'short',
      month: 'short',
      day: 'numeric'
    });
  };

  const ProgramCard = ({ program }: { program: EpgProgram }) => {
    const now = new Date();
    const startTime = new Date(program.start_time);
    const endTime = new Date(program.end_time);
    const isLive = now >= startTime && now <= endTime;
    const isUpcoming = startTime > now;

    return (
      <Card className={`transition-all duration-200 hover:shadow-lg ${isLive ? 'ring-2 ring-primary' : ''}`}>
        <CardHeader className="pb-3">
          <div className="flex items-start justify-between">
            <div className="flex-1 min-w-0">
              <div className="flex items-center space-x-2 mb-1">
                <Badge variant={isLive ? 'default' : isUpcoming ? 'secondary' : 'outline'} className="text-xs">
                  {isLive ? 'LIVE' : isUpcoming ? 'UPCOMING' : 'PAST'}
                </Badge>
                {program.category && (
                  <Badge variant="outline" className="text-xs">
                    {program.category}
                  </Badge>
                )}
              </div>
              <CardTitle className="text-sm font-medium line-clamp-2">
                {program.title}
              </CardTitle>
              <CardDescription className="text-xs">
                {program.channel_name} • {formatDate(program.start_time)}
              </CardDescription>
            </div>
            {program.channel_logo && (
              <img
                src={program.channel_logo}
                alt={program.channel_name}
                className="w-8 h-8 object-contain ml-2 flex-shrink-0"
                onError={(e) => {
                  (e.target as HTMLImageElement).style.display = 'none';
                }}
              />
            )}
          </div>
        </CardHeader>
        <CardContent className="pt-0">
          <div className="flex items-center justify-between mb-3">
            <div className="flex items-center text-xs text-muted-foreground">
              <Clock className="w-3 h-3 mr-1" />
              {formatTime(program.start_time)} - {formatTime(program.end_time)}
            </div>
            {program.rating && (
              <Badge variant="outline" className="text-xs">
                {program.rating}
              </Badge>
            )}
          </div>
          
          {program.description && (
            <p className="text-xs text-muted-foreground line-clamp-2 mb-3">
              {program.description}
            </p>
          )}

          <div className="flex justify-between items-center">
            <div className="text-xs text-muted-foreground">
              Channel: {program.channel_name}
            </div>
            <Button
              size="sm"
              onClick={() => handlePlayProgram(program)}
              disabled={!isLive && !isUpcoming}
            >
              <Play className="w-4 h-4" />
            </Button>
          </div>
        </CardContent>
      </Card>
    );
  };

  if (loading && programs.length === 0 && !guideData) {
    return (
      <div className="container mx-auto p-6">
        <div className="flex items-center justify-center h-64">
          <div className="text-center">
            <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-primary mx-auto"></div>
            <p className="mt-4 text-muted-foreground">Loading EPG data...</p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="container mx-auto p-6">
      <div className="mb-6">
        <h1 className="text-3xl font-bold mb-2">EPG Viewer</h1>
        <p className="text-muted-foreground">
          Browse electronic program guide and schedule information
        </p>
      </div>

      <Tabs value={activeTab} onValueChange={(v: string) => setActiveTab(v as 'programs' | 'guide')} className="space-y-6">
        <TabsList className="grid w-full grid-cols-2 max-w-sm">
          <TabsTrigger value="programs" className="flex items-center">
            <List className="w-4 h-4 mr-2" />
            Programs
          </TabsTrigger>
          <TabsTrigger value="guide" className="flex items-center">
            <Grid className="w-4 h-4 mr-2" />
            TV Guide
          </TabsTrigger>
        </TabsList>

        {/* Common Filters */}
        <div className="space-y-4">
          <div className="flex flex-col sm:flex-row gap-4">
            <Select value={timeRange} onValueChange={(v) => setTimeRange(v as 'today' | 'tomorrow' | 'week')}>
              <SelectTrigger className="w-full sm:w-48">
                <Calendar className="w-4 h-4 mr-2" />
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="today">Today</SelectItem>
                <SelectItem value="tomorrow">Tomorrow</SelectItem>
                <SelectItem value="week">This Week</SelectItem>
              </SelectContent>
            </Select>

            <Select value={selectedSource || 'all'} onValueChange={handleSourceFilter}>
              <SelectTrigger className="w-full sm:w-48">
                <Filter className="w-4 h-4 mr-2" />
                <SelectValue placeholder="Filter by source" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">All Sources</SelectItem>
                {sourceOptions.map((option) => (
                  <SelectItem key={option.id} value={option.id}>
                    {option.display_name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>

        <TabsContent value="programs" className="space-y-6">
          {/* Programs Search and Filters */}
          <div className="flex flex-col sm:flex-row gap-4">
            <div className="relative flex-1">
              <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-muted-foreground w-4 h-4" />
              <Input
                placeholder="Search programs..."
                value={search}
                onChange={(e) => handleSearch(e.target.value)}
                className="pl-10"
              />
            </div>
            
            <Select value={selectedCategory || 'all'} onValueChange={handleCategoryFilter}>
              <SelectTrigger className="w-full sm:w-48">
                <Filter className="w-4 h-4 mr-2" />
                <SelectValue placeholder="Filter by category" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">All Categories</SelectItem>
                {categories.map((category) => (
                  <SelectItem key={category} value={category}>
                    {category}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Error Display */}
          {error && (
            <Card className="border-destructive">
              <CardContent className="p-4">
                <p className="text-destructive">{error}</p>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => fetchPrograms(debouncedSearch, selectedSource, selectedCategory, 1, false)}
                  className="mt-2"
                >
                  Retry
                </Button>
              </CardContent>
            </Card>
          )}

          {/* Results Summary */}
          {programs.length > 0 && (
            <div className="text-sm text-muted-foreground">
              Showing {programs.length} of {total} programs
              {hasMore && !loading && (
                <span className="ml-2 text-primary">• {Math.ceil((total - programs.length) / 50)} more pages available</span>
              )}
            </div>
          )}

          {/* Programs Display */}
          {programs.length > 0 ? (
            <>
              <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
                {programs.map((program) => (
                  <ProgramCard key={program.id} program={program} />
                ))}
              </div>

              {/* Progressive Loading */}
              {hasMore && (
                <div ref={loadMoreRef} className="flex justify-center mt-6">
                  <Card className="w-full max-w-md">
                    <CardContent className="p-4 text-center">
                      {loading ? (
                        <div className="flex items-center justify-center space-x-2">
                          <div className="animate-spin rounded-full h-4 w-4 border-2 border-primary border-t-transparent"></div>
                          <p className="text-sm text-muted-foreground">Loading more programs...</p>
                        </div>
                      ) : (
                        <>
                          <p className="text-sm text-muted-foreground mb-2">
                            {Math.ceil((total - programs.length) / 50)} pages remaining
                          </p>
                          <Button 
                            variant="outline" 
                            onClick={handleLoadMore}
                            size="sm"
                            className="gap-2"
                          >
                            Load More Programs
                          </Button>
                        </>
                      )}
                    </CardContent>
                  </Card>
                </div>
              )}
            </>
          ) : !loading && (
            <Card>
              <CardContent className="p-8 text-center">
                <p className="text-muted-foreground">No programs found</p>
                {(search || selectedCategory) && (
                  <Button
                    variant="outline"
                    onClick={() => {
                      setSearch('');
                      setSelectedCategory('');
                    }}
                    className="mt-4"
                  >
                    Clear Filters
                  </Button>
                )}
              </CardContent>
            </Card>
          )}
        </TabsContent>

        <TabsContent value="guide" className="space-y-6">
          {/* TV Guide Grid */}
          {guideData ? (
            <Card>
              <CardHeader>
                <CardTitle>TV Guide Grid</CardTitle>
                <CardDescription>
                  Program schedule grid view
                </CardDescription>
              </CardHeader>
              <CardContent>
                <div className="text-center text-muted-foreground py-8">
                  <Grid className="w-12 h-12 mx-auto mb-4 opacity-50" />
                  <p>TV Guide grid view coming soon</p>
                  <p className="text-sm mt-2">
                    Found {Object.keys(guideData.channels).length} channels with programs
                  </p>
                </div>
              </CardContent>
            </Card>
          ) : !loading && (
            <Card>
              <CardContent className="p-8 text-center">
                <p className="text-muted-foreground">No guide data available</p>
              </CardContent>
            </Card>
          )}
        </TabsContent>
      </Tabs>

      {/* Video Player Modal */}
      {selectedProgram && (
        <VideoPlayerModal
          isOpen={isPlayerOpen}
          onClose={() => {
            setIsPlayerOpen(false);
            setSelectedProgram(null);
          }}
          program={selectedProgram}
        />
      )}
    </div>
  );
}