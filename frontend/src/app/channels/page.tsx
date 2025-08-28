"use client";

import React, { useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Search, Play, Filter, Grid, List, Eye, Zap, Check, Table as TableIcon } from 'lucide-react';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Checkbox } from '@/components/ui/checkbox';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger, DropdownMenuSeparator } from '@/components/ui/dropdown-menu';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { VideoPlayerModal } from '@/components/video-player-modal';

interface Channel {
  id: string;
  name: string;
  logo_url?: string;
  group?: string;
  stream_url: string;
  proxy_id?: string;
  source_type: string;
  source_name?: string;
  // M3U specific fields
  tvg_id?: string;
  tvg_name?: string;
  tvg_chno?: string;
  tvg_shift?: string;
  // Codec information
  video_codec?: string;
  audio_codec?: string;
  resolution?: string;
  last_probed_at?: string;
  probe_method?: string;
}

interface ChannelsResponse {
  channels: Channel[];
  total: number;
  page: number;
  limit: number;
  has_more: boolean;
}

// Helper functions for date formatting
const formatRelativeTime = (dateString: string): string => {
  const date = new Date(dateString);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffSeconds = Math.floor(diffMs / 1000);
  const diffMinutes = Math.floor(diffSeconds / 60);
  const diffHours = Math.floor(diffMinutes / 60);
  const diffDays = Math.floor(diffHours / 24);

  if (diffSeconds < 60) return 'Just now';
  if (diffMinutes < 60) return `${diffMinutes}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  if (diffDays < 7) return `${diffDays}d ago`;
  
  return date.toLocaleDateString();
};

const formatPreciseTime = (dateString: string): string => {
  const date = new Date(dateString);
  return date.toLocaleString();
};

export default function ChannelsPage() {
  const [channels, setChannels] = useState<Channel[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [debouncedSearch, setDebouncedSearch] = useState('');
  const [selectedGroup, setSelectedGroup] = useState<string>('');
  const [selectedSources, setSelectedSources] = useState<string[]>([]);
  const [viewMode, setViewMode] = useState<'grid' | 'list' | 'table'>('table');
  const [currentPage, setCurrentPage] = useState(1);
  const [total, setTotal] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [groups, setGroups] = useState<string[]>([]);
  const [sources, setSources] = useState<string[]>([]);
  const [selectedChannel, setSelectedChannel] = useState<Channel | null>(null);
  const [isPlayerOpen, setIsPlayerOpen] = useState(false);
  const [probingChannels, setProbingChannels] = useState<Set<string>>(new Set());
  const loadMoreRef = useRef<HTMLDivElement>(null);
  const isSearchChangeRef = useRef(false);

  // No longer need proxy resolution - only using direct stream sources

  // Debounce search input to prevent excessive API calls and focus loss
  useEffect(() => {
    const timer = setTimeout(() => {
      isSearchChangeRef.current = true; // Mark as search change
      setDebouncedSearch(search);
    }, 300); // 300ms debounce

    return () => clearTimeout(timer);
  }, [search]);

  const fetchChannels = useCallback(async (searchTerm: string = '', group: string = '', pageNum: number = 1, append: boolean = false, isSearchChange: boolean = false) => {
    try {
      setLoading(true);
      
      const params = new URLSearchParams({
        page: pageNum.toString(),
        limit: '200',
      });
      
      if (searchTerm) params.append('search', searchTerm);
      if (group) params.append('group', group);

      let apiUrl = '/api/v1/channels';

      const response = await fetch(`${apiUrl}?${params}`);
      
      if (!response.ok) {
        throw new Error(`Failed to fetch channels: ${response.statusText}`);
      }
      
      const data: { success: boolean; data: ChannelsResponse } = await response.json();
      
      if (!data.success) {
        throw new Error('API returned unsuccessful response');
      }

      let channelsData = data.data.channels;
      
      if (append) {
        setChannels(prev => {
          // Deduplicate by ID
          const existing = new Set(prev.map(channel => channel.id));
          const newChannels = channelsData.filter(channel => !existing.has(channel.id));
          return [...prev, ...newChannels];
        });
      } else if (isSearchChange && pageNum === 1) {
        // For search changes, replace the list but don't trigger a full page refresh
        setChannels(channelsData);
      } else {
        setChannels(channelsData);
      }

      setCurrentPage(pageNum);
      setTotal(data.data.total);
      setHasMore(data.data.has_more);
      
      // Extract unique groups and sources for filtering - only update on fresh fetch
      if (!append) {
        const uniqueGroups = Array.from(
          new Set(data.data.channels.map(c => c.group).filter(Boolean))
        ) as string[];
        setGroups(uniqueGroups);
        
        const uniqueSources = Array.from(
          new Set(data.data.channels.map(c => c.source_name).filter(Boolean))
        ) as string[];
        setSources(uniqueSources);
      }
      
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'An error occurred');
      if (!append) {
        setChannels([]);
      }
    } finally {
      setLoading(false);
    }
  }, []);

  const handleLoadMore = useCallback(() => {
    if (hasMore && !loading) {
      fetchChannels(debouncedSearch, selectedGroup, currentPage + 1, true, false);
    }
  }, [hasMore, loading, debouncedSearch, selectedGroup, selectedSources, currentPage, fetchChannels]);

  // Single effect that handles both search and filter changes intelligently
  useEffect(() => {
    if (isSearchChangeRef.current) {
      // This is a search change - don't clear channels to prevent focus loss
      isSearchChangeRef.current = false; // Reset the flag
      fetchChannels(debouncedSearch, selectedGroup, 1, false, true);
    } else {
      // This is a filter change - clear channels
      setChannels([]);
      setCurrentPage(1);
      fetchChannels(debouncedSearch, selectedGroup, 1, false, false);
    }
  }, [debouncedSearch, selectedGroup, selectedSources, fetchChannels]);

  // Intersection observer for infinite scroll
  useEffect(() => {
    const loadMoreElement = loadMoreRef.current;
    if (!loadMoreElement) return;

    const observer = new IntersectionObserver(
      (entries) => {
        const [entry] = entries;
        // Trigger load more when the element comes into view and we have more data
        // Only trigger on intersection, not when search changes to prevent focus loss
        if (entry.isIntersecting && hasMore && !loading && !debouncedSearch) {
          console.log('[Channels] Loading more items via infinite scroll');
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
  }, [hasMore, loading, debouncedSearch, handleLoadMore]);

  const handleSearch = (value: string) => {
    setSearch(value);
  };

  const handleGroupFilter = (value: string) => {
    setSelectedGroup(value === 'all' ? '' : value);
  };

  const handleSourceToggle = (sourceName: string) => {
    setSelectedSources(prev => {
      if (prev.includes(sourceName)) {
        return prev.filter(s => s !== sourceName);
      } else {
        return [...prev, sourceName];
      }
    });
  };

  const handleAllSourcesToggle = () => {
    if (selectedSources.length === sources.length) {
      setSelectedSources([]);
    } else {
      setSelectedSources([...sources]);
    }
  };


  const handlePlayChannel = async (channel: Channel) => {
    try {
      // Use the new unified channel streaming endpoint (directly streams content, no CORS issues)
      const streamUrl = `/channel/${channel.id}/stream`;
      
      setSelectedChannel({
        ...channel,
        stream_url: streamUrl
      });
      setIsPlayerOpen(true);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load stream');
    }
  };

  const handleProbeChannel = async (channel: Channel) => {
    try {
      setProbingChannels(prev => new Set(prev).add(channel.id));
      
      const response = await fetch(`/api/v1/channels/${channel.id}/probe`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
      });
      
      if (!response.ok) {
        throw new Error(`Failed to probe channel: ${response.statusText}`);
      }
      
      const result = await response.json();
      
      if (result.success === 'true' || result.success === true) {
        // Update the specific channel in the local state with the new codec information
        setChannels(prev => prev.map(ch => {
          if (ch.id === channel.id) {
            return {
              ...ch,
              video_codec: result.data?.video_codec || ch.video_codec,
              audio_codec: result.data?.audio_codec || ch.audio_codec,
              resolution: result.data?.resolution || ch.resolution,
              last_probed_at: new Date().toISOString(),
              probe_method: 'ffprobe_manual'
            };
          }
          return ch;
        }));
        setError(null);
      } else {
        setError(result.data?.error || result.error || 'Failed to probe channel');
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to probe channel');
    } finally {
      setProbingChannels(prev => {
        const newSet = new Set(prev);
        newSet.delete(channel.id);
        return newSet;
      });
    }
  };

  const LogoWithPopover = ({ channel }: { channel: Channel }) => {
    const [imageError, setImageError] = useState(false);
    const [popoverImageError, setPopoverImageError] = useState(false);

    if (!channel.logo_url || imageError) {
      return <div className="w-8 h-8 bg-muted rounded flex items-center justify-center text-muted-foreground text-xs">
        No Logo
      </div>;
    }

    return (
      <Popover>
        <PopoverTrigger asChild>
          <div className="cursor-pointer">
            <img
              src={channel.logo_url}
              alt={channel.name}
              className="w-8 h-8 object-contain rounded hover:scale-110 transition-transform"
              onError={() => setImageError(true)}
            />
          </div>
        </PopoverTrigger>
        <PopoverContent className="w-80">
          <div className="space-y-2">
            <h4 className="font-semibold">{channel.name}</h4>
            {popoverImageError ? (
              <div className="w-full max-w-64 h-32 bg-muted rounded flex items-center justify-center mx-auto">
                <span className="text-muted-foreground text-sm">Logo not available</span>
              </div>
            ) : (
              <img
                src={channel.logo_url}
                alt={channel.name}
                className="w-full max-w-64 h-auto object-contain mx-auto"
                onError={() => setPopoverImageError(true)}
              />
            )}
          </div>
        </PopoverContent>
      </Popover>
    );
  };

  const ChannelTableRow = ({ channel }: { channel: Channel }) => (
    <TableRow className="hover:bg-muted/50">
      <TableCell className="w-16">
        <LogoWithPopover channel={channel} />
      </TableCell>
      <TableCell className="font-medium max-w-xs">
        <div className="truncate" title={channel.name}>
          {channel.name}
        </div>
      </TableCell>
      <TableCell className="text-sm">
        {channel.tvg_chno || <span className="text-muted-foreground">-</span>}
      </TableCell>
      <TableCell>
        {channel.group ? (
          <Badge variant="secondary" className="text-xs">
            {channel.group}
          </Badge>
        ) : (
          <span className="text-muted-foreground">-</span>
        )}
      </TableCell>
      <TableCell className="text-sm">
        {channel.source_name || channel.source_type}
      </TableCell>
      <TableCell className="text-sm">
        {channel.video_codec || <span className="text-muted-foreground">-</span>}
      </TableCell>
      <TableCell className="text-sm">
        {channel.audio_codec || <span className="text-muted-foreground">-</span>}
      </TableCell>
      <TableCell className="text-sm">
        {channel.resolution || <span className="text-muted-foreground">-</span>}
      </TableCell>
      <TableCell className="text-sm">
        {channel.last_probed_at ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="cursor-help text-xs">
                {formatRelativeTime(channel.last_probed_at)}
              </span>
            </TooltipTrigger>
            <TooltipContent>
              <div className="space-y-1">
                <div>Method: {channel.probe_method || 'Unknown'}</div>
                <div>Precise time: {formatPreciseTime(channel.last_probed_at)}</div>
              </div>
            </TooltipContent>
          </Tooltip>
        ) : (
          <span className="text-muted-foreground">-</span>
        )}
      </TableCell>
      <TableCell className="w-32">
        <div className="flex gap-1">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="sm"
                onClick={() => handlePlayChannel(channel)}
                className="h-8 px-2"
              >
                <Play className="w-3 h-3" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              <p>Play channel</p>
            </TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="sm"
                variant="outline"
                onClick={() => handleProbeChannel(channel)}
                disabled={probingChannels.has(channel.id)}
                className="h-8 px-2"
              >
                {probingChannels.has(channel.id) ? (
                  <div className="w-3 h-3 animate-spin rounded-full border-2 border-primary border-t-transparent" />
                ) : (
                  <Zap className="w-3 h-3" />
                )}
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              <p>Probe codec information</p>
            </TooltipContent>
          </Tooltip>
        </div>
      </TableCell>
    </TableRow>
  );

  const ChannelCard = ({ channel }: { channel: Channel }) => (
    <Card className="transition-all duration-200 hover:shadow-lg hover:scale-105">
      <CardHeader className="pb-2">
        <div className="flex items-start justify-between">
          <div className="flex-1 min-w-0">
            <CardTitle className="text-sm font-medium truncate">
              {channel.name}
            </CardTitle>
            {channel.group && (
              <CardDescription className="mt-1">
                <Badge variant="secondary" className="text-xs">
                  {channel.group}
                </Badge>
              </CardDescription>
            )}
          </div>
          {channel.logo_url && (
            <img
              src={channel.logo_url}
              alt={channel.name}
              className="w-8 h-8 object-contain ml-2 flex-shrink-0"
              onError={(e) => {
                const img = e.target as HTMLImageElement;
                img.style.display = 'none';
              }}
            />
          )}
        </div>
      </CardHeader>
      <CardContent className="pt-0">
        <div className="flex justify-between items-center">
          <div className="flex flex-col text-xs text-muted-foreground">
            <span>Source: {channel.source_name || channel.source_type}</span>
            {channel.tvg_chno && <span>Channel #: {channel.tvg_chno}</span>}
            {channel.video_codec && (
              <div className="flex gap-1 mt-1">
                <Badge variant="outline" className="text-xs">
                  {channel.video_codec}
                </Badge>
                {channel.audio_codec && (
                  <Badge variant="outline" className="text-xs">
                    {channel.audio_codec}
                  </Badge>
                )}
              </div>
            )}
          </div>
          <div className="flex gap-1 ml-2">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  size="sm"
                  onClick={() => handlePlayChannel(channel)}
                >
                  <Play className="w-4 h-4" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                <p>Play channel</p>
              </TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => handleProbeChannel(channel)}
                  disabled={probingChannels.has(channel.id)}
                >
                  {probingChannels.has(channel.id) ? (
                    <div className="w-4 h-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
                  ) : (
                    <Zap className="w-4 h-4" />
                  )}
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                <p>Probe codec information</p>
              </TooltipContent>
            </Tooltip>
          </div>
        </div>
      </CardContent>
    </Card>
  );

  const ChannelListItem = ({ channel }: { channel: Channel }) => (
    <Card className="transition-all duration-200 hover:shadow-md">
      <CardContent className="p-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center space-x-4">
            {channel.logo_url && (
              <img
                src={channel.logo_url}
                alt={channel.name}
                className="w-10 h-10 object-contain"
                onError={(e) => {
                  const img = e.target as HTMLImageElement;
                  img.style.display = 'none';
                }}
              />
            )}
            <div>
              <h3 className="font-medium">{channel.name}</h3>
              <div className="flex items-center space-x-2 text-sm text-muted-foreground">
                {channel.group && (
                  <Badge variant="secondary" className="text-xs">
                    {channel.group}
                  </Badge>
                )}
                <span>Source: {channel.source_name || channel.source_type}</span>
                {channel.tvg_chno && <span>• Ch #{channel.tvg_chno}</span>}
                {channel.video_codec && (
                  <>
                    <span>•</span>
                    <Badge variant="outline" className="text-xs">
                      {channel.video_codec}
                    </Badge>
                  </>
                )}
                {channel.audio_codec && (
                  <Badge variant="outline" className="text-xs">
                    {channel.audio_codec}
                  </Badge>
                )}
              </div>
            </div>
          </div>
          <div className="flex gap-2">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  size="sm"
                  onClick={() => handlePlayChannel(channel)}
                >
                  <Play className="w-4 h-4 mr-2" />
                  Play
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                <p>Play channel</p>
              </TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => handleProbeChannel(channel)}
                  disabled={probingChannels.has(channel.id)}
                >
                  {probingChannels.has(channel.id) ? (
                    <div className="w-4 h-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
                  ) : (
                    <Zap className="w-4 h-4" />
                  )}
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                <p>Probe codec information</p>
              </TooltipContent>
            </Tooltip>
          </div>
        </div>
      </CardContent>
    </Card>
  );

  if (loading && channels.length === 0) {
    return (
      <div className="container mx-auto p-6">
        <div className="flex items-center justify-center h-64">
          <div className="text-center">
            <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-primary mx-auto"></div>
            <p className="mt-4 text-muted-foreground">Loading channels...</p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <TooltipProvider>
      <div className="container mx-auto p-6">
      <div className="mb-6">
        <p className="text-muted-foreground">
          Browse and play channels with detailed information and metadata
        </p>
      </div>

      {/* Search and Filters */}
      <Card className="mb-6">
        <CardContent className="p-6">
          <div className="flex flex-col sm:flex-row gap-4">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-muted-foreground w-4 h-4" />
            <Input
              placeholder="Search channels..."
              value={search}
              onChange={(e) => handleSearch(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                }
              }}
              className="pl-10"
            />
          </div>
          
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" className="w-full sm:w-48 justify-between">
                <div className="flex items-center">
                  <Filter className="w-4 h-4 mr-2" />
                  <span>
                    {selectedSources.length === 0 
                      ? 'All Sources' 
                      : selectedSources.length === sources.length 
                        ? 'All Sources' 
                        : `${selectedSources.length} Source${selectedSources.length > 1 ? 's' : ''}`
                    }
                  </span>
                </div>
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-56">
              <DropdownMenuItem onClick={handleAllSourcesToggle}>
                <Checkbox 
                  checked={selectedSources.length === sources.length && sources.length > 0}
                  className="mr-2"
                />
                All Sources
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              {sources.map((source) => (
                <DropdownMenuItem key={source} onClick={() => handleSourceToggle(source)}>
                  <Checkbox 
                    checked={selectedSources.includes(source)}
                    className="mr-2"
                  />
                  {source}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>


          <div className="flex bg-muted rounded-lg p-1">
            <Button
              variant={viewMode === 'table' ? 'default' : 'ghost'}
              size="sm"
              onClick={() => setViewMode('table')}
              title="Table view"
            >
              <TableIcon className="w-4 h-4" />
            </Button>
            <Button
              variant={viewMode === 'grid' ? 'default' : 'ghost'}
              size="sm"
              onClick={() => setViewMode('grid')}
              title="Grid view"
            >
              <Grid className="w-4 h-4" />
            </Button>
            <Button
              variant={viewMode === 'list' ? 'default' : 'ghost'}
              size="sm"
              onClick={() => setViewMode('list')}
              title="Compact list view"
            >
              <List className="w-4 h-4" />
            </Button>
          </div>
          </div>
        </CardContent>
      </Card>

      {/* Error Display */}
      {error && (
        <Card className="mb-6 border-destructive">
          <CardContent className="p-4">
            <p className="text-destructive">{error}</p>
            <Button
              variant="outline"
              size="sm"
              onClick={() => fetchChannels(debouncedSearch, selectedGroup, 1, false, false)}
              className="mt-2"
            >
              Retry
            </Button>
          </CardContent>
        </Card>
      )}

      {/* Results Summary */}
      {channels.length > 0 && (
        <div className="mb-4 text-sm text-muted-foreground">
          Showing {channels.length} of {total} channels
          {hasMore && !loading && (
            <span className="ml-2 text-primary">• {Math.ceil((total - channels.length) / 200)} more pages available</span>
          )}
        </div>
      )}

      {/* Channels Display */}
      {channels.length > 0 ? (
        <>
          {viewMode === 'table' ? (
            <Card className="mb-6">
              <CardContent className="p-0">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead className="w-16">Logo</TableHead>
                      <TableHead>Channel Name</TableHead>
                      <TableHead>Channel #</TableHead>
                      <TableHead>Group</TableHead>
                      <TableHead>Source</TableHead>
                      <TableHead>Video Codec</TableHead>
                      <TableHead>Audio Codec</TableHead>
                      <TableHead>Resolution</TableHead>
                      <TableHead>Last Probed</TableHead>
                      <TableHead className="w-32">Actions</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {channels.map((channel) => (
                      <ChannelTableRow key={channel.id} channel={channel} />
                    ))}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>
          ) : viewMode === 'grid' ? (
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4 mb-6">
              {channels.map((channel) => (
                <ChannelCard key={channel.id} channel={channel} />
              ))}
            </div>
          ) : (
            <div className="space-y-3 mb-6">
              {channels.map((channel) => (
                <ChannelListItem key={channel.id} channel={channel} />
              ))}
            </div>
          )}

          {/* Progressive Loading */}
          {hasMore && (
            <div ref={loadMoreRef} className="flex justify-center mt-6">
              <Card className="w-full max-w-md">
                <CardContent className="p-4 text-center">
                  {loading ? (
                    <div className="flex items-center justify-center space-x-2">
                      <div className="animate-spin rounded-full h-4 w-4 border-2 border-primary border-t-transparent"></div>
                      <p className="text-sm text-muted-foreground">Loading more channels...</p>
                    </div>
                  ) : (
                    <>
                      <p className="text-sm text-muted-foreground mb-2">
                        {Math.ceil((total - channels.length) / 200)} pages remaining
                      </p>
                      <Button 
                        variant="outline" 
                        onClick={handleLoadMore}
                        size="sm"
                        className="gap-2"
                      >
                        Load More Channels
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
            <p className="text-muted-foreground">No channels found</p>
            {(search || selectedGroup || selectedSources.length > 0) && (
              <Button
                variant="outline"
                onClick={() => {
                  setSearch('');
                  setSelectedGroup('');
                  setSelectedSources([]);
                }}
                className="mt-4"
              >
                Clear Filters
              </Button>
            )}
          </CardContent>
        </Card>
      )}

      {/* Video Player Modal */}
      {selectedChannel && (
        <VideoPlayerModal
          isOpen={isPlayerOpen}
          onClose={() => {
            setIsPlayerOpen(false);
            setSelectedChannel(null);
          }}
          channel={selectedChannel}
        />
      )}
      </div>
    </TooltipProvider>
  );
}