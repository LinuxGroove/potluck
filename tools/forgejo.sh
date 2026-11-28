#!/bin/sh
# Fetches Forgejo's own release binary and checks its signature, for the
# market's snap and deploy/market/test.sh, so both run the same version.
#
#   tools/forgejo.sh --version          the version
#   tools/forgejo.sh ARCH OUT           amd64 or arm64, checked, to OUT
#
# The key comes from keyservers and Forgejo's own web key directory every
# time, since Forgejo renews its signing subkey (governance#216) and a
# stale copy can't check newer releases. What's pinned is the release key
# itself: the signature must be good and made by a key whose primary is
# EB11 4F5E 6C0D C2BC DD18 3550 A4B6 1A2D C592 3710.
set -eu
version=17.0.2
fpr=EB114F5E6C0DC2BCDD183550A4B61A2DC5923710
wkd=y84sdmnksfqswe7fxf5mzjg53tbdz8f5 # release@forgejo.org

if [ "${1:-}" = --version ]; then
	echo "$version"
	exit 0
fi
arch=${1:?amd64 or arm64}
out=${2:?where to put it}
case $arch in
amd64 | arm64) ;;
*)
	echo "Forgejo has no build for $arch" >&2
	exit 1
	;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
file=forgejo-$version-linux-$arch
got=""
for site in https://code.forgejo.org https://codeberg.org; do
	url=$site/forgejo/forgejo/releases/download/v$version/$file
	if curl -fsSL --retry 3 -o "$tmp/$file" "$url" && curl -fsSL --retry 3 -o "$tmp/$file.asc" "$url.asc"; then
		got=$site
		break
	fi
done
if [ -z "$got" ]; then
	echo "couldn't download Forgejo $version" >&2
	exit 1
fi

export GNUPGHOME="$tmp/gnupg"
mkdir -m 0700 "$GNUPGHOME"
for key in \
	"https://keys.openpgp.org/vks/v1/by-fingerprint/$fpr" \
	"https://openpgpkey.forgejo.org/.well-known/openpgpkey/forgejo.org/hu/$wkd?l=release" \
	"https://forgejo.org/.well-known/openpgpkey/hu/$wkd?l=release" \
	"https://keyserver.ubuntu.com/pks/lookup?op=get&options=mr&search=0x$fpr"; do
	if curl -fsSL --retry 2 -m 30 -o "$tmp/key" "$key"; then
		gpg --batch --quiet --import "$tmp/key" 2>/dev/null || true
	fi
done

# VALIDSIG's last field is the primary key of whichever (sub)key signed it
if ! gpg --batch --status-fd 1 --verify "$tmp/$file.asc" "$tmp/$file" 2>"$tmp/why" |
	grep -q "^\[GNUPG:\] VALIDSIG .* $fpr\$"; then
	cat "$tmp/why" >&2
	echo "Forgejo $version's signature isn't good, or isn't by Forgejo's release key" >&2
	exit 1
fi
mkdir -p "$(dirname "$out")"
install -m 0755 "$tmp/$file" "$out"
echo "Forgejo $version ($arch) from $got, signed by Forgejo's release key"
