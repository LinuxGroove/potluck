#!/bin/sh
# Makes a project folder for one of the snaps kept in packaging/<name>/snap:
# this checkout with that snap/ in place of the game's. A repository has
# room for one snap/snapcraft.yaml, and the game has it.
#
#   tools/snap-project.sh potluck-server|potluck-market [build/<name>]
#   cd build/<name> && snapcraft pack
set -eu
cd "$(dirname "$0")/.."
name=${1:?which snap: potluck-server or potluck-market}
if [ ! -f "packaging/$name/snap/snapcraft.yaml" ]; then
	echo "no packaging/$name/snap/snapcraft.yaml" >&2
	exit 1
fi
out=${2:-build/$name}
version=$(tools/version.sh)
rm -rf "$out"
mkdir -p "$out"
git ls-files -z --cached --others --exclude-standard | grep -zv '^snap/' | grep -zv '^build/' | xargs -0 cp --parents -t "$out"
cp -r "packaging/$name/snap" "$out/snap"
# the copy has no git history, so it gets the version as it is now
printf '#!/bin/sh\necho %s\n' "$version" >"$out/tools/version.sh"
echo "$out ($version)"
