#!/usr/bin/env bash
# M0b fixtures: 30 s test videos with burned-in timecode, frame number and a 0.1 s white
# flash + 1 kHz beep at every full second (check A/V sync and seek accuracy by eye/ear).
# Output: %TEMP%\gg-spike\video. Needs ffmpeg 7.x on PATH.
set -euo pipefail
OUT="${OUT:-$(cygpath -u "$TEMP")/gg-spike/video}"
mkdir -p "$OUT"
cd "$OUT"

FONT="C\:/Windows/Fonts/consola.ttf"
vsrc() { # $1 = WxH
  echo "testsrc2=size=$1:rate=30:duration=30,\
drawtext=fontfile='$FONT':text='%{pts\:hms}  #%{n}':fontsize=h/12:fontcolor=white:box=1:boxcolor=black@0.6:x=(w-tw)/2:y=h/2-th,\
drawbox=x=0:y=0:w=iw:h=ih/10:color=white:t=fill:enable='lt(mod(t\,1)\,0.1)',format=yuv420p"
}
asrc="sine=frequency=440:beep_factor=2.27:sample_rate=48000:duration=30"
# beep_factor beeps at 440*2.27 ≈ 1 kHz for the first 0.04 s of every second.

gen() { # $1 = out, $2 = size, rest = codec args
  local out=$1 size=$2; shift 2
  [ -f "$out" ] && { echo "skip $out"; return; }
  echo "== $out"
  ffmpeg -hide_banner -loglevel error -y -f lavfi -i "$(vsrc "$size")" -f lavfi -i "$asrc" -shortest "$@" "$out"
}

gen h264-aac-1080p.mp4  1920x1080 -c:v libx264 -preset fast -crf 20 -c:a aac -b:a 160k -movflags +faststart
gen hevc-aac-1080p.mov  1920x1080 -c:v libx265 -x265-params log-level=error -preset fast -crf 24 -tag:v hvc1 -c:a aac -b:a 160k
gen hevc-aac-2160p.mov  3840x2160 -c:v libx265 -x265-params log-level=error -preset ultrafast -crf 24 -tag:v hvc1 -c:a aac -b:a 160k
gen h264-aac-2160p.mp4  3840x2160 -c:v libx264 -preset veryfast -crf 22 -c:a aac -b:a 160k
gen vp9-opus-1080p.webm 1920x1080 -c:v libvpx-vp9 -deadline realtime -cpu-used 8 -row-mt 1 -b:v 6M -c:a libopus -b:a 128k
# Phone-like GOP (1 s); the others use the encoder default (x264: 250 frames), a worst case for exact seeks.
gen h264-aac-1080p-gop30.mp4 1920x1080 -c:v libx264 -preset fast -crf 20 -g 30 -c:a aac -b:a 160k -movflags +faststart
gen hevc-aac-2160p-gop30.mov 3840x2160 -c:v libx265 -x265-params log-level=error -preset ultrafast -crf 24 -g 30 -tag:v hvc1 -c:a aac -b:a 160k

# MKV with chapters (every 10 s).
if [ ! -f h264-opus-chapters.mkv ]; then
  echo "== h264-opus-chapters.mkv"
  cat > chapters.txt <<'META'
;FFMETADATA1
title=Chapter fixture
[CHAPTER]
TIMEBASE=1/1000
START=0
END=10000
title=One
[CHAPTER]
TIMEBASE=1/1000
START=10000
END=20000
title=Two
[CHAPTER]
TIMEBASE=1/1000
START=20000
END=30000
title=Three
META
  ffmpeg -hide_banner -loglevel error -y -f lavfi -i "$(vsrc 1920x1080)" -f lavfi -i "$asrc" -i chapters.txt \
    -map 0:v -map 1:a -map_metadata 2 -map_chapters 2 -shortest \
    -c:v libx264 -preset fast -crf 20 -c:a libopus -b:a 128k h264-opus-chapters.mkv
fi

# Phone portrait: landscape-coded frames with a display matrix rotation, like an iPhone .MOV.
if [ ! -f hevc-aac-1080p-rot90.mov ]; then
  echo "== hevc-aac-1080p-rot90.mov"
  ffmpeg -hide_banner -loglevel error -y -display_rotation:v:0 -90 -i hevc-aac-1080p.mov -c copy hevc-aac-1080p-rot90.mov
fi

for f in *.mp4 *.mov *.mkv *.webm; do
  printf '%-28s %8s  ' "$f" "$(du -h "$f" | cut -f1)"
  ffprobe -v error -show_entries stream=codec_name,width,height:stream_side_data=rotation:format=duration -of compact=p=0:nk=1 "$f" | tr '\n' ' '
  echo
done
