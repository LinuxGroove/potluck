#!/bin/sh
# The benchmark gallery: the same kinds of scene as Ken's seven Hytale
# shots (2026-11-27), at every lighting tier with frame rates, to put beside
# them after each pass on the looks. The camp, the farm and the villa are
# built for them (crates/potluck/src/world/places.rs).
#
#   tools/gallery.sh [out dir] [size] [game binary] [tiers]
#
# Defaults: /tmp/potluck-gallery, 2560x1440, target/release/potluck (build it
# with `cargo build --release --bin potluck --features raytracing`), and
# "low medium high ultra". Runs in a scratch world. Writes <view>-<tier>.png
# and fps.txt (with each one's RAM and graphics memory). A height of 0 or
# less in --at is that far above the ground.
set -eu
out=${1:-/tmp/potluck-gallery}
size=${2:-2560x1440}
bin=${3:-target/release/potluck}
tiers=${4:-low medium high ultra}
mkdir -p "$out"
export POTLUCK_HOME="$out/home"
: > "$out/fps.txt"
# name, --at (x,y,z,yaw,pitch) and --time (0.25 sunrise, 0.5 noon, 0.75 sunset)
# (the villa's views give a height: they're under a roof or on one)
views="1-rooftop-sunset:58,33.6,212,80,-6:0.735
2-misty-forest:200,-1.7,142,0,4:0.3
3-camp-meadow:291,-1.7,328,90,2:0.47
4-camp-above:300,-44,360,40,-22:0.72
5-balcony-dusk:46.4,27.4,207.2,82,-3:0.745
6-library:61,27.6,206,90,4:0.6
7-farm-night:241,-1.7,309,90,8:0.93"
for tier in $tiers; do
    echo "$views" | while IFS=: read -r name at tod; do
        log="$out/$name-$tier.log"
        "$bin" --shot="$out/$name-$tier.png" --after=14 --at="$at" --time="$tod" --size="$size" --lighting="$tier" --screen=play --no-vsync > "$log" 2>&1 || true
        fps=$(grep -o '[0-9]* frames a second' "$log" | head -1)
        rt=$(grep -o 'lighting ray traced\|without ray tracing' "$log" | head -1)
        # what it held, from the memory report it logs on quitting
        mem=$(grep -o 'Memory: .*' "$log" | tail -1 | sed -E 's/^Memory: RAM ([0-9]+ MB).*GPU ([0-9]+ MB) in [0-9]+ allocations \(([0-9]+ MB) reserved.*/RAM \1, GPU \2 (\3 reserved)/')
        echo "$name $tier: ${fps:-no frame rate} ${rt}${mem:+, $mem}" | tee -a "$out/fps.txt"
    done
done
