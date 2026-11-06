#!/bin/bash
# views.sh <label> <spot> [view...]: the diagnostic views at a spot of
# spots.txt, raw (HALO_RT_NO_DLSS=1), one capture each, still and (where
# the spot has a dolly reach) in motion, under scripts/boil/out/<label>/<spot>/views/.
# (9000 frames in: a new view's pipelines compile cold, a minute or so, and
# a capture before that shows the raw emissives and "Total 0.00 ms")
# Views (SOLARIK_SHADER_DEFS), all by default:
#   VISUALIZE_DI_PICK:     red = share of the tile candidates with any
#                          contribution, green = share of the cell candidates,
#                          blue = the pick was shadow-tested dark
#   VISUALIZE_CACHE_AGE:   the samples in the cell the diffuse bounce reads,
#                          red one to green full; magenta no cell; black sky
#   VISUALIZE_CELL_LEAK:   red = the cell's position is out of the surface's
#                          sight (behind a wall, inside a rock); green in sight
#   VISUALIZE_CELL_LIGHTS: the lights the pixel's cell offers, a colour each
#                          by share; magenta no cell; black none
#   VISUALIZE_WORLD_CACHE: the cache's irradiance at the surface
# EXTRA_DEFS=A,B adds shader definitions to every view (CELL_LOOKUP_GEOMETRIC:
# the cell lookups by the surface's geometric normal). Env passes through. Also prints each still view's colour classes' shares
# (view_shares.py), for a number beside the picture.
set -u
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
label=${1:?label}; spot=${2:?spot}; shift 2
views=${@:-VISUALIZE_DI_PICK VISUALIZE_CACHE_AGE VISUALIZE_CELL_LEAK VISUALIZE_CELL_LIGHTS}
exe="$repo/target/release/halo_rt.exe"
export HALO_MAPS="${HALO_MAPS:-C:\\Users\\hyrum\\Downloads\\halo-windows-release\\maps}" HALO_RT_NO_DLSS=1
read -r name map bsp cam dolly period < <(grep -v '^#' "$here/spots.txt" | grep -E "^$spot\b")
# (a spot with a sway period of its own: a level flight, see run.sh)
if [ -n "${period:-}" ]; then export HALO_RT_DOLLY_PERIOD=$period HALO_RT_DOLLY_LEVEL=1; motion_at=$((9000 + period / 4)); else motion_at=9030; fi
[ -z "${name:-}" ] && { echo "no spot $spot in spots.txt"; exit 1; }
out="$here/out/$label/$spot/views"; mkdir -p "$out"
for view in $views; do
  SOLARIK_SHADER_DEFS="$view,${EXTRA_DEFS:-}" timeout 600 "$exe" "$map" --viewer --bsp "$bsp" --camera "$cam" --screenshot "$out/${view#VISUALIZE_}_still.png" --screenshot-after 9000 > "$out/${view#VISUALIZE_}_still.log" 2>&1
  python "$here/view_shares.py" "$out/${view#VISUALIZE_}_still.png"
  if [ "${dolly:-0}" != "0" ]; then
    SOLARIK_SHADER_DEFS="$view,${EXTRA_DEFS:-}" HALO_RT_DOLLY_DURING=1 timeout 600 "$exe" "$map" --viewer --bsp "$bsp" --camera "$cam" --dolly "$dolly" --screenshot "$out/${view#VISUALIZE_}_motion.png" --screenshot-after $motion_at > "$out/${view#VISUALIZE_}_motion.log" 2>&1
  fi
done
echo "views: $out"
