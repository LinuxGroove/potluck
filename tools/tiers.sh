#!/bin/sh
# Screenshots of every lighting tier from a few places, with each one's frame
# rate, for judging looks and speed on a real graphics card.
#
#   tools/tiers.sh [out dir] [size] [game binary]
#
# Defaults: /tmp/potluck-tiers, 2560x1440, target/release/potluck (build it with
# `cargo build --release --bin potluck --features raytracing` to try ray traced
# Ultra). Runs in a scratch world so your own settings and saves stay as they
# are. Writes <view>-<tier>.png and fps.txt (with each one's RAM and
# graphics memory).
set -eu
out=${1:-/tmp/potluck-tiers}
size=${2:-2560x1440}
bin=${3:-target/release/potluck}
mkdir -p "$out"
export POTLUCK_HOME="$out/home"
: > "$out/fps.txt"
# name and --at (x,y,z,yaw,pitch) and --time (0.5 is noon)
views="green:192.5,31,272,0,-14:0.42
paddock:194,31,266,-36,-17:0.42
hills:192,60,330,0,-12:0.42
evening:192.5,31,272,0,-14:0.74"
for tier in low medium high ultra; do
    echo "$views" | while IFS=: read -r name at tod; do
        log="$out/$name-$tier.log"
        "$bin" --shot="$out/$name-$tier.png" --after=12 --at="$at" --time="$tod" --size="$size" --lighting="$tier" --screen=play --no-vsync > "$log" 2>&1 || true
        fps=$(grep -o '[0-9]* frames a second' "$log" | head -1)
        rt=$(grep -o 'lighting ray traced\|without ray tracing' "$log" | head -1)
        # what it held, from the memory report it logs on quitting
        mem=$(grep -o 'Memory: .*' "$log" | tail -1 | sed -E 's/^Memory: RAM ([0-9]+ MB).*GPU ([0-9]+ MB) in [0-9]+ allocations \(([0-9]+ MB) reserved.*/RAM \1, GPU \2 (\3 reserved)/')
        echo "$name $tier: ${fps:-no frame rate} ${rt}${mem:+, $mem}" | tee -a "$out/fps.txt"
    done
done
