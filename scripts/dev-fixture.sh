#!/usr/bin/env bash
# Build a small sample stash for development and the end-to-end tests.
#
#   scripts/dev-fixture.sh <stash-dir>
#
# <stash-dir> must be missing or empty. Needs ffmpeg/ffprobe; uses exiftool for
# photo EXIF and vips for an extra HEIC photo when they are available. Every
# clip is 320x180 and a few seconds long (one is 45 s, long enough for "Continue
# watching"), so the whole stash stays around 2 MB. All titles are invented.
#
# What it creates (paths relative to <stash-dir>):
#   Movies/Paper Boats (2011)/Paper Boats (2011).mp4        H.264/AAC MP4 (direct play)
#   Movies/Glass Orchard (2016)/Glass Orchard (2016).mkv    HEVC/AC-3 MKV (needs HLS)
#   Movies/Tin Lighthouse (2014)/Tin Lighthouse (2014).mp4  VP9/Opus MP4, 45 s (direct play in
#                                                           browsers without H.264, e.g. Playwright's Chromium)
#   Shows/The Lantern Keepers/Season 01/... S01E01, S01E02; Season 02/... S02E01
#   Home Videos/2023/07/IMG_0042.MOV                        QuickTime make/model/creation_time
#   Photos/2022/05/DSC_0101.jpg ... DSC_0103.jpg             EXIF date + camera
#   Photos/2022/08/PXL_20220814_093000.heic                  only when vips can write HEIC
#   Other/Morning Notes.mp3, Other/Recipe Card.pdf
#   Family 2019/beach.mp4        an owner's own folder: indexed in place, never moved
#   Inbox/Harbor.Lights.2019.1080p.WEB-DL.x264-NOGRP.mp4  -> Movies/Harbor Lights (2019)/
#   Inbox/Quiet.Meadow.S01E03.720p.WEB.h264.mp4         -> Shows/Quiet Meadow/Season 01/
#   Inbox/IMG_1234.MOV (creation 2024-06-13)             -> Home Videos/2024/06/
#   Inbox/clip_final_v2.mkv                              ambiguous: waits for review
#
# Every file is back-dated by an hour so the server's startup scan treats the
# Inbox as stable and sorts it straight away.
set -euo pipefail

usage() {
  echo "usage: $0 <stash-dir>" >&2
  exit 2
}

[[ $# -eq 1 ]] || usage
root=$1
if [[ -e "$root" ]] && [[ -n "$(ls -A "$root" 2>/dev/null)" ]]; then
  echo "error: $root exists and is not empty" >&2
  exit 1
fi
for tool in ffmpeg ffprobe; do
  command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
done
mkdir -p "$root"
root=$(cd "$root" && pwd)

ff() { ffmpeg -nostdin -hide_banner -loglevel error -y "$@"; }

# clip <out> <seconds> <video-codec-args...> -- <audio-codec-args...> [-- <extra output args...>]
# A test pattern with a sine tone. The pattern differs per file (seeded by the name)
# so thumbnails and quick hashes differ too.
clip() {
  local out=$1 secs=$2
  shift 2
  local vargs=() aargs=() extra=()
  while [[ $# -gt 0 && $1 != -- ]]; do vargs+=("$1"); shift; done
  [[ $# -gt 0 ]] && shift
  while [[ $# -gt 0 && $1 != -- ]]; do aargs+=("$1"); shift; done
  [[ $# -gt 0 ]] && shift
  extra=("$@")
  local seed freq
  seed=$(( $(printf '%s' "$out" | cksum | cut -d' ' -f1) % 360 ))
  freq=$(( 300 + seed ))
  mkdir -p "$(dirname "$out")"
  ff -f lavfi -i "testsrc2=size=320x180:rate=24:duration=${secs},hue=h=${seed}" \
     -f lavfi -i "sine=frequency=${freq}:sample_rate=48000:duration=${secs}" \
     -map 0:v -map 1:a "${vargs[@]}" "${aargs[@]}" "${extra[@]}" -shortest "$out"
}

h264=(-c:v libx264 -preset veryfast -crf 30 -pix_fmt yuv420p -g 48)
aac=(-c:a aac -b:a 64k -ac 2)

# Movies, already organized.
clip "$root/Movies/Paper Boats (2011)/Paper Boats (2011).mp4" 6 "${h264[@]}" -- "${aac[@]}" -- -movflags +faststart
clip "$root/Movies/Glass Orchard (2016)/Glass Orchard (2016).mkv" 8 \
  -c:v libx265 -preset ultrafast -crf 32 -pix_fmt yuv420p -x265-params log-level=error -- \
  -c:a ac3 -b:a 96k -ac 2
clip "$root/Movies/Tin Lighthouse (2014)/Tin Lighthouse (2014).mp4" 45 \
  -c:v libvpx-vp9 -deadline realtime -cpu-used 8 -b:v 50k -pix_fmt yuv420p -g 48 -- \
  -c:a libopus -b:a 32k -ac 2 -- -movflags +faststart

# A show with two seasons.
show="$root/Shows/The Lantern Keepers"
clip "$show/Season 01/The Lantern Keepers - S01E01.mp4" 4 "${h264[@]}" -- "${aac[@]}"
clip "$show/Season 01/The Lantern Keepers - S01E02.mp4" 4 "${h264[@]}" -- "${aac[@]}"
clip "$show/Season 02/The Lantern Keepers - S02E01.mp4" 4 "${h264[@]}" -- "${aac[@]}"

# A phone video with QuickTime camera metadata. Sets `meta` for clip's extra arguments.
phone_meta() {
  local when=$1 model=$2
  meta=(-movflags +use_metadata_tags
    -metadata "creation_time=${when}"
    -metadata "com.apple.quicktime.make=Apple"
    -metadata "com.apple.quicktime.model=${model}"
    -metadata "com.apple.quicktime.creationdate=${when}"
    -f mov)
}
phone_meta 2023-07-14T10:21:00Z "iPhone 13"
clip "$root/Home Videos/2023/07/IMG_0042.MOV" 4 "${h264[@]}" -- "${aac[@]}" -- "${meta[@]}"

# Photos with EXIF dates and a camera.
photo() {
  local out=$1 when=$2 hue=$3
  mkdir -p "$(dirname "$out")"
  ff -f lavfi -i "testsrc2=size=640x480,hue=h=${hue}" -frames:v 1 -q:v 4 "$out"
  if command -v exiftool >/dev/null; then
    exiftool -q -overwrite_original \
      "-DateTimeOriginal=${when}" "-CreateDate=${when}" \
      "-Make=Fujifilm" "-Model=X-T30" "$out"
  fi
}
photo "$root/Photos/2022/05/DSC_0101.jpg" "2022:05:21 14:03:00" 0
photo "$root/Photos/2022/05/DSC_0102.jpg" "2022:05:21 14:05:30" 120
photo "$root/Photos/2022/05/DSC_0103.jpg" "2022:05:22 09:12:10" 240
if command -v vips >/dev/null; then
  heic_src=$(mktemp --suffix=.jpg)
  ff -f lavfi -i "testsrc2=size=640x480,hue=h=60" -frames:v 1 -q:v 4 "$heic_src"
  mkdir -p "$root/Photos/2022/08"
  if vips copy "$heic_src" "$root/Photos/2022/08/PXL_20220814_093000.heic" 2>/dev/null \
    && [[ -s "$root/Photos/2022/08/PXL_20220814_093000.heic" ]]; then
    echo "added a HEIC photo"
  else
    rm -f "$root/Photos/2022/08/PXL_20220814_093000.heic"
    rmdir "$root/Photos/2022/08" 2>/dev/null || true
    echo "note: this vips cannot write HEIC; skipping the HEIC photo"
  fi
  rm -f "$heic_src"
fi

# Other: audio and a document.
mkdir -p "$root/Other"
ff -f lavfi -i "sine=frequency=523:sample_rate=44100:duration=3" -c:a libmp3lame -b:a 64k \
  "$root/Other/Morning Notes.mp3"
pdf="$root/Other/Recipe Card.pdf"
{
  printf '%%PDF-1.4\n'
  o1=$(( $(printf '%%PDF-1.4\n' | wc -c) ))
  body1='1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n'
  body2='2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n'
  body3='3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >> endobj\n'
  stream='BT /F1 18 Tf 40 100 Td (Pancakes for four) Tj ET'
  body4="4 0 obj << /Length ${#stream} >> stream\n${stream}\nendstream endobj\n"
  body5='5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n'
  offsets=()
  pos=$o1
  for b in "$body1" "$body2" "$body3" "$body4" "$body5"; do
    offsets+=("$pos")
    printf '%b' "$b"
    pos=$(( pos + $(printf '%b' "$b" | wc -c) ))
  done
  printf 'xref\n0 6\n0000000000 65535 f \n'
  for o in "${offsets[@]}"; do printf '%010d 00000 n \n' "$o"; done
  printf 'trailer << /Size 6 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' "$pos"
} > "$pdf"

# The owner's own folder: indexed where it is, never moved.
clip "$root/Family 2019/beach.mp4" 4 "${h264[@]}" -- "${aac[@]}"

# The Inbox.
clip "$root/Inbox/Harbor.Lights.2019.1080p.WEB-DL.x264-NOGRP.mp4" 4 "${h264[@]}" -- "${aac[@]}"
clip "$root/Inbox/Quiet.Meadow.S01E03.720p.WEB.h264.mp4" 4 "${h264[@]}" -- "${aac[@]}"
phone_meta 2024-06-13T10:21:00Z "iPhone 15"
clip "$root/Inbox/IMG_1234.MOV" 3 "${h264[@]}" -- "${aac[@]}" -- "${meta[@]}"
clip "$root/Inbox/clip_final_v2.mkv" 3 "${h264[@]}" -- "${aac[@]}"

# Back-date everything so the Inbox counts as stable at the first scan.
find "$root" -exec touch -h -d '1 hour ago' {} +

echo "sample stash ready in $root ($(du -sh "$root" | cut -f1))"
