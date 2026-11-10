#!/bin/bash
# run.sh <label> [spot...]: the boiling measures at each spot of spots.txt
# (all of them by default) in the level viewer, as one table. Env passes
# through (SOLARIK_SHADER_DEFS, HALO_RT_NO_DLSS, HALO_RT_NO_PVS_LIGHTS, ...).
#
# For each spot, three runs of halo_rt --viewer at the spot's BSP and camera:
#   still:  captures every 40 frames from 9000 to 10600 (blotch wander, 3-shot)
#   series: captures every 4 frames from 9600 to 9760 (noise.py)
#   motion: --dolly <reach> with HALO_RT_DOLLY_DURING=1, a capture mid-sweep
#           every sweep from 9030 to 13710 (blotch wander, 3-shot in motion);
#           skipped when the spot's dolly is 0
# (all past frame 9000: the pipelines compile cold in every process once the
# driver's shader cache stops storing them, as it did on 4 November with
# 15 GB in it, and captures before that are unlit)
# and the GPU pass times from HALO_RT_GPU_LOG on the still run.
# Output under scripts/boil/out/<label>/<spot>/<kind>/; the table in
# scripts/boil/out/<label>/table.txt.
#
# MODES=still,series,motion picks the runs; FRAMES_SCALE=0.5 shortens them.
set -u
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
label=${1:?label}; shift
exe="${EXE:-$repo/target/release/halo_rt.exe}"
export HALO_MAPS="${HALO_MAPS:-C:\\Users\\hyrum\\Downloads\\halo-windows-release\\maps}"
modes=${MODES:-still,series,motion}
out="$here/out/$label"; mkdir -p "$out"
table="$out/table.txt"; : > "$table"

spots=$(grep -v '^#' "$here/spots.txt" | grep -v '^\s*$')
if [ $# -gt 0 ]; then
  spots=$(echo "$spots" | grep -E "^($(echo "$@" | tr ' ' '|'))\b")
fi

run() { # run <dir> <args...>: one viewer run, its log in <dir>/log.txt
  local dir=$1; shift
  mkdir -p "$dir"; rm -f "$dir"/x_*.png
  timeout 900 "$exe" "$@" > "$dir/log.txt" 2>&1
}
# (two spans summed: the specular pass is two stages with NRC's specular query)
pass2() { python -c "import sys; a=sys.argv[1]; print(round(float(a) if a else 0, 2) + (float(sys.argv[2]) if sys.argv[2] else 0))" "$(pass "$1" "$2")" "$(pass "$1" "$3")"; }
gpu_line() { # the last GPU pass log of a run
  sed 's/\x1b\[[0-9;]*m//g' "$1/log.txt" | grep -a 'gpu [0-9.]* ms' | tail -1 | sed 's/.*halo_rt: //'
}
pass() { # pass <line> <name>: one pass's ms from a GPU log line
  echo "$1" | grep -o "$2 [0-9.]*" | head -1 | awk '{print $2}'
}

# (a warm-up run first: with new shader defs the pipelines compile cold, a
# minute or so, and captures before that show the raw emissives; the driver
# keeps the compiled pipelines for the runs after. WARM=0 skips it.)
if [ "${WARM:-0}" != "0" ]; then
  read -r name map bsp cam dolly period <<< "$(echo "$spots" | head -1)"
  run "$out/warm" "$map" --viewer --bsp "$bsp" --camera "$cam" --screenshot "$out/warm/x.png" --screenshot-after 9000
fi

printf '%-13s %6s %6s %6s %6s %6s %6s | %7s %7s %7s | %7s %7s | %7s %7s\n' spot gpu direct cache diffGI specGI dlss stillWdr stillP90 still3s noise noiseP95 moveWdr move3s >> "$table"
while read -r name map bsp cam dolly period; do
  d="$out/$name"
  gpu=; direct=; cache=; dgi=; sgi=; dlss=; sw=; sp=; s3=; nz=; np=; mw=; m3=
  if [[ ",$modes," == *,still,* ]]; then
    HALO_RT_GPU_LOG=1 run "$d/still" "$map" --viewer --bsp "$bsp" --camera "$cam" --screenshot "$d/still/x.png" --screenshot-every 40 --screenshot-from 9000 --screenshot-after 10600
    line=$(gpu_line "$d/still")
    gpu=$(echo "$line" | grep -o 'gpu [0-9.]*' | awk '{print $2}')
    direct=$(pass "$line" direct_lighting); cache=$(pass "$line" world_cache)
    dgi=$(pass "$line" diffuse_indirect_lighting); sgi=$(pass2 "$line" specular_indirect_lighting specular_indirect_reuse); dlss=$(pass "$line" dlss_ray_reconstruction)
    b=$(python "$here/blotch.py" "$d/still"); sw=$(echo "$b" | grep -o 'wander mean [0-9.]*' | awk '{print $3}'); sp=$(echo "$b" | grep -o 'p90 [0-9.]*' | head -1 | awk '{print $2}')
    s3=$(python "$here/three_shot.py" "$d/still" | grep -o 'mean [0-9.]*' | awk '{print $2}')
  fi
  if [[ ",$modes," == *,series,* ]]; then
    run "$d/series" "$map" --viewer --bsp "$bsp" --camera "$cam" --screenshot "$d/series/x.png" --screenshot-every 4 --screenshot-from 9600 --screenshot-after 9760
    n=$(python "$here/noise.py" "$d/series"); nz=$(echo "$n" | grep -o 'noise mean [0-9.]*' | awk '{print $3}'); np=$(echo "$n" | grep -o 'p95 [0-9.]*' | awk '{print $2}')
  fi
  if [[ ",$modes," == *,motion,* ]] && [ "${dolly:-0}" != "0" ]; then
    # (a period of its own: a level flight, the mid-sweep capture a quarter
    # period in, seven of them)
    if [ -n "${period:-}" ]; then
      HALO_RT_DOLLY_DURING=1 HALO_RT_DOLLY_PERIOD=$period HALO_RT_DOLLY_LEVEL=1 run "$d/motion" "$map" --viewer --bsp "$bsp" --camera "$cam" --dolly "$dolly" --screenshot "$d/motion/x.png" --screenshot-every "$period" --screenshot-from $((9000 + period / 4)) --screenshot-after $((9000 + period / 4 + 6 * period))
    else
      HALO_RT_DOLLY_DURING=1 run "$d/motion" "$map" --viewer --bsp "$bsp" --camera "$cam" --dolly "$dolly" --screenshot "$d/motion/x.png" --screenshot-every 120 --screenshot-from 9030 --screenshot-after 13710
    fi
    b=$(python "$here/blotch.py" "$d/motion"); mw=$(echo "$b" | grep -o 'wander mean [0-9.]*' | awk '{print $3}')
    m3=$(python "$here/three_shot.py" "$d/motion" | grep -o 'mean [0-9.]*' | awk '{print $2}')
  fi
  printf '%-13s %6s %6s %6s %6s %6s %6s | %7s %7s %7s | %7s %7s | %7s %7s\n' "$name" "$gpu" "$direct" "$cache" "$dgi" "$sgi" "$dlss" "$sw" "$sp" "$s3" "$nz" "$np" "$mw" "$m3" | tee -a "$table"
done <<< "$spots"
echo "table: $table"
