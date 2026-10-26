#!/usr/bin/env bash
# The M0b bench matrix. Fixtures from fixtures.sh; results in results/*.json.
# Default: hwdec=auto-copy, render at display size, display-synced timing (--sync display).
cd "$(dirname "$0")"
b=./bench.sh
d="--sync display"
$b idle-start            h264-aac-1080p.mp4 --idle on
# Every fixture.
$b mp4-h264-1080p        h264-aac-1080p.mp4 $d
$b mp4-h264-1080p-gop30  h264-aac-1080p-gop30.mp4 $d
$b mov-hevc-1080p        hevc-aac-1080p.mov $d
$b mov-hevc-1080p-rot    hevc-aac-1080p-rot90.mov $d
$b mkv-h264-opus         h264-opus-chapters.mkv $d
$b webm-vp9-opus         vp9-opus-1080p.webm $d
$b mp4-h264-2160p        h264-aac-2160p.mp4 $d
$b mov-hevc-2160p        hevc-aac-2160p.mov $d
$b mov-hevc-2160p-gop30  hevc-aac-2160p-gop30.mov $d
# mpv's default timing (frames timed to the audio clock, not to our vsync), for comparison.
$b audio-h264-1080p      h264-aac-1080p.mp4
$b audio-h264-1080p-2    h264-aac-1080p.mp4
$b audio-hevc-2160p      hevc-aac-2160p.mov
# Render at the video's own size instead of the display size.
$b native-h264-1080p     h264-aac-1080p.mp4 $d --render-size native
$b native-hevc-2160p     hevc-aac-2160p.mov $d --render-size native
# Software decoding.
$b swdec-h264-1080p      h264-aac-1080p.mp4 $d --hwdec no
$b swdec-hevc-2160p      hevc-aac-2160p.mov $d --hwdec no
$b swdec-vp9-1080p       vp9-opus-1080p.webm $d --hwdec no
$b idle-end              h264-aac-1080p.mp4 --idle on
