"use client";

import React, { useEffect, useRef, useState, useCallback, ErrorInfo, Component, useMemo } from 'react';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { X, Maximize2, Minimize2, Volume2, VolumeX, Settings, Copy, ExternalLink, Check } from 'lucide-react';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Debug } from '@/utils/debug';
import mpegts from 'mpegts.js';

// Make mpegts available globally for compatibility
declare global {
  interface Window {
    mpegts: typeof mpegts;
  }
}

interface Channel {
  id: string;
  name: string;
  logo_url?: string;
  group?: string;
  stream_url: string;
  source_type: string;
  source_name?: string;
}

interface EpgProgram {
  id: string;
  channel_id: string;
  channel_name: string;
  title: string;
  description?: string;
  start_time: string;
  end_time: string;
  category?: string;
  stream_url?: string; // Add stream_url for programs
}

interface VideoPlayerModalProps {
  isOpen: boolean;
  onClose: () => void;
  channel?: Channel;
  program?: EpgProgram;
}

// Error boundary to catch video player errors and prevent app crashes
class VideoPlayerErrorBoundary extends Component<
  { children: React.ReactNode; onError: (error: string) => void },
  { hasError: boolean }
> {
  constructor(props: { children: React.ReactNode; onError: (error: string) => void }) {
    super(props);
    this.state = { hasError: false };
  }

  static getDerivedStateFromError(_: Error) {
    return { hasError: true };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error('Video player error boundary caught error:', error, errorInfo);
    this.props.onError(`Video player error: ${error.message}`);
  }

  render() {
    if (this.state.hasError) {
      return null; // Let parent component handle error display
    }

    return this.props.children;
  }
}

export function VideoPlayerModal({ isOpen, onClose, channel, program }: VideoPlayerModalProps) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const playerRef = useRef<any>(null);
  const errorCountRef = useRef<number>(0);
  const lastErrorTimeRef = useRef<number>(0);
  
  // Create debug logger for this component - use useMemo to avoid recreating on every render
  const debug = useMemo(() => Debug.createLogger('VideoPlayer'), []);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [isMuted, setIsMuted] = useState(false);
  const [mpegtsLoaded, setMpegtsLoaded] = useState(false);
  const [copySuccess, setCopySuccess] = useState(false);
  const [hevcSupport, setHevcSupport] = useState<{
    platformHEVC: boolean;
    webPlatformFeatures: boolean;
    hardwareSupport: string;
  } | null>(null);
  const [showControls, setShowControls] = useState(true);
  const [isHevcStream, setIsHevcStream] = useState(false);
  const [streamCodecs, setStreamCodecs] = useState<{
    video?: string;
    audio?: string;
  }>({});
  const [bufferInfo, setBufferInfo] = useState<{
    bufferLength?: number;
    backBufferLength?: number;
  }>({});
  const controlsTimerRef = useRef<NodeJS.Timeout | null>(null);
  const lastMouseMoveRef = useRef<number>(0);
  const bufferUpdateIntervalRef = useRef<NodeJS.Timeout | null>(null);

  // Show controls immediately (for mouse enter/move)
  const showControlsWithTimer = useCallback(() => {
    setShowControls(true);
    
    // Clear existing timer
    if (controlsTimerRef.current) {
      clearTimeout(controlsTimerRef.current);
    }
    
    // Set new timer to hide controls after 2.5 seconds of inactivity
    controlsTimerRef.current = setTimeout(() => {
      setShowControls(false);
    }, 2500);
  }, []);

  // Hide controls immediately (for mouse leave)
  const hideControlsImmediately = useCallback(() => {
    // Clear existing timer
    if (controlsTimerRef.current) {
      clearTimeout(controlsTimerRef.current);
    }
    
    // Hide controls immediately when mouse leaves
    setShowControls(false);
  }, []);

  // Handle mouse movement with throttling
  const handleMouseMove = useCallback(() => {
    const now = Date.now();
    
    // Throttle mouse move events to avoid excessive updates
    if (now - lastMouseMoveRef.current < 100) return;
    lastMouseMoveRef.current = now;
    
    showControlsWithTimer();
  }, [showControlsWithTimer]);

  // Handle mouse enter - show controls immediately
  const handleMouseEnter = useCallback(() => {
    showControlsWithTimer();
  }, [showControlsWithTimer]);

  // Handle mouse leave - hide controls immediately 
  const handleMouseLeave = useCallback(() => {
    hideControlsImmediately();
  }, [hideControlsImmediately]);

  // Auto-show controls on modal open
  useEffect(() => {
    if (!isOpen) return;
    
    showControlsWithTimer();
    
    // Cleanup timer on unmount
    return () => {
      if (controlsTimerRef.current) {
        clearTimeout(controlsTimerRef.current);
      }
    };
  }, [isOpen, showControlsWithTimer]);

  // Detect HEVC support and load mpegts.js
  useEffect(() => {
    if (typeof window === 'undefined') return;

    const detectHEVCSupport = async () => {
      const video = document.createElement('video');
      
      // Test HEVC codec support
      const hevcCodecs = [
        'video/mp4; codecs="hvc1.1.1.L93.B0"',
        'video/mp4; codecs="hev1.1.1.L93.B0"',
        'video/mp4; codecs="hvc1.1.6.L93.B0"'
      ];
      
      let hardwareSupport = 'none';
      for (const codec of hevcCodecs) {
        const support = video.canPlayType(codec);
        if (support === 'probably') {
          hardwareSupport = 'probably';
          break;
        } else if (support === 'maybe' && hardwareSupport === 'none') {
          hardwareSupport = 'maybe';
        }
      }

      // Detect browser flags (Chrome/Edge only)
      let platformHEVC = false;
      let webPlatformFeatures = false;
      
      if (navigator.userAgent.includes('Chrome') || navigator.userAgent.includes('Edge')) {
        try {
          // Try to detect if experimental features are enabled
          // This is indirect detection since we can't directly access flags
          const canvas = document.createElement('canvas');
          const ctx = canvas.getContext('2d');
          
          // Test for experimental web platform features availability
          if ('OffscreenCanvas' in window && 'MediaStreamTrackProcessor' in window) {
            webPlatformFeatures = true;
          }
          
          // Platform HEVC is harder to detect directly, but we can infer from codec support
          if (hardwareSupport !== 'none') {
            platformHEVC = true;
          }
        } catch (e) {
          // Flags detection failed, use defaults
        }
      }

      setHevcSupport({
        platformHEVC,
        webPlatformFeatures,
        hardwareSupport
      });
    };

    const initMpegtsJs = () => {
      try {
        // mpegts.js is now imported directly, so just make it available globally
        if (typeof window !== 'undefined' && mpegts) {
          window.mpegts = mpegts;
          setMpegtsLoaded(true);
        }
      } catch (err) {
        console.error('Failed to initialize mpegts.js:', err);
        setError('Failed to initialize video player');
      }
    };

    detectHEVCSupport();
    initMpegtsJs();
  }, []); // Remove debug from dependencies to avoid infinite loops

  // Initialize player when modal opens
  useEffect(() => {
    if (!isOpen || !mpegtsLoaded || !videoRef.current) return;

    const initializePlayer = async () => {
      try {
        setIsLoading(true);
        setError(null);

        const streamUrl = channel?.stream_url || program?.stream_url || '';
        if (!streamUrl) {
          setError('No stream URL available for this channel');
          setIsLoading(false);
          return;
        }

        // Destroy existing player if it exists
        if (playerRef.current) {
          try {
            // Pause video first to prevent any ongoing operations
            if (videoRef.current) {
              videoRef.current.pause();
              videoRef.current.src = '';
              videoRef.current.load(); // Force video element reset
            }
            
            // Properly destroy the player
            playerRef.current.unload();
            playerRef.current.detachMediaElement();
            playerRef.current.destroy();
          } catch (e) {
            debug.warn('Error destroying player:', e);
          }
          playerRef.current = null;
        }

        // Ensure video element is clean
        if (videoRef.current) {
          videoRef.current.className = 'w-full h-full';
          videoRef.current.controls = true;
          videoRef.current.preload = 'auto'; // Changed from 'metadata' for better buffering
          videoRef.current.playsInline = true;
          // Disable picture-in-picture to avoid duplicate controls
          (videoRef.current as any).disablePictureInPicture = true;
          
          // Enhanced buffering attributes
          videoRef.current.setAttribute('buffered', 'true');
          videoRef.current.setAttribute('crossorigin', 'anonymous');
          
          // Set buffer size hints if supported
          if ('setBufferSize' in videoRef.current) {
            (videoRef.current as any).setBufferSize(1024 * 1024 * 5); // 5MB buffer
          }
        }

        // Check if mpegts.js is supported
        if (!window.mpegts.getFeatureList().mseLivePlayback) {
          setError('Your browser does not support MSE live playback required for streaming');
          setIsLoading(false);
          return;
        }

        // Create mpegts.js player
        const mediaDataSource = {
          type: 'mpegts',
          url: streamUrl,
          isLive: true,
          hasAudio: true,
          hasVideo: true,
        };

        const config = {
          enableWorker: false,
          enableStashBuffer: true,
          stashInitialSize: 128, // Increased for better buffering
          stashBufferThreshold: 60, // Increased buffer size for smoother playback
          
          // Live streaming optimizations
          liveBufferLatencyChasing: true,
          liveBufferLatencyMaxLatency: 5, // Increased to allow more buffering
          liveBufferLatencyMinRemain: 1.5, // Increased minimum buffer
          
          // Buffer management
          autoCleanupSourceBuffer: true,
          autoCleanupMaxBackwardDuration: 30,
          autoCleanupMinBackwardDuration: 10,
          
          // Additional buffering settings
          enableSeekableRanges: false,
          lazyLoad: true,
          lazyLoadMaxDuration: 180, // 3 minutes of content
          lazyLoadRecoverDuration: 30,
          
          // Network and buffering tweaks
          reuseRedirectedURL: true,
          
          // Advanced buffering configuration
          initialLiveManifestSize: 3, // Start with more segments
          liveBufferLatencyChaseUpToTolerance: true,
          
          // Performance optimizations
          enableStatisticsInfo: true, // Enable buffer statistics
          fixAudioTimestampGap: true,
          
          // Segment loading optimizations  
          headers: {
            'Cache-Control': 'no-cache',
            'Pragma': 'no-cache'
          }
        };

        const player = window.mpegts.createPlayer(mediaDataSource, config);
        player.attachMediaElement(videoRef.current!);

        // Set up event listeners
        player.on(window.mpegts.Events.LOADING_COMPLETE, () => {
          debug.log('Player loading complete');
          setIsLoading(false);
        });

        // Only listen for specific events we care about

        player.on(window.mpegts.Events.MEDIA_INFO, (mediaInfo: any) => {
          debug.log('CODEC - Full media info:', JSON.stringify(mediaInfo, null, 2));
          
          // Extract codec information - try all possible property paths
          const codecs: { video?: string; audio?: string } = {};
          
          // Comprehensive search for video codec information
          let videoCodec = mediaInfo?.videoTracks?.[0]?.codec || 
                          mediaInfo?.videoTracks?.[0]?.codecName ||
                          mediaInfo?.video?.codec ||
                          mediaInfo?.video?.codecName ||
                          mediaInfo?.tracks?.[0]?.codec ||
                          mediaInfo?.tracks?.[0]?.codecName ||
                          mediaInfo?.videoConfig?.codec;
          
          // Try to extract video codec from mimeType if direct codec info isn't available
          if (!videoCodec && mediaInfo?.mimeType) {
            const mimeType = mediaInfo.mimeType.toLowerCase();
            if (mimeType.includes('avc1') || mimeType.includes('h264')) {
              videoCodec = 'avc1';
            } else if (mimeType.includes('hvc1') || mimeType.includes('hev1') || mimeType.includes('h265')) {
              videoCodec = 'hvc1';
            }
          }
          
          debug.log('CODEC - Raw video codec found:', videoCodec);
          
          if (videoCodec) {
            // Clean up codec name for display, ignore container formats like mp2t
            let cleanVideoCodec = videoCodec.toString().toLowerCase();
            if (cleanVideoCodec === 'mp2t') {
              // mp2t is a container format, not a codec - skip it
              debug.log('CODEC - Skipping container format mp2t');
            } else if (cleanVideoCodec.includes('h264') || cleanVideoCodec.includes('avc1') || cleanVideoCodec.includes('avc')) {
              cleanVideoCodec = 'H.264';
              codecs.video = cleanVideoCodec;
            } else if (cleanVideoCodec.includes('h265') || cleanVideoCodec.includes('hevc') || 
                      cleanVideoCodec.includes('hvc1') || cleanVideoCodec.includes('hev1')) {
              cleanVideoCodec = 'H.265';
              setIsHevcStream(true);
              codecs.video = cleanVideoCodec;
            } else if (cleanVideoCodec.includes('av01') || cleanVideoCodec.includes('av1')) {
              cleanVideoCodec = 'AV1';
              codecs.video = cleanVideoCodec;
            } else if (cleanVideoCodec.includes('vp9')) {
              cleanVideoCodec = 'VP9';
              codecs.video = cleanVideoCodec;
            } else if (cleanVideoCodec.includes('vp8')) {
              cleanVideoCodec = 'VP8';
              codecs.video = cleanVideoCodec;
            } else if (!['mp2t', 'mpegts', 'ts'].includes(cleanVideoCodec)) {
              // Only show if it's not a known container format
              cleanVideoCodec = videoCodec.toString().replace(/[^a-zA-Z0-9.]/g, '').toUpperCase();
              codecs.video = cleanVideoCodec;
            }
            
            debug.log('CODEC - Cleaned video codec:', codecs.video);
          }
          
          // Enhanced search for audio codec - look in all possible locations
          let audioCodec = mediaInfo?.audioTracks?.[0]?.codec ||
                          mediaInfo?.audioTracks?.[0]?.codecName ||
                          mediaInfo?.audio?.codec ||
                          mediaInfo?.audio?.codecName ||
                          mediaInfo?.tracks?.find((track: any) => track.type === 'audio')?.codec ||
                          mediaInfo?.tracks?.find((track: any) => track.type === 'audio')?.codecName ||
                          mediaInfo?.audioConfig?.codec;
          
          // If we still don't have audio codec, check if there are multiple tracks
          if (!audioCodec && mediaInfo?.tracks && mediaInfo.tracks.length > 1) {
            // Try second track if first is video
            audioCodec = mediaInfo.tracks[1]?.codec || mediaInfo.tracks[1]?.codecName;
          }
          
          // Additional fallback: check if we can detect from stream format/container
          if (!audioCodec && mediaInfo?.mimeType) {
            const mimeType = mediaInfo.mimeType.toLowerCase();
            if (mimeType.includes('aac') || mimeType.includes('mp4a')) {
              audioCodec = 'aac';
            } else if (mimeType.includes('mp3')) {
              audioCodec = 'mp3';
            }
          }
          
          // If we still can't detect it, just use unknown indicator
          if (!audioCodec) {
            debug.log('CODEC - No audio codec detected');
            audioCodec = '?';
          }
          
          debug.log('CODEC - Raw audio codec found:', audioCodec);
          
          if (audioCodec) {
            // Clean up audio codec name for display
            let cleanAudioCodec = audioCodec.toString().toLowerCase();
            if (cleanAudioCodec.includes('aac')) {
              cleanAudioCodec = 'AAC';
            } else if (cleanAudioCodec.includes('mp3')) {
              cleanAudioCodec = 'MP3';
            } else if (cleanAudioCodec.includes('ac3') && !cleanAudioCodec.includes('eac3')) {
              cleanAudioCodec = 'AC3';
            } else if (cleanAudioCodec.includes('eac3')) {
              cleanAudioCodec = 'E-AC3';
            } else if (cleanAudioCodec.includes('opus')) {
              cleanAudioCodec = 'Opus';
            } else if (cleanAudioCodec.includes('vorbis')) {
              cleanAudioCodec = 'Vorbis';
            } else if (cleanAudioCodec.includes('pcm')) {
              cleanAudioCodec = 'PCM';
            } else if (cleanAudioCodec.includes('mp2')) {
              cleanAudioCodec = 'MP2';
            } else if (!['mp2t', 'mpegts', 'ts'].includes(cleanAudioCodec)) {
              // Only show if it's not a known container format
              cleanAudioCodec = audioCodec.toString().replace(/[^a-zA-Z0-9.]/g, '').toUpperCase();
            } else {
              // Skip container formats
              cleanAudioCodec = null;
            }
            
            if (cleanAudioCodec) {
              debug.log('CODEC - Cleaned audio codec:', cleanAudioCodec);
              codecs.audio = cleanAudioCodec;
            }
          }
          
          debug.log('CODEC - Setting final codecs:', codecs);
          setStreamCodecs(codecs);
        });

        player.on(window.mpegts.Events.ERROR, (type: string, details: string, data: any) => {
          debug.error('mpegts.js error:', type, details, data);
          
          // Error rate limiting to prevent browser crashes from error loops
          const now = Date.now();
          if (now - lastErrorTimeRef.current < 5000) { // Less than 5 seconds since last error
            errorCountRef.current++;
          } else {
            errorCountRef.current = 1; // Reset count if errors are spaced out
          }
          lastErrorTimeRef.current = now;
          
          // If too many errors in a short time, give up to prevent crashes
          if (errorCountRef.current > 3) {
            debug.error('Too many errors detected, preventing further attempts to avoid browser crash');
            setError('Multiple playback errors detected. This stream may be incompatible with your browser. Please try a different channel or refresh the page.');
            setIsLoading(false);
            return;
          }
          
          let errorMsg = 'Failed to load video stream';

          // Enhanced error handling with recovery mechanisms
          if (type === 'NetworkError') {
            errorMsg = 'Network error while loading video. Please check your connection and try again.';
          } else if (type === 'MediaError') {
            if (details && (details.includes('unsupported') || details.includes('hvc1') || details.includes('hev1') || details.includes('HEVC'))) {
              setIsHevcStream(true); // Flag this as an HEVC stream
              errorMsg = 'codec_unsupported'; // Special flag for codec issues
            } else if (details && details.includes('decode')) {
              errorMsg = 'H.264 decode error: Your browser may not support this stream format. Try refreshing or use a different browser.';
            } else if (details && details.includes('buffer')) {
              errorMsg = 'Buffer error: Stream may be too fast for your device. This is a known issue with some H.264 streams.';
            } else {
              errorMsg = 'Media error: The stream format may not be supported or corrupted.';
            }
          } else if (type === 'OtherError') {
            if (details && details.includes('SourceBuffer')) {
              errorMsg = 'Buffer management error: Please refresh the page and try again.';
            } else {
              errorMsg = 'Video playback error: ' + (details || 'Unknown error occurred');
            }
          }

          // Only attempt recovery if we haven't had too many errors
          if (type === 'MediaError' && playerRef.current && errorCountRef.current <= 2) {
            try {
              debug.warn('Attempting player recovery after MediaError');
              // Try to recover by reloading after a delay
              setTimeout(() => {
                if (playerRef.current && videoRef.current) {
                  try {
                    playerRef.current.load();
                  } catch (recoveryError) {
                    debug.error('Recovery attempt failed:', recoveryError);
                  }
                }
              }, 2000); // Increased delay to prevent rapid retries
            } catch (e) {
              debug.error('Error during recovery attempt:', e);
            }
          }

          setError(errorMsg);
          setIsLoading(false);
        });

        // Set up video element event listeners
        if (videoRef.current) {
          videoRef.current.addEventListener('loadstart', () => {
            setIsLoading(true);
            setError(null);
          });

          videoRef.current.addEventListener('canplay', () => {
            setIsLoading(false);
            // Also try auto-play when the video element is ready to play
            if (videoRef.current) {
              videoRef.current.play().catch((e) => {
                debug.warn('Auto-play failed on canplay event (browser may require user interaction):', e);
              });
            }
          });

          videoRef.current.addEventListener('volumechange', () => {
            if (videoRef.current) {
              setIsMuted(videoRef.current.muted);
            }
          });

          videoRef.current.addEventListener('fullscreenchange', () => {
            setIsFullscreen(!!document.fullscreenElement);
          });

          videoRef.current.addEventListener('error', (e) => {
            debug.error('Video element error:', e);
            setError('Video playback failed');
            setIsLoading(false);
          });
        }

        // Load the stream
        player.load();
        playerRef.current = player;
        
        // Set up enhanced buffer monitoring with adaptive management
        bufferUpdateIntervalRef.current = setInterval(() => {
          if (playerRef.current && videoRef.current) {
            try {
              // Get comprehensive buffer information from mpegts.js
              const statistics = playerRef.current.statisticsInfo;
              if (statistics) {
                setBufferInfo({
                  bufferLength: statistics.bufferLength,
                  backBufferLength: statistics.backBufferLength,
                });
                
                // Adaptive buffer management based on buffer health
                const bufferHealth = statistics.bufferLength || 0;
                
                // If buffer is running low, try to help by reducing playback rate slightly
                if (bufferHealth < 2 && !videoRef.current.paused) {
                  debug.log('Low buffer detected, applying adaptive measures');
                  // Slightly slow down playback to allow buffering
                  if (videoRef.current.playbackRate > 0.98) {
                    videoRef.current.playbackRate = 0.98;
                  }
                } else if (bufferHealth > 5 && videoRef.current.playbackRate < 1.0) {
                  // Buffer is healthy, restore normal playback rate
                  debug.log('Buffer recovered, restoring normal playback rate');
                  videoRef.current.playbackRate = 1.0;
                }
              } else {
                // Enhanced fallback using video element buffered ranges
                const buffered = videoRef.current.buffered;
                if (buffered.length > 0) {
                  const currentTime = videoRef.current.currentTime;
                  let totalBuffered = 0;
                  let bufferAhead = 0;
                  
                  // Calculate total buffered content and content ahead of current time
                  for (let i = 0; i < buffered.length; i++) {
                    const start = buffered.start(i);
                    const end = buffered.end(i);
                    totalBuffered += (end - start);
                    
                    if (end > currentTime) {
                      bufferAhead += Math.max(0, end - Math.max(start, currentTime));
                    }
                  }
                  
                  setBufferInfo({
                    bufferLength: bufferAhead,
                    backBufferLength: totalBuffered - bufferAhead,
                  });
                  
                  // Apply adaptive buffering for HTML5 fallback too
                  if (bufferAhead < 2 && !videoRef.current.paused) {
                    if (videoRef.current.playbackRate > 0.98) {
                      videoRef.current.playbackRate = 0.98;
                    }
                  } else if (bufferAhead > 5 && videoRef.current.playbackRate < 1.0) {
                    videoRef.current.playbackRate = 1.0;
                  }
                }
              }
            } catch (e) {
              // Silently ignore buffer info errors
              debug.warn('Buffer monitoring error:', e);
            }
          }
        }, 500); // Update more frequently (every 500ms) for better responsiveness

      } catch (err) {
        debug.error('Player initialization error:', err);
        setError(err instanceof Error ? err.message : 'Failed to initialize video player');
        setIsLoading(false);
      }
    };

    initializePlayer();

    // Cleanup on unmount or modal close
    return () => {
      // Clean up timers
      if (controlsTimerRef.current) {
        clearTimeout(controlsTimerRef.current);
        controlsTimerRef.current = null;
      }
      
      if (bufferUpdateIntervalRef.current) {
        clearInterval(bufferUpdateIntervalRef.current);
        bufferUpdateIntervalRef.current = null;
      }
      
      // Clean up player
      if (playerRef.current) {
        try {
          // Comprehensive cleanup to prevent memory leaks and crashes
          if (videoRef.current) {
            videoRef.current.pause();
            videoRef.current.src = '';
            videoRef.current.load();
          }
          
          // Properly cleanup mpegts.js player
          playerRef.current.unload();
          playerRef.current.detachMediaElement();
          playerRef.current.destroy();
        } catch (e) {
          debug.warn('Error destroying player on cleanup:', e);
        }
        playerRef.current = null;
      }
      
      // Clean up video element
      if (videoRef.current) {
        videoRef.current.src = '';
        videoRef.current.load();
      }
    };
  }, [isOpen, mpegtsLoaded, channel?.stream_url]);


  const handleClose = () => {
    if (videoRef.current) {
      videoRef.current.pause();
    }
    onClose();
  };

  const toggleMute = () => {
    if (videoRef.current) {
      videoRef.current.muted = !videoRef.current.muted;
      setIsMuted(videoRef.current.muted);
    }
  };

  const toggleFullscreen = () => {
    if (videoRef.current) {
      if (document.fullscreenElement) {
        document.exitFullscreen();
      } else {
        videoRef.current.requestFullscreen();
      }
    }
  };

  const copyStreamUrl = async () => {
    const streamUrl = channel?.stream_url || program?.stream_url;
    if (streamUrl) {
      try {
        await navigator.clipboard.writeText(streamUrl);
        setCopySuccess(true);
        setTimeout(() => setCopySuccess(false), 2000); // Reset after 2 seconds
      } catch (err) {
        debug.error('Failed to copy stream URL:', err);
        // Fallback for older browsers
        try {
          const textArea = document.createElement('textarea');
          textArea.value = streamUrl;
          document.body.appendChild(textArea);
          textArea.select();
          document.execCommand('copy');
          document.body.removeChild(textArea);
          setCopySuccess(true);
          setTimeout(() => setCopySuccess(false), 2000);
        } catch (fallbackErr) {
          debug.error('Fallback copy also failed:', fallbackErr);
        }
      }
    }
  };

  const openInExternalPlayer = async () => {
    const streamUrl = channel?.stream_url || program?.stream_url;
    if (streamUrl) {
      try {
        // Convert relative URLs to absolute URLs
        let absoluteUrl = streamUrl;
        if (streamUrl.startsWith('/')) {
          absoluteUrl = `${window.location.origin}${streamUrl}`;
        }
        
        // Copy URL to clipboard
        await navigator.clipboard.writeText(absoluteUrl);
        
        // Create a proper .m3u8 playlist file that the system can handle
        const playlistContent = `#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:10
#EXT-X-MEDIA-SEQUENCE:0
#EXT-X-PLAYLIST-TYPE:LIVE
#EXTINF:10.0,
${absoluteUrl}`;
        
        const blob = new Blob([playlistContent], { type: 'application/vnd.apple.mpegurl' });
        const url = URL.createObjectURL(blob);
        
        // Create a link to download the playlist file
        const a = document.createElement('a');
        a.href = url;
        a.download = `${(channel?.name || program?.title || 'stream').replace(/[^a-zA-Z0-9]/g, '_')}.m3u8`;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        
        // Clean up
        setTimeout(() => URL.revokeObjectURL(url), 1000);
        
        // Show notification
        setCopySuccess(true);
        setTimeout(() => setCopySuccess(false), 3000);
      } catch (err) {
        debug.error('Failed to create playlist file:', err);
        // Fallback: just copy the URL
        await copyStreamUrl();
      }
    }
  };

  const displayTitle = program?.title || channel?.name || 'Video Player';
  const displaySubtitle = program 
    ? `${program.channel_name} • ${new Date(program.start_time).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`
    : channel?.group 
    ? channel.group
    : undefined;

  // Debug log the render with codecs
  if (streamCodecs.video || streamCodecs.audio) {
    debug.log('CODEC - Render with codecs:', streamCodecs);
  }

  return (
    <Dialog open={isOpen} onOpenChange={handleClose}>
      <DialogContent 
        className="max-w-5xl w-[90vw] p-0 bg-black [&>button]:hidden" 
        aria-describedby="video-player-description"
        onMouseMove={handleMouseMove}
        onMouseEnter={handleMouseEnter}
        onMouseLeave={hideControlsImmediately}
      >
        <div className="relative">
          {/* Header */}
          <DialogHeader className={`absolute top-0 left-0 right-0 z-10 bg-gradient-to-b from-black/80 to-transparent p-4 transition-opacity ${showControls ? 'duration-200 opacity-100' : 'duration-500 opacity-0'}`}>
            <div className="flex items-start justify-between text-white">
              <div className="flex-1 min-w-0">
                <DialogTitle className="text-lg font-medium truncate">
                  {displayTitle}
                </DialogTitle>
                {displaySubtitle && (
                  <DialogDescription className="text-gray-300 text-sm mt-1">
                    {displaySubtitle}
                  </DialogDescription>
                )}
                <div id="video-player-description" className="sr-only">
                  Video player for {displayTitle}. Use controls to play, pause, and adjust volume.
                </div>
                <div className="flex items-center space-x-2 mt-2">
                  {program?.category && (
                    <Badge variant="secondary" className="text-xs">
                      {program.category}
                    </Badge>
                  )}
                  {/* Source name badge */}
                  {(channel?.source_name || channel?.source_type) && (
                    <Badge 
                      variant="outline" 
                      className={`text-xs bg-black/50 backdrop-blur-sm ${
                        channel.source_type === 'proxy' ? 'border-blue-500 text-blue-300' :
                        channel.source_type === 'source' ? 'border-green-500 text-green-300' :
                        'border-gray-500 text-gray-300'
                      }`}
                    >
                      {channel.source_name || channel.source_type}
                    </Badge>
                  )}
                  {/* Video codec badge */}
                  {streamCodecs.video && (
                    <Badge variant="outline" className="text-xs bg-black/50 backdrop-blur-sm border-purple-500 text-purple-300">
                      {streamCodecs.video}
                    </Badge>
                  )}
                  {/* Audio codec badge */}
                  {streamCodecs.audio && (
                    <Badge variant="outline" className="text-xs bg-black/50 backdrop-blur-sm border-orange-500 text-orange-300">
                      {streamCodecs.audio}
                    </Badge>
                  )}
                  {/* Enhanced Buffer information */}
                  {bufferInfo.bufferLength !== undefined && (
                    <Badge 
                      variant="outline" 
                      className={`text-xs bg-black/50 backdrop-blur-sm ${
                        bufferInfo.bufferLength < 2 ? 'border-red-500 text-red-300' :
                        bufferInfo.bufferLength < 5 ? 'border-yellow-500 text-yellow-300' :
                        'border-green-500 text-green-300'
                      }`}
                    >
                      Buffer: {bufferInfo.bufferLength.toFixed(1)}s
                      {bufferInfo.backBufferLength !== undefined && (
                        <span className="ml-1 opacity-70">
                          (+{bufferInfo.backBufferLength.toFixed(1)}s)
                        </span>
                      )}
                    </Badge>
                  )}
                </div>
              </div>
              
              <div className="flex items-center space-x-2 ml-4">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={copyStreamUrl}
                  className={`text-white hover:bg-white/10 ${copySuccess ? 'bg-green-600/20' : ''}`}
                  title="Copy stream URL to clipboard"
                >
                  {copySuccess ? <Check className="w-4 h-4" /> : <Copy className="w-4 h-4" />}
                </Button>

                <Button
                  variant="ghost"
                  size="sm"
                  onClick={openInExternalPlayer}
                  className="text-white hover:bg-white/10"
                  title="Download playlist file for external player"
                >
                  <ExternalLink className="w-4 h-4" />
                </Button>
                
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleClose}
                  className="text-white hover:bg-white/10"
                >
                  <X className="w-4 h-4" />
                </Button>
              </div>
            </div>
          </DialogHeader>

          {/* Video Player */}
          <div 
            className="relative bg-black aspect-video flex items-center justify-center group"
          >
            {error ? (
              <div className="m-4 text-center text-white max-w-2xl mx-auto">
                <div className="bg-red-900/50 border border-red-600 rounded-lg p-6">
                  <h3 className="text-lg font-semibold mb-3">Video Playback Error</h3>
                  
                  {error === 'codec_unsupported' && isHevcStream ? (
                    <div className="text-left space-y-3 mb-4">
                      <p className="text-red-200">This video uses the H.265/HEVC codec which requires additional support.</p>
                      
                      {hevcSupport && (
                        <div className="bg-blue-900/30 border border-blue-600 rounded p-3 text-xs">
                          <p className="text-blue-200 font-medium mb-2">🔍 Browser Detection Results:</p>
                          <div className="space-y-1 text-blue-100">
                            <div className="flex justify-between">
                              <span>Hardware H.265 Support:</span>
                              <span className={hevcSupport.hardwareSupport === 'probably' ? 'text-green-300' : 
                                              hevcSupport.hardwareSupport === 'maybe' ? 'text-yellow-300' : 'text-red-300'}>
                                {hevcSupport.hardwareSupport}
                              </span>
                            </div>
                            <div className="flex justify-between">
                              <span>Web Platform Features:</span>
                              <span className={hevcSupport.webPlatformFeatures ? 'text-green-300' : 'text-red-300'}>
                                {hevcSupport.webPlatformFeatures ? 'enabled' : 'disabled'}
                              </span>
                            </div>
                            <div className="flex justify-between">
                              <span>Platform HEVC Decoder:</span>
                              <span className={hevcSupport.platformHEVC ? 'text-green-300' : 'text-red-300'}>
                                {hevcSupport.platformHEVC ? 'available' : 'not detected'}
                              </span>
                            </div>
                          </div>
                        </div>
                      )}
                      
                      {hevcSupport && (hevcSupport.hardwareSupport === 'none' || !hevcSupport.webPlatformFeatures || !hevcSupport.platformHEVC) && (
                        <div className="bg-yellow-900/30 border border-yellow-600 rounded p-3">
                          <p className="text-yellow-200 text-sm font-medium mb-2">💡 Enable H.265 support:</p>
                          <div className="text-yellow-100 text-xs space-y-2">
                            {(navigator.userAgent.includes('Chrome') || navigator.userAgent.includes('Edge')) ? (
                              <div>
                                <p className="font-medium">{navigator.userAgent.includes('Chrome') ? 'Chrome/Chromium' : 'Edge'}:</p>
                                <ol className="list-decimal list-inside ml-2 space-y-1">
                                  <li>Go to <a 
                                    href={navigator.userAgent.includes('Chrome') ? 'chrome://flags' : 'edge://flags'} 
                                    className="bg-black/30 px-1 rounded text-blue-300 hover:text-blue-200 underline cursor-pointer"
                                    onClick={(e) => {
                                      e.preventDefault();
                                      window.open(navigator.userAgent.includes('Chrome') ? 'chrome://flags' : 'edge://flags', '_blank');
                                    }}
                                  >
                                    {navigator.userAgent.includes('Chrome') ? 'chrome://flags' : 'edge://flags'}
                                  </a></li>
                                  {!hevcSupport?.webPlatformFeatures && (
                                    <li className="text-orange-200">Enable "Experimental Web Platform features" ⚠️</li>
                                  )}
                                  {!hevcSupport?.platformHEVC && (
                                    <li className="text-orange-200">Enable "Platform HEVCDecoder" (if available) ⚠️</li>
                                  )}
                                  <li>Restart browser and try again</li>
                                </ol>
                              </div>
                            ) : (
                              <p>For other browsers, H.265 support may be limited.</p>
                            )}
                            {hevcSupport?.hardwareSupport === 'none' && (
                              <p className="text-red-300 text-xs mt-2">
                                ⚠️ Your system may not have hardware H.265 decoding support
                              </p>
                            )}
                          </div>
                        </div>
                      )}
                      
                      {hevcSupport && hevcSupport.hardwareSupport === 'probably' && (
                        <div className="bg-green-900/30 border border-green-600 rounded p-3">
                          <p className="text-green-200 text-sm">✅ H.265 support appears to be enabled. The error might be stream-specific.</p>
                        </div>
                      )}
                      
                      <p className="text-gray-300 text-sm">
                        Or copy the stream URL below to use with VLC, MPV, or other media players.
                      </p>
                    </div>
                  ) : (
                    <p className="text-red-200 mb-4">{error}</p>
                  )}
                  
                  <div className="flex justify-center space-x-3">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={copyStreamUrl}
                      className={`text-white border-blue-400 hover:bg-blue-800 ${copySuccess ? 'bg-green-600/20 border-green-400' : ''}`}
                    >
                      {copySuccess ? <Check className="w-4 h-4 mr-2" /> : <Copy className="w-4 h-4 mr-2" />}
                      {copySuccess ? 'Copied!' : 'Copy URL'}
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => {
                        setError(null);
                        if (playerRef.current && channel?.stream_url) {
                          try {
                            // Reload the stream
                            playerRef.current.load();
                          } catch (err) {
                            debug.error('Error retrying video:', err);
                            setError('Failed to retry video playback');
                          }
                        }
                      }}
                      className="text-white border-red-400 hover:bg-red-800"
                    >
                      Retry
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={handleClose}
                      className="text-white border-gray-400 hover:bg-gray-800"
                    >
                      Close
                    </Button>
                  </div>
                </div>
              </div>
            ) : (
              <>
                <VideoPlayerErrorBoundary onError={(err) => setError(err)}>
                  <video
                    ref={videoRef}
                    className="w-full h-full bg-black"
                    controls
                    playsInline
                    preload="metadata"
                    controlsList="nodownload"
                    disablePictureInPicture
                  />
                </VideoPlayerErrorBoundary>
                
                {/* HEVC Support Detection Info - only show if stream is actually HEVC */}
                {hevcSupport && error !== 'codec_unsupported' && isHevcStream && (
                  <div className="absolute bottom-4 right-4 opacity-0 group-hover:opacity-100 transition-opacity duration-300">
                    <div className="bg-black/80 rounded-lg p-3 text-white text-xs max-w-sm">
                      <div className="flex items-center space-x-2 mb-2">
                        <div className={`w-2 h-2 rounded-full ${
                          hevcSupport.hardwareSupport === 'probably' ? 'bg-green-400' :
                          hevcSupport.hardwareSupport === 'maybe' ? 'bg-yellow-400' : 'bg-red-400'
                        }`} />
                        <span className="font-medium">H.265/HEVC Support</span>
                      </div>
                      <div className="text-gray-300">
                        {hevcSupport.hardwareSupport === 'probably' ? 'Hardware acceleration available' :
                         hevcSupport.hardwareSupport === 'maybe' ? 'Limited hardware support' :
                         'Software decoding only'}
                      </div>
                      {hevcSupport.hardwareSupport === 'none' && (
                        <div className="text-yellow-300 text-xs mt-1">
                          <a 
                            href={navigator.userAgent.includes('Chrome') ? 'chrome://flags' : 'edge://flags'}
                            className="text-blue-300 hover:text-blue-200 underline cursor-pointer"
                            onClick={(e) => {
                              e.preventDefault();
                              window.open(navigator.userAgent.includes('Chrome') ? 'chrome://flags' : 'edge://flags', '_blank');
                            }}
                          >
                            Enable in chrome://flags for better performance
                          </a>
                        </div>
                      )}
                    </div>
                  </div>
                )}
                
                {isLoading && (
                  <div className="absolute inset-0 flex items-center justify-center bg-black/50">
                    <div className="text-center text-white">
                      <div className="animate-spin rounded-full h-12 w-12 border-2 border-white border-t-transparent mx-auto mb-4"></div>
                      <p>Loading stream...</p>
                    </div>
                  </div>
                )}
              </>
            )}
          </div>

          {/* Program Description - hidden in fullscreen */}
          {program?.description && !isFullscreen && (
            <div className="p-4 bg-gray-900 text-white border-t border-gray-700">
              <h4 className="font-medium mb-2 text-gray-200">Description</h4>
              <p className="text-sm text-gray-300 leading-relaxed">
                {program.description}
              </p>
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}