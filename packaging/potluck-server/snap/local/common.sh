# Shared helpers for the snap's hooks, services and commands. Sourced, not run.
#
# Layout:
#   $SNAP/bin                    potluck-server, caddy and these scripts
#   $SNAP_COMMON/world           the world and its creations (snap_daemon)
#   $SNAP_COMMON/                your own certificate files, if any
#   $SNAP_COMMON/caddy           certificates Caddy fetched for `domain`
#   $SNAP_DATA/caddy/Caddyfile   the proxy's generated config

# shellcheck disable=SC2034
set -euo pipefail

# snap_daemon is the shared system user snapd creates for system-usernames.
DAEMON_UID=584788
WORLD="$SNAP_COMMON/world"

export LC_ALL=C.UTF-8
export PATH="$SNAP/bin:$SNAP/usr/bin:$PATH"
LD_LIBRARY_PATH="$SNAP/usr/lib/$(uname -m)-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export LD_LIBRARY_PATH

log() {
  echo "$*" >&2
}

fail() {
  echo "$*" >&2
  exit 1
}

# cfg KEY DEFAULT: a snap config value, or DEFAULT when unset.
cfg() {
  local v
  v="$(snapctl get "$1" 2>/dev/null || true)"
  if [ -z "$v" ]; then
    echo "${2:-}"
  else
    echo "$v"
  fi
}

# A certificate setting as a path: relative names are in $SNAP_COMMON.
in_common() {
  case "$1" in
    "") echo "" ;;
    /*) echo "$1" ;;
    *) echo "$SNAP_COMMON/$1" ;;
  esac
}
