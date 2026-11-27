#!/bin/sh
# Runs the market's tests against a real Forgejo in a container: the
# service's own (crates/share-service/src/tests.rs) and the game's, which
# share, suggest and merge from the game (crates/potluck/src/tests.rs).
#
#   deploy/market/test.sh [docker|podman]
#
# The container (potluck-test-forgejo, on port 3300) is left running so the
# next run is quick; remove it with: docker rm -f potluck-test-forgejo
set -eu
run=${1:-docker}
image=${FORGEJO_IMAGE:-codeberg.org/forgejo/forgejo:17}
name=potluck-test-forgejo
port=3300
cd "$(dirname "$0")/../.."
if ! $run container inspect $name >/dev/null 2>&1; then
	$run run -d --name $name -p 127.0.0.1:$port:3000 \
		-e FORGEJO__security__INSTALL_LOCK=true \
		-e FORGEJO__server__ROOT_URL=http://127.0.0.1:$port/ \
		-e FORGEJO__server__DISABLE_SSH=true \
		-e FORGEJO__database__DB_TYPE=sqlite3 \
		"$image" >/dev/null
fi
echo "waiting for Forgejo"
until curl -sf http://127.0.0.1:$port/api/v1/version >/dev/null; do sleep 1; done
token_file=build/market-test-forgejo.token
mkdir -p build
if [ ! -s $token_file ]; then
	$run exec -u git $name forgejo admin user create --admin --username market --email market@test.invalid \
		--password "$(head -c 18 /dev/urandom | base64)" --must-change-password=false >/dev/null 2>&1 || true
	$run exec -u git $name forgejo admin user generate-access-token --username market \
		--token-name "tests-$(date +%s)" --scopes all --raw > $token_file
	chmod 600 $token_file
fi
export POTLUCK_TEST_FORGEJO=http://127.0.0.1:$port
POTLUCK_TEST_FORGEJO_TOKEN=$(cat $token_file)
export POTLUCK_TEST_FORGEJO_TOKEN
cargo test -p potluck-share
cargo test -p potluck -- market qr remix
