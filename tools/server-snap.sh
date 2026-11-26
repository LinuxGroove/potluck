#!/bin/sh
# Makes a project folder for the potluck-server snap: this checkout with the
# server's snap/ (packaging/potluck-server/snap) in place of the game's.
# A repository has room for one snap/snapcraft.yaml, and the game has it.
#
#   tools/server-snap.sh [build/potluck-server]
#   cd build/potluck-server && snapcraft pack
set -eu
cd "$(dirname "$0")/.."
out=${1:-build/potluck-server}
version=$(tools/version.sh)
rm -rf "$out"
mkdir -p "$out"
git ls-files -z --cached --others --exclude-standard | grep -zv '^snap/' | grep -zv '^build/' | xargs -0 cp --parents -t "$out"
cp -r packaging/potluck-server/snap "$out/snap"
# the copy has no git history, so it gets the version as it is now
printf '#!/bin/sh\necho %s\n' "$version" >"$out/tools/version.sh"
echo "$out ($version)"
