#!/bin/bash
set -e

echo "=== LinuxServer FFmpeg Base + M3U Proxy Test Script ===" 
echo ""

echo "1. Checking host GPU devices..."
ls -la /dev/dri/
echo ""

echo "2. Checking group memberships..."
echo "Render group: $(getent group render | cut -d: -f3)"
echo "Video group: $(getent group video | cut -d: -f3)"
echo ""

# Use the LinuxServer FFmpeg base image
IMAGE_TAG="m3u-proxy:latest"

echo "Testing LinuxServer FFmpeg base container with m3u-proxy..."
echo "Image: $IMAGE_TAG"
echo ""

echo "3. Testing LinuxServer FFmpeg capabilities..."
echo ""

echo "3a. Available hardware accelerators..."
if podman run --rm --entrypoint=/usr/local/bin/ffmpeg "$IMAGE_TAG" -hwaccels 2>/dev/null; then
    echo "Hardware accelerators detected"
else
    echo "Could not detect hardware accelerators"
fi
echo ""

echo "3b. Available encoders (filtering for hardware)..."
echo "AMD AMF encoders:"
podman run --rm --entrypoint=/usr/local/bin/ffmpeg "$IMAGE_TAG" -encoders 2>/dev/null | grep -E "(amf)" || echo "No AMD AMF encoders found"
echo "VAAPI encoders:"
podman run --rm --entrypoint=/usr/local/bin/ffmpeg "$IMAGE_TAG" -encoders 2>/dev/null | grep -E "(vaapi)" || echo "No VAAPI encoders found"
echo "NVIDIA encoders:"
podman run --rm --entrypoint=/usr/local/bin/ffmpeg "$IMAGE_TAG" -encoders 2>/dev/null | grep -E "(nvenc)" || echo "No NVIDIA encoders found"
echo "Intel encoders:"
podman run --rm --entrypoint=/usr/local/bin/ffmpeg "$IMAGE_TAG" -encoders 2>/dev/null | grep -E "(qsv)" || echo "No Intel QSV encoders found"
echo ""

echo "3c. Available decoders (filtering for hardware)..."
echo "AMD decoders:"
podman run --rm --entrypoint=/usr/local/bin/ffmpeg "$IMAGE_TAG" -decoders 2>/dev/null | grep -E "(vaapi)" | head -5 || echo "No AMD VAAPI decoders found"
echo "NVIDIA decoders:"
podman run --rm --entrypoint=/usr/local/bin/ffmpeg "$IMAGE_TAG" -decoders 2>/dev/null | grep -E "(nvdec|cuvid)" | head -5 || echo "No NVIDIA decoders found"
echo ""

echo "4. Testing VAAPI with different device paths..."
echo ""

echo "4a. Testing with renderD128..."
if podman run --rm --device=/dev/dri:/dev/dri --group-add $(getent group render | cut -d: -f3) --group-add $(getent group video | cut -d: -f3) --entrypoint=/usr/local/bin/ffmpeg "$IMAGE_TAG" -v error -init_hw_device vaapi=gpu:/dev/dri/renderD128 -f lavfi -i testsrc2=duration=1:size=320x240:rate=1 -vf hwupload,scale_vaapi=640x480 -c:v h264_vaapi -f null - 2>&1; then
    echo "✅ renderD128 VAAPI SUCCESS"
else
    echo "❌ renderD128 VAAPI FAILED"
fi
echo ""

echo "4b. Testing with card0..."
if podman run --rm --device=/dev/dri:/dev/dri --group-add $(getent group render | cut -d: -f3) --group-add $(getent group video | cut -d: -f3) --entrypoint=/usr/local/bin/ffmpeg "$IMAGE_TAG" -v error -init_hw_device vaapi=gpu:/dev/dri/card0 -f lavfi -i testsrc2=duration=1:size=320x240:rate=1 -vf hwupload,scale_vaapi=640x480 -c:v h264_vaapi -f null - 2>&1; then
    echo "✅ card0 VAAPI SUCCESS"
else
    echo "❌ card0 VAAPI FAILED"
fi
echo ""

echo "4c. Testing AMD-specific VAAPI with auto-detection..."
if podman run --rm --device=/dev/dri:/dev/dri --group-add $(getent group render | cut -d: -f3) --group-add $(getent group video | cut -d: -f3) --entrypoint=/usr/local/bin/ffmpeg "$IMAGE_TAG" -v error -vaapi_device /dev/dri/renderD128 -f lavfi -i testsrc2=duration=1:size=320x240:rate=1 -vf format=nv12,hwupload -c:v h264_vaapi -f null - 2>&1; then
    echo "✅ AMD VAAPI auto-detection SUCCESS"
else
    echo "❌ AMD VAAPI auto-detection FAILED"
fi
echo ""

echo "5. Testing software encoding performance..."
echo "5a. H.264 software encoding test..."
if time podman run --rm --entrypoint=/usr/local/bin/ffmpeg "$IMAGE_TAG" -v error -f lavfi -i testsrc2=duration=5:size=1920x1080:rate=30 -c:v libx264 -preset ultrafast -f null - 2>&1; then
    echo "✅ Software H.264 encoding works"
else
    echo "❌ Software H.264 encoding failed"
fi
echo ""

echo "5b. H.265 software encoding test..."
if time podman run --rm --entrypoint=/usr/local/bin/ffmpeg "$IMAGE_TAG" -v error -f lavfi -i testsrc2=duration=2:size=1280x720:rate=30 -c:v libx265 -preset ultrafast -f null - 2>&1; then
    echo "✅ Software H.265 encoding works"  
else
    echo "❌ Software H.265 encoding failed"
fi
echo ""

echo "6. Container analysis..."
echo "6a. Binary info..."
echo "LinuxServer FFmpeg container with shell:"
podman run --rm --entrypoint=/bin/bash "$IMAGE_TAG" -c "ls -lh /usr/local/bin/ffmpeg /usr/local/bin/ffprobe /app/m3u-proxy"
echo ""

echo "6b. FFmpeg version and features..."
podman run --rm --entrypoint=/usr/local/bin/ffmpeg "$IMAGE_TAG" -version | head -5
echo ""

echo "6c. M3U Proxy application test..."
echo "Testing m3u-proxy binary:"
podman run --rm --entrypoint=/app/m3u-proxy "$IMAGE_TAG" --help | head -5
echo ""

echo "=== LinuxServer FFmpeg + M3U Proxy Test Complete ==="