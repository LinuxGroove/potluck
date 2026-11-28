# Shared helpers for the snap's hooks, services and commands. Sourced, not run.
#
# Layout:
#   $SNAP/bin                      potluck-share, forgejo, caddy, iroh-relay and these scripts
#   $SNAP/share/words.txt          words names may not use ($SNAP_COMMON/words.txt wins)
#   $SNAP_COMMON/market            the market's index, signing key and files (snap_daemon)
#   $SNAP_COMMON/forgejo           Forgejo: a git repository per creation, its database (snap_daemon)
#   $SNAP_COMMON/secrets           Forgejo's token and secrets (root only)
#   $SNAP_COMMON/caddy             certificates Caddy fetched for `domain`
#   $SNAP_COMMON/                  your own certificate files for the relay, if any
#   $SNAP_DATA/{market,relay,caddy} configs written from the snap's settings at each start

# shellcheck disable=SC2034
set -euo pipefail

# snap_daemon is the shared system user snapd creates for system-usernames.
DAEMON_UID=584788
MARKET="$SNAP_COMMON/market"
FORGEJO="$SNAP_COMMON/forgejo"
SECRETS="$SNAP_COMMON/secrets"
FORGEJO_INI="$FORGEJO/custom/conf/app.ini"
SHARE_TOML="$SNAP_DATA/market/share.toml"

# Ports only this machine uses, next to the market's.
MARKET_PORT=47180
FORGEJO_PORT=47181
RELAY_HTTP_PORT=47182
RELAY_METRICS_PORT=47183
# Games ask the relay for their address the internet sees on this UDP port
# (iroh's default; the game doesn't know any other).
RELAY_QUIC_PORT=7842

export LC_ALL=C.UTF-8
export PATH="$SNAP/bin:$SNAP/usr/bin:$PATH"
LD_LIBRARY_PATH="$SNAP/usr/lib/$(uname -m)-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export LD_LIBRARY_PATH
# git from the snap, not whatever the base has
export GIT_EXEC_PATH="$SNAP/usr/lib/git-core"
export GIT_TEMPLATE_DIR="$SNAP/usr/share/git-core/templates"
export GIT_CONFIG_NOSYSTEM=1

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

# A file setting as a path: relative names are in $SNAP_COMMON.
in_common() {
  case "$1" in
    "") echo "" ;;
    /*) echo "$1" ;;
    *) echo "$SNAP_COMMON/$1" ;;
  esac
}

# A TOML string.
toml_str() {
  local s="$1"
  s="${s//\\/\\\\}"
  s="${s//\"/\\\"}"
  printf '"%s"' "$s"
}

# Runs a command as snap_daemon.
as_daemon() {
  setpriv --clear-groups --reuid "$DAEMON_UID" --regid "$DAEMON_UID" -- "$@"
}

# The address players and browsers use.
public_url() {
  local url domain
  url="$(cfg url)"
  domain="$(cfg domain)"
  if [ -n "$url" ]; then
    echo "${url%/}"
  elif [ -n "$domain" ]; then
    echo "https://$domain"
  else
    echo "http://localhost:$MARKET_PORT"
  fi
}

# The host name in the public address.
public_host() {
  local u
  u="$(public_url)"
  u="${u#*://}"
  u="${u%%/*}"
  echo "${u%%:*}"
}

# The relay's certificate and key on this machine, as two lines (nothing when
# it has none yet): your own files, or the one Caddy keeps for `domain`.
relay_cert() {
  local cert key domain found
  cert="$(cfg relay.cert)"
  key="$(cfg relay.key)"
  if [ -n "$cert" ] && [ -n "$key" ]; then
    in_common "$cert"
    in_common "$key"
    return
  fi
  domain="$(cfg domain)"
  [ -n "$domain" ] || return 0
  found="$(find "$SNAP_COMMON/caddy/caddy/certificates" -path "*/$domain/$domain.crt" 2>/dev/null | head -n 1 || true)"
  if [ -n "$found" ] && [ -f "${found%.crt}.key" ]; then
    echo "$found"
    echo "${found%.crt}.key"
  fi
}

# Whether this machine runs the relay: relay.mode is "here" and it has (or
# will have, from Caddy) a certificate.
relay_here() {
  [ "$(cfg relay.mode here)" = here ] || return 1
  [ -n "$(cfg domain)" ] || { [ -n "$(cfg relay.cert)" ] && [ -n "$(cfg relay.key)" ]; }
}

# The relay's address, given to games ("" for none).
relay_url() {
  local url
  url="$(cfg relay.url)"
  case "$(cfg relay.mode here)" in
    off) echo "" ;;
    elsewhere) echo "${url%/}" ;;
    *)
      if ! relay_here; then
        echo ""
      elif [ -n "$url" ]; then
        echo "${url%/}"
      else
        echo "https://$(public_host):$(cfg relay.port 8443)"
      fi
      ;;
  esac
}

# Forgejo's command line, as snap_daemon, on the snap's own Forgejo.
forgejo_cli() {
  HOME="$FORGEJO/home" USER=snap_daemon as_daemon "$SNAP/bin/forgejo" --config "$FORGEJO_INI" --work-path "$FORGEJO" "$@"
}

# Waits up to $1 seconds for an address to answer.
wait_for() {
  local secs="$1" url="$2" i
  for ((i = 0; i < secs; i++)); do
    if curl -sf -o /dev/null "$url"; then
      return 0
    fi
    sleep 1
  done
  return 1
}
