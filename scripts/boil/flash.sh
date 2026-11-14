#!/bin/bash
# flash.sh <label> [spot]: a long still run at a spot of spots.txt (default
# c20_strips) for flash.py: 300 captures, every 20 frames from 9000 to 15000,
# under scripts/boil/out/flash/<label>/still. Env passes through
# (SOLARIK_SHADER_DEFS, HALO_RT_EMITTER_MAPS, EXE, ...). About 4 minutes.
#
# run.sh's 40-capture stills can't rank variants for flashes (a glint lit for
# a capture and fading over 80 frames; the strip hall's door, 10 November):
# two runs of one build differ by more than a fix's gain. Over 300 captures
# flash.py's counts repeat within a tenth.
set -u
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
label=${1:?label}; spot=${2:-c20_strips}
exe="${EXE:-$repo/target/release/halo_rt.exe}"
export HALO_MAPS="${HALO_MAPS:-C:\Users\hyrum\Downloads\halo-windows-release\maps}"
read -r name map bsp cam rest <<< "$(grep "^$spot\b" "$here/spots.txt")"
d="$here/out/flash/$label/still"; mkdir -p "$d"; rm -f "$d"/x_*.png
# (the stats text hidden: the user's saved options must not reach the
# measured pictures)
HALO_RT_STATS=0 HALO_RT_GPU_LOG=1 timeout 900 "$exe" "$map" --viewer --bsp "$bsp" --camera "$cam" --screenshot "$d/x.png" --screenshot-every 20 --screenshot-from 9000 --screenshot-after 15000 > "$d/log.txt" 2>&1
echo "$label exit $? $(ls "$d"/x_*.png | wc -l) captures; $(sed 's/\x1b\[[0-9;]*m//g' "$d/log.txt" | grep -a 'gpu [0-9.]* ms' | tail -1 | grep -o 'gpu [0-9.]* ms')"
