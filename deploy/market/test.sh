#!/bin/sh
# Runs the market's tests against a real Forgejo: the service's own
# (crates/share-service/src/tests.rs) and the game's, which share, suggest
# and merge from the game (crates/potluck/src/tests.rs).
#
#   deploy/market/test.sh
#
# Forgejo is its own release, the version the market's snap runs
# (packaging/potluck-market/snap/snapcraft.yaml: change both together),
# checked against its release key and kept in build/forgejo/. It's left
# running on port 3300 so the next run is quick; stop it with
#   kill $(cat build/forgejo/pid)
set -eu
version=17.0.2
port=3300
cd "$(dirname "$0")/../.."
dir=$PWD/build/forgejo
work=$dir/work
ini=$work/custom/conf/app.ini
bin=$dir/forgejo-$version
mkdir -p "$work/custom/conf"

if [ ! -x "$bin" ]; then
	case $(uname -m) in
	x86_64) arch=amd64 ;;
	aarch64) arch=arm64 ;;
	*)
		echo "Forgejo has no build for $(uname -m)" >&2
		exit 1
		;;
	esac
	file=forgejo-$version-linux-$arch
	url=https://code.forgejo.org/forgejo/forgejo/releases/download/v$version/$file
	echo "fetching Forgejo $version"
	curl -fsSL --retry 3 -o "$bin.part" "$url"
	curl -fsSL --retry 3 -o "$bin.asc" "$url.asc"
	gpg --batch --yes --dearmor -o "$dir/forgejo.gpg" packaging/potluck-market/snap/keys/forgejo.asc
	gpgv --keyring "$dir/forgejo.gpg" "$bin.asc" "$bin.part"
	chmod +x "$bin.part"
	mv "$bin.part" "$bin"
fi

forgejo() {
	"$bin" --config "$ini" --work-path "$work" "$@"
}

if ! curl -sf "http://127.0.0.1:$port/api/v1/version" >/dev/null; then
	if [ ! -s "$ini" ]; then
		# Forgejo won't run as root unless told to; this one is only for tests
		root=""
		if [ "$(id -u)" = 0 ]; then
			root="I_AM_BEING_UNSAFE_RUNNING_AS_ROOT = true"
		fi
		cat >"$ini" <<-EOF
			APP_NAME = Potluck market tests
			RUN_MODE = prod
			WORK_PATH = $work
			$root

			[server]
			HTTP_ADDR = 127.0.0.1
			HTTP_PORT = $port
			ROOT_URL = http://127.0.0.1:$port/
			DISABLE_SSH = true
			START_SSH_SERVER = false
			LFS_START_SERVER = false
			OFFLINE_MODE = true

			[database]
			DB_TYPE = sqlite3
			PATH = $work/data/forgejo.db

			[repository]
			ROOT = $work/data/repositories
			DEFAULT_BRANCH = main

			[security]
			INSTALL_LOCK = true
			SECRET_KEY = $("$bin" generate secret SECRET_KEY)
			INTERNAL_TOKEN = $("$bin" generate secret INTERNAL_TOKEN)

			[oauth2]
			ENABLED = false

			[log]
			MODE = file
			LEVEL = Warn
			ROOT_PATH = $work/log
		EOF
	fi
	HOME=$work nohup "$bin" web --config "$ini" --work-path "$work" >"$dir/forgejo.out" 2>&1 &
	echo $! >"$dir/pid"
fi
echo "waiting for Forgejo"
i=0
until curl -sf "http://127.0.0.1:$port/api/v1/version" >/dev/null; do
	i=$((i + 1))
	if [ $i -gt 120 ]; then
		echo "Forgejo didn't start; see $dir/forgejo.out" >&2
		exit 1
	fi
	sleep 1
done

token_file=$dir/token
if [ ! -s "$token_file" ]; then
	HOME=$work forgejo admin user create --admin --username market --email market@test.invalid \
		--random-password --must-change-password=false >/dev/null 2>&1 || true
	HOME=$work forgejo admin user generate-access-token --username market \
		--token-name "tests-$(date +%s)" --scopes all --raw >"$token_file"
	chmod 600 "$token_file"
fi
export POTLUCK_TEST_FORGEJO=http://127.0.0.1:$port
POTLUCK_TEST_FORGEJO_TOKEN=$(cat "$token_file")
export POTLUCK_TEST_FORGEJO_TOKEN
cargo test -p potluck-share
cargo test -p potluck -- market qr remix
