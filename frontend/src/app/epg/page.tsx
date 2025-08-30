"use client";

import React, {
  useState,
  useEffect,
  useCallback,
  useRef,
  useMemo,
} from "react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import {
  Search,
  Calendar,
  Clock,
  Filter,
  Play,
  Grid,
  List,
  Table as TableIcon,
  Star,
  ChevronLeft,
  ChevronRight,
} from "lucide-react";
import { DateTimePicker } from "@/components/ui/date-time-picker";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { VideoPlayerModal } from "@/components/video-player-modal";

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
  is_streamable: boolean;
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
  type: "epg_source" | "stream_source";
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
  const [search, setSearch] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [selectedSource, setSelectedSource] = useState<string>("");
  const [selectedCategory, setSelectedCategory] = useState<string>("");
  const [viewMode, setViewMode] = useState<"grid" | "list" | "table" | "guide">("guide");
  const [currentPage, setCurrentPage] = useState(1);
  const [total, setTotal] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [categories, setCategories] = useState<string[]>([]);
  const [timeRange, setTimeRange] = useState<
    "today" | "tomorrow" | "week" | "custom"
  >("today");
  const [customDate, setCustomDate] = useState<Date | undefined>(undefined);
  const [selectedProgram, setSelectedProgram] = useState<EpgProgram | null>(
    null,
  );
  const [isPlayerOpen, setIsPlayerOpen] = useState(false);
  const [hidePastPrograms, setHidePastPrograms] = useState(true);
  const [channelFilter, setChannelFilter] = useState("");
  const [currentTime, setCurrentTime] = useState(new Date());
  const [guideTimeRange, setGuideTimeRange] = useState<"3h" | "6h" | "12h">(
    "6h",
  );
  const [guideStartTime, setGuideStartTime] = useState<Date | undefined>(
    undefined,
  );
  const loadMoreRef = useRef<HTMLDivElement>(null);

  const getTimeRangeParams = useCallback(() => {
    const now = new Date();
    let startTime: Date;
    let endTime: Date;

    switch (timeRange) {
      case "today":
        // If hiding past programs, start from current time, otherwise start from beginning of day
        startTime = hidePastPrograms
          ? now
          : new Date(now.getFullYear(), now.getMonth(), now.getDate());
        endTime = new Date(
          now.getFullYear(),
          now.getMonth(),
          now.getDate() + 1,
        );
        break;
      case "tomorrow":
        startTime = new Date(
          now.getFullYear(),
          now.getMonth(),
          now.getDate() + 1,
        );
        endTime = new Date(startTime.getTime() + 24 * 60 * 60 * 1000);
        break;
      case "week":
        // If hiding past programs, start from current time, otherwise start from beginning of today
        startTime = hidePastPrograms
          ? now
          : new Date(now.getFullYear(), now.getMonth(), now.getDate());
        endTime = new Date(startTime.getTime() + 7 * 24 * 60 * 60 * 1000);
        break;
      case "custom":
        if (customDate) {
          // For custom date, check if it's today and apply hidePastPrograms logic
          const isToday = customDate.toDateString() === now.toDateString();
          startTime =
            isToday && hidePastPrograms
              ? now
              : new Date(
                  customDate.getFullYear(),
                  customDate.getMonth(),
                  customDate.getDate(),
                );
          endTime = new Date(
            customDate.getFullYear(),
            customDate.getMonth(),
            customDate.getDate() + 1,
          );
        } else {
          // Fallback to today if no custom date is set
          startTime = hidePastPrograms
            ? now
            : new Date(now.getFullYear(), now.getMonth(), now.getDate());
          endTime = new Date(
            now.getFullYear(),
            now.getMonth(),
            now.getDate() + 1,
          );
        }
        break;
    }

    return {
      start_time: startTime.toISOString(),
      end_time: endTime.toISOString(),
    };
  }, [timeRange, hidePastPrograms, customDate]);

  // Debounce search input to prevent excessive API calls and focus loss
  useEffect(() => {
    const timer = setTimeout(() => {
      setDebouncedSearch(search);
    }, 300); // 300ms debounce

    return () => clearTimeout(timer);
  }, [search]);

  // Update current time every minute for live indicators
  useEffect(() => {
    const timer = setInterval(() => {
      setCurrentTime(new Date());
    }, 60000); // Update every minute

    return () => clearInterval(timer);
  }, []);

  const fetchPrograms = useCallback(
    async (
      searchTerm: string = "",
      sourceId: string = "",
      category: string = "",
      pageNum: number = 1,
      append: boolean = false,
    ) => {
      try {
        setLoading(true);

        const params = new URLSearchParams({
          page: pageNum.toString(),
          limit: "50",
          ...getTimeRangeParams(),
        });

        if (searchTerm) params.append("search", searchTerm);
        if (sourceId) params.append("source_id", sourceId);
        if (category) params.append("category", category);

        const response = await fetch(`/api/v1/epg/programs?${params}`);

        if (!response.ok) {
          throw new Error(`Failed to fetch programs: ${response.statusText}`);
        }

        const data: { success: boolean; data: EpgProgramsResponse } =
          await response.json();

        if (!data.success) {
          throw new Error("API returned unsuccessful response");
        }

        if (append) {
          setPrograms((prev) => {
            // Deduplicate by ID
            const existing = new Set(prev.map((program) => program.id));
            const newPrograms = data.data.programs.filter(
              (program) => !existing.has(program.id),
            );
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
            new Set(data.data.programs.map((p) => p.category).filter(Boolean)),
          ) as string[];
          setCategories(uniqueCategories);
        }

        setError(null);
      } catch (err) {
        setError(err instanceof Error ? err.message : "An error occurred");
        if (!append) {
          setPrograms([]);
        }
      } finally {
        setLoading(false);
      }
    },
    [getTimeRangeParams],
  );

  const fetchSources = async () => {
    try {
      const options: SourceOption[] = [];

      // Fetch EPG Sources
      try {
        const epgSourcesResponse = await fetch("/api/v1/sources/epg");
        if (epgSourcesResponse.ok) {
          const epgSourcesData: { success: boolean; data: { items: any[] } } =
            await epgSourcesResponse.json();
          if (epgSourcesData.success && epgSourcesData.data.items) {
            // Only store active EPG sources (backend should already filter, but ensure consistency)
            const activeEpgSources = epgSourcesData.data.items.filter(
              (source) => source.is_active,
            );
            setSources(activeEpgSources);

            activeEpgSources.forEach((source) => {
              options.push({
                id: source.id,
                name: source.name,
                type: "epg_source",
                display_name: `${source.name} (${source.source_type.toUpperCase()})`,
              });
            });
          }
        }
      } catch (err) {
        console.warn("Failed to fetch EPG sources:", err);
      }


      // Deduplicate sources based on ID to ensure uniqueness
      const uniqueOptions = Array.from(
        new Map(options.map((opt) => [opt.id, opt])).values(),
      );
      setSourceOptions(uniqueOptions);
    } catch (err) {
      console.error("Failed to fetch sources:", err);
    }
  };

  const fetchGuideData = async () => {
    try {
      setLoading(true);

      // Use guide-specific time range
      const baseTime = guideStartTime || new Date();
      const hours = parseInt(guideTimeRange.replace("h", ""));
      const startTime = baseTime;
      const endTime = new Date(baseTime.getTime() + hours * 60 * 60 * 1000);

      const params = new URLSearchParams({
        start_time: startTime.toISOString(),
        end_time: endTime.toISOString(),
      });

      if (selectedSource) params.append("source_id", selectedSource);

      const response = await fetch(`/api/v1/epg/guide?${params}`);

      if (!response.ok) {
        throw new Error(`Failed to fetch guide data: ${response.statusText}`);
      }

      const data: { success: boolean; data: EpgGuideResponse } =
        await response.json();

      if (!data.success) {
        throw new Error("API returned unsuccessful response");
      }

      setGuideData(data.data);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "An error occurred");
      setGuideData(null);
    } finally {
      setLoading(false);
    }
  };

  const handleLoadMore = useCallback(() => {
    if (hasMore && !loading && viewMode !== "guide") {
      fetchPrograms(
        debouncedSearch,
        selectedSource,
        selectedCategory,
        currentPage + 1,
        true,
      );
    }
  }, [
    hasMore,
    loading,
    viewMode,
    debouncedSearch,
    selectedSource,
    selectedCategory,
    currentPage,
    fetchPrograms,
  ]);

  useEffect(() => {
    fetchSources();
  }, []);

  // Handle search changes without losing focus
  const performProgramSearch = useCallback(() => {
    if (viewMode !== "guide") {
      setPrograms([]);
      setCurrentPage(1);
      fetchPrograms(
        debouncedSearch,
        selectedSource,
        selectedCategory,
        1,
        false,
      );
    }
  }, [
    viewMode,
    debouncedSearch,
    selectedSource,
    selectedCategory,
    fetchPrograms,
  ]);

  useEffect(() => {
    performProgramSearch();
  }, [debouncedSearch]); // Only trigger on debounced search changes

  // Handle non-search filter changes
  useEffect(() => {
    if (viewMode === "guide") {
      fetchGuideData();
    } else {
      setPrograms([]);
      setCurrentPage(1);
      fetchPrograms(
        debouncedSearch,
        selectedSource,
        selectedCategory,
        1,
        false,
      );
    }
  }, [
    selectedSource,
    selectedCategory,
    timeRange,
    viewMode,
    hidePastPrograms,
    customDate,
    guideTimeRange,
    guideStartTime,
  ]); // Include hidePastPrograms and customDate

  // Intersection observer for infinite scroll - only for non-guide views
  useEffect(() => {
    const loadMoreElement = loadMoreRef.current;
    if (!loadMoreElement || viewMode === "guide") return;

    const observer = new IntersectionObserver(
      (entries) => {
        const [entry] = entries;
        // Trigger load more when the element comes into view and we have more data
        // Only trigger on intersection, not when search changes to prevent focus loss
        if (
          entry.isIntersecting &&
          hasMore &&
          !loading &&
          !debouncedSearch
        ) {
          handleLoadMore();
        }
      },
      {
        // Trigger when the element is 200px away from being visible
        rootMargin: "200px",
        threshold: 0.1,
      },
    );

    observer.observe(loadMoreElement);

    return () => {
      observer.unobserve(loadMoreElement);
    };
  }, [hasMore, loading, debouncedSearch, viewMode, handleLoadMore]);

  const handleSearch = (value: string) => {
    setSearch(value);
  };

  const handleSourceFilter = (value: string) => {
    setSelectedSource(value === "all" ? "" : value);
  };

  const handleCategoryFilter = (value: string) => {
    setSelectedCategory(value === "all" ? "" : value);
  };

  // Helper function to determine button state and tooltip
  const getPlayButtonState = (program: EpgProgram) => {
    const now = new Date();
    const startTime = new Date(program.start_time);
    const endTime = new Date(program.end_time);
    const isLive = now >= startTime && now <= endTime;
    const isUpcoming = startTime > now;
    const isPast = endTime < now;

    if (!program.is_streamable) {
      return {
        disabled: true,
        variant: "outline" as const,
        tooltip: "Channel not available for streaming",
      };
    }

    if (isPast) {
      return {
        disabled: true,
        variant: "outline" as const,
        tooltip: "Program has already ended",
      };
    }

    if (isUpcoming) {
      return {
        disabled: false,
        variant: "secondary" as const,
        tooltip: "Play upcoming program (will show current live stream)",
      };
    }

    if (isLive) {
      return {
        disabled: false,
        variant: "default" as const,
        tooltip: "Play live program",
      };
    }

    return {
      disabled: true,
      variant: "outline" as const,
      tooltip: "Program not available",
    };
  };

  const handlePlayProgram = (program: EpgProgram) => {
    // Use the new unified channel streaming endpoint (directly streams content)
    const streamUrl = `/channel/${encodeURIComponent(program.channel_id)}/stream`;

    // Update the program with the direct streaming URL for the video player
    const updatedProgram = {
      ...program,
      stream_url: streamUrl,
    };

    setSelectedProgram(updatedProgram);
    setIsPlayerOpen(true);
  };

  const formatTime = (timeString: string) => {
    // Parse the time string - JavaScript Date constructor handles ISO 8601/RFC3339 correctly
    const date = new Date(timeString);

    // Check if date is valid
    if (isNaN(date.getTime())) {
      return "--:--";
    }

    return date.toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false, // Use 24-hour format for clarity
    });
  };

  const formatDate = (timeString: string) => {
    return new Date(timeString).toLocaleDateString([], {
      weekday: "short",
      month: "short",
      day: "numeric",
    });
  };

  // Guide-specific helper functions
  const formatGuideTime = (timeString: string) => {
    return new Date(timeString).toLocaleTimeString([], {
      hour: "numeric",
      hour12: true,
    });
  };

  const isCurrentTimeSlot = (timeSlot: string) => {
    const slotTime = new Date(timeSlot);
    const now = currentTime;
    const slotEnd = new Date(slotTime.getTime() + 60 * 60 * 1000); // Add 1 hour
    return now >= slotTime && now < slotEnd;
  };


  const getFilteredChannels = () => {
    if (!guideData) return [];

    let channels = Object.entries(guideData.channels);

    // Filter by search term - search channels and their programs
    if (channelFilter) {
      const searchLower = channelFilter.toLowerCase();
      channels = channels.filter(([id, channel]) => {
        // Check if channel name or ID matches
        if (
          channel.name.toLowerCase().includes(searchLower) ||
          id.toLowerCase().includes(searchLower)
        ) {
          return true;
        }

        // Check if any program title matches
        const channelPrograms = guideData.programs[id] || [];
        return channelPrograms.some(
          (program) =>
            program.title.toLowerCase().includes(searchLower) ||
            (program.description &&
              program.description.toLowerCase().includes(searchLower)),
        );
      });
    }


    // Sort alphabetically by channel name
    channels.sort(([aId, aChannel], [bId, bChannel]) => {
      return aChannel.name.localeCompare(bChannel.name);
    });

    return channels;
  };

  const getProgramsForTimeSlot = (channelId: string, timeSlot: string) => {
    if (!guideData?.programs[channelId]) return [];

    const slotStart = new Date(timeSlot);
    const slotEnd = new Date(slotStart.getTime() + 60 * 60 * 1000); // 1 hour slot

    return guideData.programs[channelId].filter((program) => {
      const programStart = new Date(program.start_time);
      const programEnd = new Date(program.end_time);

      // Program overlaps with this time slot
      return programStart < slotEnd && programEnd > slotStart;
    });
  };

  // Filter programs based on hide past programs toggle and live only toggle
  const filteredPrograms = useMemo(() => {
    let filtered = programs;

    // Filter by past programs
    if (hidePastPrograms) {
      const now = new Date();
      filtered = filtered.filter((program) => {
        const endTime = new Date(program.end_time);
        // Keep programs that haven't ended yet (live and upcoming)
        return endTime > now;
      });
    }


    return filtered;
  }, [programs, hidePastPrograms]);

  const ProgramCard = ({ program }: { program: EpgProgram }) => {
    const now = new Date();
    const startTime = new Date(program.start_time);
    const endTime = new Date(program.end_time);
    const isLive = now >= startTime && now <= endTime;
    const isUpcoming = startTime > now;
    const buttonState = getPlayButtonState(program);

    return (
      <Card
        className={`transition-all duration-200 hover:shadow-lg ${isLive ? "ring-2 ring-primary" : ""}`}
      >
        <CardHeader className="pb-3">
          <div className="flex items-start justify-between">
            <div className="flex-1 min-w-0">
              <div className="flex items-center space-x-2 mb-1">
                <Badge
                  variant={
                    isLive ? "default" : isUpcoming ? "secondary" : "outline"
                  }
                  className="text-xs"
                >
                  {isLive ? "LIVE" : isUpcoming ? "UPCOMING" : "PAST"}
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
                  (e.target as HTMLImageElement).style.display = "none";
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
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  size="sm"
                  onClick={() => handlePlayProgram(program)}
                  disabled={buttonState.disabled}
                  variant={buttonState.variant}
                >
                  <Play className="w-4 h-4" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                <p>{buttonState.tooltip}</p>
              </TooltipContent>
            </Tooltip>
          </div>
        </CardContent>
      </Card>
    );
  };

  const ProgramTableRow = ({ program }: { program: EpgProgram }) => {
    const now = new Date();
    const startTime = new Date(program.start_time);
    const endTime = new Date(program.end_time);
    const isLive = now >= startTime && now <= endTime;
    const isUpcoming = startTime > now;
    const buttonState = getPlayButtonState(program);

    return (
      <TableRow className={`hover:bg-muted/50 ${isLive ? "bg-primary/5" : ""}`}>
        <TableCell className="w-16">
          {program.channel_logo ? (
            <img
              src={program.channel_logo}
              alt={program.channel_name}
              className="w-8 h-8 object-contain"
              onError={(e) => {
                (e.target as HTMLImageElement).style.display = "none";
              }}
            />
          ) : (
            <div className="w-8 h-8 bg-muted rounded flex items-center justify-center text-muted-foreground text-xs">
              No Logo
            </div>
          )}
        </TableCell>
        <TableCell className="font-medium max-w-xs">
          <div className="truncate" title={program.title}>
            {program.title}
          </div>
        </TableCell>
        <TableCell className="text-sm">{program.channel_name}</TableCell>
        <TableCell className="text-sm">
          <div className="flex items-center">
            <Clock className="w-3 h-3 mr-1" />
            {formatTime(program.start_time)} - {formatTime(program.end_time)}
          </div>
        </TableCell>
        <TableCell className="text-sm">
          {formatDate(program.start_time)}
        </TableCell>
        <TableCell>
          <Badge
            variant={isLive ? "default" : isUpcoming ? "secondary" : "outline"}
            className="text-xs"
          >
            {isLive ? "LIVE" : isUpcoming ? "UPCOMING" : "PAST"}
          </Badge>
        </TableCell>
        <TableCell>
          {program.category ? (
            <Badge variant="outline" className="text-xs">
              {program.category}
            </Badge>
          ) : (
            <span className="text-muted-foreground">-</span>
          )}
        </TableCell>
        <TableCell className="text-sm max-w-md">
          {program.description ? (
            <div className="truncate" title={program.description}>
              {program.description}
            </div>
          ) : (
            <span className="text-muted-foreground">-</span>
          )}
        </TableCell>
        <TableCell className="w-24">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="sm"
                onClick={() => handlePlayProgram(program)}
                disabled={buttonState.disabled}
                className="h-8 px-2"
                variant={buttonState.variant}
              >
                <Play className="w-3 h-3" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              <p>{buttonState.tooltip}</p>
            </TooltipContent>
          </Tooltip>
        </TableCell>
      </TableRow>
    );
  };

  const ProgramListItem = ({ program }: { program: EpgProgram }) => {
    const now = new Date();
    const startTime = new Date(program.start_time);
    const endTime = new Date(program.end_time);
    const isLive = now >= startTime && now <= endTime;
    const isUpcoming = startTime > now;
    const buttonState = getPlayButtonState(program);

    return (
      <Card
        className={`transition-all duration-200 hover:shadow-md ${isLive ? "ring-1 ring-primary" : ""}`}
      >
        <CardContent className="p-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center space-x-4">
              {program.channel_logo && (
                <img
                  src={program.channel_logo}
                  alt={program.channel_name}
                  className="w-10 h-10 object-contain"
                  onError={(e) => {
                    (e.target as HTMLImageElement).style.display = "none";
                  }}
                />
              )}
              <div>
                <h3 className="font-medium">{program.title}</h3>
                <div className="flex items-center space-x-2 text-sm text-muted-foreground">
                  <span>{program.channel_name}</span>
                  <span>•</span>
                  <div className="flex items-center">
                    <Clock className="w-3 h-3 mr-1" />
                    {formatTime(program.start_time)} -{" "}
                    {formatTime(program.end_time)}
                  </div>
                  <span>•</span>
                  <span>{formatDate(program.start_time)}</span>
                  <Badge
                    variant={
                      isLive ? "default" : isUpcoming ? "secondary" : "outline"
                    }
                    className="text-xs"
                  >
                    {isLive ? "LIVE" : isUpcoming ? "UPCOMING" : "PAST"}
                  </Badge>
                  {program.category && (
                    <>
                      <span>•</span>
                      <Badge variant="outline" className="text-xs">
                        {program.category}
                      </Badge>
                    </>
                  )}
                </div>
                {program.description && (
                  <p className="text-sm text-muted-foreground mt-1 line-clamp-2">
                    {program.description}
                  </p>
                )}
              </div>
            </div>
            <div className="flex gap-2">
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    size="sm"
                    onClick={() => handlePlayProgram(program)}
                    disabled={buttonState.disabled}
                    variant={buttonState.variant}
                  >
                    <Play className="w-4 h-4 mr-2" />
                    Play
                  </Button>
                </TooltipTrigger>
                <TooltipContent>
                  <p>{buttonState.tooltip}</p>
                </TooltipContent>
              </Tooltip>
            </div>
          </div>
        </CardContent>
      </Card>
    );
  };

  // TV Guide Components
  const GuideTimeSlotHeader = () => {
    if (!guideData?.time_slots) return null;

    return (
      <div className="flex bg-background border-b sticky top-0 z-10">
        <div className="w-48 flex-shrink-0 p-3 border-r bg-muted/50">
          <span className="text-sm font-medium">Channels</span>
        </div>
        <ScrollArea className="flex-1">
          <div className="flex">
            {guideData.time_slots.map((timeSlot) => (
              <div
                key={timeSlot}
                className={`min-w-[120px] p-3 border-r text-center ${
                  isCurrentTimeSlot(timeSlot)
                    ? "bg-primary text-primary-foreground"
                    : "bg-muted/30"
                }`}
              >
                <Badge
                  variant={
                    isCurrentTimeSlot(timeSlot) ? "secondary" : "outline"
                  }
                  className="text-xs"
                >
                  {formatGuideTime(timeSlot)}
                </Badge>
              </div>
            ))}
          </div>
        </ScrollArea>
      </div>
    );
  };

  const GuideChannelRow = ({
    channelId,
    channel,
  }: {
    channelId: string;
    channel: { id: string; name: string; logo?: string };
  }) => {
    if (!guideData?.time_slots) return null;

    return (
      <div className="flex border-b hover:bg-muted/50">
        {/* Channel Info */}
        <div className="w-48 flex-shrink-0 p-3 border-r bg-background">
          <div className="flex items-center space-x-2">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => {
                    // Create a dummy program to trigger channel playback
                    const channelProgram: EpgProgram = {
                      id: `channel-${channelId}`,
                      channel_id: channelId,
                      channel_name: channel.name || channelId,
                      title: `Live: ${channel.name || channelId}`,
                      start_time: new Date().toISOString(),
                      end_time: new Date(Date.now() + 60 * 60 * 1000).toISOString(),
                      is_streamable: true,
                    };
                    handlePlayProgram(channelProgram);
                  }}
                  className="p-1 h-auto"
                >
                  <Play className="w-4 h-4 text-primary" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                <p>Play channel live stream</p>
              </TooltipContent>
            </Tooltip>
            {channel.logo && (
              <img
                src={channel.logo}
                alt={channel.name}
                className="w-8 h-8 object-contain"
                onError={(e) => {
                  (e.target as HTMLImageElement).style.display = "none";
                }}
              />
            )}
            <div className="flex-1 min-w-0">
              <p className="text-sm font-medium truncate">
                {channel.name || channelId}
              </p>
              <p className="text-xs text-muted-foreground">{channelId}</p>
            </div>
          </div>
        </div>

        {/* Program Cells */}
        <div className="flex-1">
          <div className="flex">
            {guideData.time_slots.map((timeSlot) => {
              const programs = getProgramsForTimeSlot(channelId, timeSlot);
              const program = programs[0]; // Take first program in this slot

              return (
                <div
                  key={timeSlot}
                  className="min-w-[120px] border-r h-16 relative"
                >
                  {program ? (
                    <GuideProgramCell program={program} timeSlot={timeSlot} />
                  ) : (
                    <div className="h-full bg-muted/30 flex items-center justify-center">
                      <span className="text-xs text-muted-foreground">
                        No Program
                      </span>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      </div>
    );
  };

  const GuideProgramCell = ({
    program,
    timeSlot,
  }: {
    program: EpgProgram;
    timeSlot: string;
  }) => {
    const now = currentTime;
    const startTime = new Date(program.start_time);
    const endTime = new Date(program.end_time);
    const isLive = now >= startTime && now <= endTime;
    const buttonState = getPlayButtonState(program);

    // Calculate program span across time slots
    const slotStart = new Date(timeSlot);
    const slotDuration = 60 * 60 * 1000; // 1 hour in ms
    const programDuration = endTime.getTime() - startTime.getTime();
    const slotsSpanned = Math.ceil(programDuration / slotDuration);

    return (
      <div
        className={`h-full p-1 transition-colors cursor-pointer group ${
          isLive 
            ? "bg-accent text-accent-foreground border-l-2 border-primary" 
            : "bg-secondary hover:bg-muted/80"
        }`}
        style={{ minWidth: `${slotsSpanned * 120}px` }}
      >
        <div className="flex justify-between items-start h-full">
          <div className="flex-1 min-w-0">
            <h4
              className="text-xs font-medium mb-1 overflow-hidden"
              style={{
                display: "-webkit-box",
                WebkitLineClamp: 2,
                WebkitBoxOrient: "vertical",
              }}
            >
              {program.title}
            </h4>
            <p className="text-xs text-muted-foreground">
              {formatTime(program.start_time)} - {formatTime(program.end_time)}
            </p>
          </div>
          <div className="opacity-0 group-hover:opacity-100 transition-opacity">
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  size="sm"
                  variant={buttonState.variant}
                  disabled={buttonState.disabled}
                  onClick={() => handlePlayProgram(program)}
                  className="h-6 w-6 p-0"
                >
                  <Play className="w-3 h-3" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                <p>{buttonState.tooltip}</p>
              </TooltipContent>
            </Tooltip>
          </div>
        </div>
        {isLive && (
          <div className="absolute top-0 right-0 w-2 h-2 bg-red-500 rounded-full animate-pulse"></div>
        )}
      </div>
    );
  };

  const CurrentTimeIndicator = () => {
    if (!guideData?.time_slots) return null;

    const now = currentTime;
    const guideStart = new Date(guideData.start_time);
    const totalDuration =
      new Date(guideData.end_time).getTime() - guideStart.getTime();
    const currentOffset = now.getTime() - guideStart.getTime();
    const percentage = Math.max(
      0,
      Math.min(100, (currentOffset / totalDuration) * 100),
    );

    if (percentage <= 0 || percentage >= 100) return null;

    return (
      <div
        className="absolute top-0 bottom-0 w-0.5 bg-red-500 z-20 pointer-events-none"
        style={{ left: `calc(192px + ${percentage}%)` }}
      >
        <div className="absolute -top-2 -left-2 w-4 h-4 bg-red-500 rounded-full"></div>
        <Badge
          variant="destructive"
          className="absolute -top-8 -left-8 text-xs px-1 py-0.5"
        >
          LIVE
        </Badge>
      </div>
    );
  };

  if (loading && programs.length === 0 && !guideData) {
    return (
      <TooltipProvider>
        <div className="container mx-auto p-6">
          <div className="mb-6">
            <p className="text-muted-foreground">
              Browse electronic program guide and schedule information
            </p>
          </div>

          <div className="space-y-6">
            {/* Skeleton Unified Filters and Controls */}
            <Card>
              <CardContent className="p-4">
                <div className="flex flex-col lg:flex-row gap-4 items-center">
                  <div className="relative flex-1 min-w-0">
                    <Skeleton className="h-10 w-full" />
                  </div>
                  <Skeleton className="h-10 w-48" />
                  <Skeleton className="h-10 w-32" />
                  <Skeleton className="h-10 w-48" />
                  <div className="flex bg-muted rounded-lg p-1">
                    <Skeleton className="h-8 w-8 m-1" />
                    <Skeleton className="h-8 w-8 m-1" />
                    <Skeleton className="h-8 w-8 m-1" />
                    <Skeleton className="h-8 w-8 m-1" />
                  </div>
                </div>
              </CardContent>
            </Card>

            {/* Skeleton TV Guide (default view) */}
            <Card className="overflow-hidden">
              <div className="relative">
                {/* Skeleton TV Guide Header */}
                <div className="flex bg-background border-b sticky top-0 z-10">
                  <div className="w-48 flex-shrink-0 p-3 border-r bg-muted/50">
                    <Skeleton className="h-5 w-20" />
                  </div>
                  <ScrollArea className="flex-1">
                    <div className="flex">
                      {Array.from({ length: 6 }).map((_, i) => (
                        <div key={i} className="min-w-[120px] p-3 border-r text-center bg-muted/30">
                          <Skeleton className="h-6 w-16 mx-auto" />
                        </div>
                      ))}
                    </div>
                  </ScrollArea>
                </div>

                {/* Skeleton TV Guide Rows */}
                <ScrollArea className="h-[600px] w-full">
                  <div className="relative">
                    {Array.from({ length: 8 }).map((_, i) => (
                      <div key={i} className="flex border-b">
                        {/* Skeleton Channel Info */}
                        <div className="w-48 flex-shrink-0 p-3 border-r bg-background">
                          <div className="flex items-center space-x-2">
                            <Skeleton className="w-6 h-6 rounded" />
                            <Skeleton className="w-8 h-8 rounded" />
                            <div className="flex-1">
                              <Skeleton className="h-4 w-24 mb-1" />
                              <Skeleton className="h-3 w-16" />
                            </div>
                          </div>
                        </div>

                        {/* Skeleton Program Cells */}
                        <div className="flex-1">
                          <div className="flex">
                            {Array.from({ length: 6 }).map((_, j) => (
                              <div key={j} className="min-w-[120px] border-r h-16 p-1">
                                {Math.random() > 0.3 ? (
                                  <div className="h-full bg-secondary p-1 rounded">
                                    <Skeleton className="h-3 w-full mb-1" />
                                    <Skeleton className="h-3 w-16" />
                                  </div>
                                ) : (
                                  <div className="h-full bg-muted/30 flex items-center justify-center">
                                    <Skeleton className="h-3 w-16" />
                                  </div>
                                )}
                              </div>
                            ))}
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                </ScrollArea>
              </div>

              {/* Skeleton Guide Footer */}
              <div className="border-t bg-muted/50 p-3">
                <div className="flex justify-between items-center">
                  <Skeleton className="h-4 w-32" />
                  <Skeleton className="h-4 w-20" />
                </div>
              </div>
            </Card>
          </div>
        </div>
      </TooltipProvider>
    );
  }

  return (
    <TooltipProvider>
      <div className="container mx-auto p-6">
        <div className="mb-6">
          <p className="text-muted-foreground">
            Browse electronic program guide and schedule information
          </p>
        </div>

        <div className="space-y-6">

          {/* Unified Filters and Controls */}
          <Card>
            <CardContent className="p-4">
              <div className="flex flex-col lg:flex-row gap-4 items-center">
                {/* Search */}
                <div className="relative flex-1 min-w-0">
                  <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 text-muted-foreground w-4 h-4" />
                  <Input
                    placeholder={viewMode === "guide" ? "Search channels, programs..." : "Search programs, channels, descriptions..."}
                    value={viewMode === "guide" ? channelFilter : search}
                    onChange={(e) => viewMode === "guide" ? setChannelFilter(e.target.value) : handleSearch(e.target.value)}
                    className="pl-10"
                  />
                </div>

                {/* Date/Time Picker */}
                <DateTimePicker
                  value={viewMode === "guide" ? guideStartTime : customDate}
                  onChange={viewMode === "guide" ? setGuideStartTime : setCustomDate}
                  placeholder="Now"
                  className="w-48"
                />

                {/* Time Range for Guide or Date Range for Programs */}
                {viewMode === "guide" ? (
                  <Select
                    value={guideTimeRange}
                    onValueChange={(v) => setGuideTimeRange(v as "3h" | "6h" | "12h")}
                  >
                    <SelectTrigger className="w-32">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="3h">3 Hours</SelectItem>
                      <SelectItem value="6h">6 Hours</SelectItem>
                      <SelectItem value="12h">12 Hours</SelectItem>
                    </SelectContent>
                  </Select>
                ) : (
                  <Select
                    value={timeRange}
                    onValueChange={(v) => setTimeRange(v as "today" | "tomorrow" | "week" | "custom")}
                  >
                    <SelectTrigger className="w-40">
                      <Calendar className="w-4 h-4 mr-2" />
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="today">Today</SelectItem>
                      <SelectItem value="tomorrow">Tomorrow</SelectItem>
                      <SelectItem value="week">This Week</SelectItem>
                      <SelectItem value="custom">Custom</SelectItem>
                    </SelectContent>
                  </Select>
                )}

                {/* Source Filter */}
                <Select
                  value={selectedSource || "all"}
                  onValueChange={handleSourceFilter}
                >
                  <SelectTrigger className="w-48">
                    <Filter className="w-4 h-4 mr-2" />
                    <SelectValue placeholder="All Sources" />
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

                {/* Category Filter (only for non-guide views) */}
                {viewMode !== "guide" && (
                  <Select
                    value={selectedCategory || "all"}
                    onValueChange={handleCategoryFilter}
                  >
                    <SelectTrigger className="w-48">
                      <Filter className="w-4 h-4 mr-2" />
                      <SelectValue placeholder="All Categories" />
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
                )}


                {/* Layout Buttons */}
                <div className="flex bg-muted rounded-lg p-1">
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant={viewMode === "guide" ? "default" : "ghost"}
                        size="sm"
                        onClick={() => setViewMode("guide")}
                      >
                        <Grid className="w-4 h-4" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>TV Guide</p>
                    </TooltipContent>
                  </Tooltip>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant={viewMode === "table" ? "default" : "ghost"}
                        size="sm"
                        onClick={() => setViewMode("table")}
                      >
                        <TableIcon className="w-4 h-4" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>Table view</p>
                    </TooltipContent>
                  </Tooltip>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant={viewMode === "list" ? "default" : "ghost"}
                        size="sm"
                        onClick={() => setViewMode("list")}
                      >
                        <List className="w-4 h-4" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>List view</p>
                    </TooltipContent>
                  </Tooltip>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant={viewMode === "grid" ? "default" : "ghost"}
                        size="sm"
                        onClick={() => setViewMode("grid")}
                      >
                        <Grid className="w-4 h-4" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>
                      <p>Card view</p>
                    </TooltipContent>
                  </Tooltip>
                </div>
              </div>
            </CardContent>
          </Card>

          {/* Programs Views (grid, list, table) */}
          {viewMode !== "guide" && (
            <div className="space-y-6">
              {/* Hide Past Programs Toggle for non-guide views */}
              <div className="flex items-center space-x-2">
                <Switch
                  id="hide-past-programs"
                  checked={hidePastPrograms}
                  onCheckedChange={setHidePastPrograms}
                />
                <label
                  htmlFor="hide-past-programs"
                  className="text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70"
                >
                  Hide past programs (keep live and upcoming only)
                </label>
              </div>

            {/* Error Display */}
            {error && (
              <Card className="border-destructive">
                <CardContent className="p-4">
                  <p className="text-destructive">{error}</p>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() =>
                      fetchPrograms(
                        debouncedSearch,
                        selectedSource,
                        selectedCategory,
                        1,
                        false,
                      )
                    }
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
                Showing {filteredPrograms.length} of {programs.length} programs
                {hidePastPrograms &&
                  filteredPrograms.length !== programs.length && (
                    <span className="ml-2 text-primary">
                      • {programs.length - filteredPrograms.length} past
                      programs hidden
                    </span>
                  )}
                {hasMore && !loading && (
                  <span className="ml-2 text-primary">
                    • {Math.ceil((total - programs.length) / 50)} more pages
                    available
                  </span>
                )}
              </div>
            )}

            {/* Programs Display */}
            {loading ? (
              // Loading skeletons for different view modes
              <>
                {viewMode === "table" ? (
                  <Card className="mb-6">
                    <CardContent className="p-0">
                      <Table>
                        <TableHeader>
                          <TableRow>
                            <TableHead className="w-16">Logo</TableHead>
                            <TableHead>Program Title</TableHead>
                            <TableHead>Channel</TableHead>
                            <TableHead>Time</TableHead>
                            <TableHead>Date</TableHead>
                            <TableHead>Status</TableHead>
                            <TableHead>Category</TableHead>
                            <TableHead>Description</TableHead>
                            <TableHead className="w-24">Actions</TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          {Array.from({ length: 6 }).map((_, i) => (
                            <TableRow key={i}>
                              <TableCell><Skeleton className="w-8 h-8 rounded" /></TableCell>
                              <TableCell><Skeleton className="h-4 w-32" /></TableCell>
                              <TableCell><Skeleton className="h-4 w-24" /></TableCell>
                              <TableCell><Skeleton className="h-4 w-20" /></TableCell>
                              <TableCell><Skeleton className="h-4 w-16" /></TableCell>
                              <TableCell><Skeleton className="h-6 w-12 rounded-full" /></TableCell>
                              <TableCell><Skeleton className="h-6 w-16 rounded-full" /></TableCell>
                              <TableCell><Skeleton className="h-4 w-40" /></TableCell>
                              <TableCell><Skeleton className="h-8 w-8 rounded" /></TableCell>
                            </TableRow>
                          ))}
                        </TableBody>
                      </Table>
                    </CardContent>
                  </Card>
                ) : viewMode === "grid" ? (
                  <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4 mb-6">
                    {Array.from({ length: 12 }).map((_, i) => (
                      <Card key={i}>
                        <CardHeader className="pb-3">
                          <div className="flex items-start justify-between">
                            <div className="flex-1 min-w-0">
                              <div className="flex items-center space-x-2 mb-1">
                                <Skeleton className="h-5 w-12 rounded-full" />
                                <Skeleton className="h-5 w-16 rounded-full" />
                              </div>
                              <Skeleton className="h-4 w-full mb-1" />
                              <Skeleton className="h-3 w-24" />
                            </div>
                            <Skeleton className="w-8 h-8 ml-2 rounded" />
                          </div>
                        </CardHeader>
                        <CardContent className="pt-0">
                          <div className="flex items-center justify-between mb-3">
                            <Skeleton className="h-3 w-20" />
                            <Skeleton className="h-5 w-8 rounded-full" />
                          </div>
                          <Skeleton className="h-3 w-full mb-1" />
                          <Skeleton className="h-3 w-3/4 mb-3" />
                          <div className="flex justify-between items-center">
                            <Skeleton className="h-3 w-16" />
                            <Skeleton className="h-8 w-8 rounded" />
                          </div>
                        </CardContent>
                      </Card>
                    ))}
                  </div>
                ) : (
                  <div className="space-y-3 mb-6">
                    {Array.from({ length: 8 }).map((_, i) => (
                      <Card key={i}>
                        <CardContent className="p-4">
                          <div className="flex items-center justify-between">
                            <div className="flex items-center space-x-4">
                              <Skeleton className="w-10 h-10 rounded" />
                              <div>
                                <Skeleton className="h-4 w-48 mb-2" />
                                <div className="flex items-center space-x-2">
                                  <Skeleton className="h-3 w-20" />
                                  <Skeleton className="h-3 w-16" />
                                  <Skeleton className="h-5 w-12 rounded-full" />
                                  <Skeleton className="h-5 w-16 rounded-full" />
                                </div>
                              </div>
                            </div>
                            <Skeleton className="h-8 w-16 rounded" />
                          </div>
                        </CardContent>
                      </Card>
                    ))}
                  </div>
                )}
              </>
            ) : filteredPrograms.length > 0 ? (
              <>
                {viewMode === "table" ? (
                  <Card className="mb-6">
                    <CardContent className="p-0">
                      <Table>
                        <TableHeader>
                          <TableRow>
                            <TableHead className="w-16">Logo</TableHead>
                            <TableHead>Program Title</TableHead>
                            <TableHead>Channel</TableHead>
                            <TableHead>Time</TableHead>
                            <TableHead>Date</TableHead>
                            <TableHead>Status</TableHead>
                            <TableHead>Category</TableHead>
                            <TableHead>Description</TableHead>
                            <TableHead className="w-24">Actions</TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          {filteredPrograms.map((program) => (
                            <ProgramTableRow
                              key={program.id}
                              program={program}
                            />
                          ))}
                        </TableBody>
                      </Table>
                    </CardContent>
                  </Card>
                ) : viewMode === "grid" ? (
                  <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4 mb-6">
                    {filteredPrograms.map((program) => (
                      <ProgramCard key={program.id} program={program} />
                    ))}
                  </div>
                ) : (
                  <div className="space-y-3 mb-6">
                    {filteredPrograms.map((program) => (
                      <ProgramListItem key={program.id} program={program} />
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
                            <p className="text-sm text-muted-foreground">
                              Loading more programs...
                            </p>
                          </div>
                        ) : (
                          <>
                            <p className="text-sm text-muted-foreground mb-2">
                              {Math.ceil((total - programs.length) / 50)} pages
                              remaining
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
            ) : programs.length > 0 && filteredPrograms.length === 0 ? (
              <Card>
                <CardContent className="p-8 text-center">
                  <p className="text-muted-foreground">
                    All programs are hidden by current filters
                  </p>
                  <Button
                    variant="outline"
                    onClick={() => setHidePastPrograms(false)}
                    className="mt-4"
                  >
                    Show Past Programs
                  </Button>
                </CardContent>
              </Card>
            ) : (
              !loading && (
                <Card>
                  <CardContent className="p-8 text-center">
                    <p className="text-muted-foreground">No programs found</p>
                    {(search || selectedCategory) && (
                      <Button
                        variant="outline"
                        onClick={() => {
                          setSearch("");
                          setSelectedCategory("");
                        }}
                        className="mt-4"
                      >
                        Clear Filters
                      </Button>
                    )}
                  </CardContent>
                </Card>
              )
            )}
            </div>
          )}

          {/* TV Guide View */}
          {viewMode === "guide" && (
            <div className="space-y-6">

            {/* TV Guide Grid */}
            {guideData ? (
              <Card className="overflow-hidden">
                <div className="relative">
                  <GuideTimeSlotHeader />

                  <ScrollArea className="h-[600px] w-full">
                    <div className="relative">
                      {getFilteredChannels().map(([channelId, channel]) => (
                        <GuideChannelRow
                          key={channelId}
                          channelId={channelId}
                          channel={channel}
                        />
                      ))}
                    </div>
                  </ScrollArea>

                  <CurrentTimeIndicator />
                </div>

                {/* Guide Footer */}
                <div className="border-t bg-muted/50 p-3">
                  <div className="flex justify-between items-center text-sm text-muted-foreground">
                    <span>
                      Showing {getFilteredChannels().length} of{" "}
                      {Object.keys(guideData.channels).length} channels
                    </span>
                    <span>Updated: {new Date().toLocaleTimeString()}</span>
                  </div>
                </div>
              </Card>
            ) : loading ? (
              <Card className="overflow-hidden">
                <div className="relative">
                  {/* Skeleton TV Guide Header */}
                  <div className="flex bg-background border-b sticky top-0 z-10">
                    <div className="w-48 flex-shrink-0 p-3 border-r bg-muted/50">
                      <Skeleton className="h-5 w-20" />
                    </div>
                    <ScrollArea className="flex-1">
                      <div className="flex">
                        {Array.from({ length: 6 }).map((_, i) => (
                          <div key={i} className="min-w-[120px] p-3 border-r text-center bg-muted/30">
                            <Skeleton className="h-6 w-16 mx-auto" />
                          </div>
                        ))}
                      </div>
                    </ScrollArea>
                  </div>

                  {/* Skeleton TV Guide Rows */}
                  <ScrollArea className="h-[600px] w-full">
                    <div className="relative">
                      {Array.from({ length: 8 }).map((_, i) => (
                        <div key={i} className="flex border-b">
                          {/* Skeleton Channel Info */}
                          <div className="w-48 flex-shrink-0 p-3 border-r bg-background">
                            <div className="flex items-center space-x-2">
                              <Skeleton className="w-6 h-6 rounded" />
                              <Skeleton className="w-8 h-8 rounded" />
                              <div className="flex-1">
                                <Skeleton className="h-4 w-24 mb-1" />
                                <Skeleton className="h-3 w-16" />
                              </div>
                            </div>
                          </div>

                          {/* Skeleton Program Cells */}
                          <div className="flex-1">
                            <div className="flex">
                              {Array.from({ length: 6 }).map((_, j) => (
                                <div key={j} className="min-w-[120px] border-r h-16 p-1">
                                  {Math.random() > 0.3 ? (
                                    <div className="h-full bg-secondary p-1 rounded">
                                      <Skeleton className="h-3 w-full mb-1" />
                                      <Skeleton className="h-3 w-16" />
                                    </div>
                                  ) : (
                                    <div className="h-full bg-muted/30 flex items-center justify-center">
                                      <Skeleton className="h-3 w-16" />
                                    </div>
                                  )}
                                </div>
                              ))}
                            </div>
                          </div>
                        </div>
                      ))}
                    </div>
                  </ScrollArea>
                </div>

                {/* Skeleton Guide Footer */}
                <div className="border-t bg-muted/50 p-3">
                  <div className="flex justify-between items-center">
                    <Skeleton className="h-4 w-32" />
                    <Skeleton className="h-4 w-20" />
                  </div>
                </div>
              </Card>
            ) : (
              <Card>
                <CardContent className="p-8 text-center">
                  <Grid className="w-12 h-12 mx-auto mb-4 opacity-50" />
                  <p className="text-muted-foreground mb-4">
                    No guide data available
                  </p>
                  <Button onClick={fetchGuideData} variant="outline">
                    Retry Loading Guide
                  </Button>
                </CardContent>
              </Card>
            )}
            </div>
          )}
        </div>

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
    </TooltipProvider>
  );
}
